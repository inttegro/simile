use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, to_bytes};
use axum::extract::{ConnectInfo, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, HeaderName, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use serde_json::json;
use simile_learning::{Direction, ExchangeInput, ObservedMessage, Session};
use tokio::net::TcpListener;

use crate::{Error, Result};

const BODY_LIMIT: usize = 16 * 1024 * 1024;
const BODY_LIMIT_U64: u64 = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct ProxyOptions {
    pub upstream: String,
    pub relay_path: Option<String>,
    pub webhook_destination: Option<String>,
    pub learning: Option<Session>,
}

pub struct ProxyServer {
    listener: TcpListener,
    router: Router,
    address: SocketAddr,
}

struct ProxyState {
    agent: ureq::Agent,
    upstream: String,
    relay_path: Option<String>,
    webhook_destination: Option<String>,
    learning: Option<Session>,
}

struct ForwardedResponse {
    status: u16,
    headers: HeaderMap,
    body: Vec<u8>,
}

struct ProxyAccess<'a> {
    direction: Direction,
    method: &'a Method,
    path: &'a str,
    status: StatusCode,
    peer: Option<SocketAddr>,
    request_bytes: usize,
    response_bytes: usize,
    elapsed: Duration,
}

impl ProxyServer {
    pub async fn bind(address: SocketAddr, options: ProxyOptions) -> Result<Self> {
        let listener = TcpListener::bind(address)
            .await
            .map_err(|source| Error::Bind { address, source })?;
        let address = listener.local_addr().map_err(Error::LocalAddress)?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();
        let state = Arc::new(ProxyState {
            agent,
            upstream: options.upstream.trim_end_matches('/').to_owned(),
            relay_path: options.relay_path,
            webhook_destination: options.webhook_destination,
            learning: options.learning,
        });
        let router = Router::new()
            .route("/.simile/health", any(health))
            .route("/.simile/learning", any(learning_status))
            .fallback(proxy)
            .with_state(state);
        Ok(Self {
            listener,
            router,
            address,
        })
    }

    #[must_use]
    pub const fn local_addr(&self) -> SocketAddr {
        self.address
    }

    pub async fn run(self) -> Result<()> {
        self.run_until(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::warn!(%error, "could not install Ctrl-C handler");
            }
        })
        .await
    }

    pub async fn run_until<F>(self, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        axum::serve(
            self.listener,
            self.router
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(Error::Serve)
    }
}

async fn health() -> Response {
    Json(json!({ "status": "ok" })).into_response()
}

async fn learning_status(State(state): State<Arc<ProxyState>>) -> Response {
    match &state.learning {
        Some(session) => Json(json!({
            "enabled": true,
            "exchanges": session.exchange_count()
        }))
        .into_response(),
        None => Json(json!({ "enabled": false })).into_response(),
    }
}

async fn proxy(
    State(state): State<Arc<ProxyState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request<Body>,
) -> Response {
    let direction = if state.relay_path.as_deref() == Some(request.uri().path()) {
        Direction::Webhook
    } else if request.uri().path().starts_with("/.simile/") {
        return error_response(
            StatusCode::NOT_FOUND,
            "simile_route_not_found",
            "Simile has no internal route at this path",
        );
    } else {
        Direction::Api
    };
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let query = request.uri().query().map(str::to_owned);
    let request_headers = request.headers().clone();
    let request_content_type = content_type(&request_headers);
    let started = Instant::now();
    let body = match to_bytes(request.into_body(), BODY_LIMIT).await {
        Ok(body) => body,
        Err(_) => {
            return error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_too_large",
                "request body exceeds the proxy limit",
            );
        }
    };
    let observed_request = ObservedMessage::capture(
        observed_headers(&request_headers),
        request_content_type,
        &body,
    );
    let target = match direction {
        Direction::Api => format!(
            "{}{}{}",
            state.upstream,
            path,
            query
                .as_ref()
                .map(|query| format!("?{query}"))
                .unwrap_or_default()
        ),
        Direction::Webhook => match &state.webhook_destination {
            Some(destination) => destination.clone(),
            None => {
                let status = StatusCode::NO_CONTENT;
                let response = ObservedMessage::capture(std::iter::empty(), None, &[]);
                record(
                    &state,
                    ExchangeInput {
                        direction,
                        method: method.to_string(),
                        path: path.clone(),
                        query,
                        request: observed_request,
                        status: status.as_u16(),
                        response,
                        elapsed: started.elapsed(),
                    },
                );
                access_log(ProxyAccess {
                    direction,
                    method: &method,
                    path: &path,
                    status,
                    peer: Some(peer),
                    request_bytes: body.len(),
                    response_bytes: 0,
                    elapsed: started.elapsed(),
                });
                return status.into_response();
            }
        },
    };
    let agent = state.agent.clone();
    let forwarded_method = method.clone();
    let forwarded_headers = request_headers.clone();
    let forwarded_body = body.to_vec();
    let result = tokio::task::spawn_blocking(move || {
        forward(
            &agent,
            &forwarded_method,
            &target,
            &forwarded_headers,
            forwarded_body,
        )
    })
    .await;
    let forwarded = match result {
        Ok(Ok(forwarded)) => forwarded,
        Ok(Err(error)) => {
            tracing::warn!(target: "simile::proxy", %error, direction = direction_label(direction), %method, %path, "upstream request failed");
            access_log(ProxyAccess {
                direction,
                method: &method,
                path: &path,
                status: StatusCode::BAD_GATEWAY,
                peer: Some(peer),
                request_bytes: body.len(),
                response_bytes: 0,
                elapsed: started.elapsed(),
            });
            return error_response(
                StatusCode::BAD_GATEWAY,
                "proxy_upstream_failed",
                "the upstream service could not be reached",
            );
        }
        Err(error) => {
            tracing::error!(target: "simile::proxy", %error, direction = direction_label(direction), %method, %path, "proxy worker failed");
            access_log(ProxyAccess {
                direction,
                method: &method,
                path: &path,
                status: StatusCode::BAD_GATEWAY,
                peer: Some(peer),
                request_bytes: body.len(),
                response_bytes: 0,
                elapsed: started.elapsed(),
            });
            return error_response(
                StatusCode::BAD_GATEWAY,
                "proxy_worker_failed",
                "the proxy could not complete the upstream request",
            );
        }
    };
    let status = StatusCode::from_u16(forwarded.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let response_content_type = content_type(&forwarded.headers);
    let observed_response = ObservedMessage::capture(
        observed_headers(&forwarded.headers),
        response_content_type,
        &forwarded.body,
    );
    record(
        &state,
        ExchangeInput {
            direction,
            method: method.to_string(),
            path: path.clone(),
            query,
            request: observed_request,
            status: status.as_u16(),
            response: observed_response,
            elapsed: started.elapsed(),
        },
    );
    access_log(ProxyAccess {
        direction,
        method: &method,
        path: &path,
        status,
        peer: Some(peer),
        request_bytes: body.len(),
        response_bytes: forwarded.body.len(),
        elapsed: started.elapsed(),
    });
    outgoing_response(status, forwarded.headers, forwarded.body)
}

fn forward(
    agent: &ureq::Agent,
    method: &Method,
    target: &str,
    headers: &HeaderMap,
    body: Vec<u8>,
) -> std::result::Result<ForwardedResponse, String> {
    let mut request = ureq::http::Request::builder()
        .method(method.as_str())
        .uri(target);
    let outgoing_headers = request
        .headers_mut()
        .ok_or_else(|| "could not construct upstream request headers".to_owned())?;
    copy_headers(headers, outgoing_headers);
    let request = request.body(body).map_err(|error| error.to_string())?;
    let response = agent.run(request).map_err(|error| error.to_string())?;
    let (parts, mut body) = response.into_parts();
    let bytes = body
        .with_config()
        .limit(BODY_LIMIT_U64)
        .read_to_vec()
        .map_err(|error| error.to_string())?;
    Ok(ForwardedResponse {
        status: parts.status.as_u16(),
        headers: parts.headers,
        body: bytes,
    })
}

fn copy_headers(source: &HeaderMap, destination: &mut HeaderMap) {
    for (name, value) in source {
        if !hop_by_hop(name) {
            destination.append(name.clone(), value.clone());
        }
    }
}

fn hop_by_hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
            | "host"
    )
}

fn outgoing_response(status: StatusCode, headers: HeaderMap, body: Vec<u8>) -> Response {
    let mut response = (status, Body::from(body)).into_response();
    copy_headers(&headers, response.headers_mut());
    response
}

fn observed_headers(headers: &HeaderMap) -> impl Iterator<Item = (String, Vec<String>)> + '_ {
    let mut values = BTreeMap::<String, Vec<String>>::new();
    for name in headers.keys() {
        let collected = headers
            .get_all(name)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !collected.is_empty() {
            values.insert(name.as_str().to_owned(), collected);
        }
    }
    values.into_iter()
}

fn content_type(headers: &HeaderMap) -> Option<String> {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn record(state: &ProxyState, exchange: ExchangeInput) {
    let Some(session) = &state.learning else {
        return;
    };
    if let Err(error) = session.record(exchange) {
        tracing::warn!(target: "simile::learning", %error, "could not record exchange; proxy delivery continued");
    }
}

fn access_log(access: ProxyAccess<'_>) {
    tracing::info!(
        target: "simile::proxy",
        direction = direction_label(access.direction),
        method = %access.method,
        path = %access.path,
        status = access.status.as_u16(),
        peer = access.peer.map(|value| value.ip().to_string()).as_deref().unwrap_or("-"),
        request_bytes = access.request_bytes,
        response_bytes = access.response_bytes,
        elapsed_us = access.elapsed.as_micros(),
        "proxied exchange"
    );
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "code": code,
                "message": message
            }
        })),
    )
        .into_response()
}

const fn direction_label(direction: Direction) -> &'static str {
    match direction {
        Direction::Api => "api",
        Direction::Webhook => "webhook",
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener as StdTcpListener;
    use std::thread;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;

    use super::*;

    fn upstream(response: &'static str) -> (String, thread::JoinHandle<String>) {
        let listener =
            StdTcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("{error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("{error}"));
        let task = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap_or_else(|error| panic!("{error}"));
            let request = read_http_request(&mut stream);
            stream
                .write_all(response.as_bytes())
                .unwrap_or_else(|error| panic!("{error}"));
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{address}"), task)
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        loop {
            let mut chunk = [0_u8; 4_096];
            let read = stream
                .read(&mut chunk)
                .unwrap_or_else(|error| panic!("{error}"));
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
            let Some(headers_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
            else {
                continue;
            };
            let body_start = headers_end + 4;
            let content_length = String::from_utf8_lossy(&request[..headers_end])
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if request.len() >= body_start + content_length {
                break;
            }
        }
        request
    }

    #[tokio::test]
    async fn forwards_api_requests_and_records_sanitized_evidence() {
        let (upstream, upstream_task) = upstream(concat!(
            "HTTP/1.1 201 Created\r\n",
            "Content-Type: application/json\r\n",
            "Content-Length: 29\r\n",
            "Connection: close\r\n\r\n",
            "{\"id\":\"cu_1234\",\"name\":\"Ada\"}"
        ));
        let root =
            std::env::temp_dir().join(format!("simile-proxy-server-test-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        let session =
            Session::create("acme", "learn_test", &root).unwrap_or_else(|error| panic!("{error}"));
        let server = ProxyServer::bind(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            ProxyOptions {
                upstream,
                relay_path: Some("/.simile/webhooks/acme".to_owned()),
                webhook_destination: None,
                learning: Some(session.clone()),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        let address = server.local_addr();
        let (shutdown, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(server.run_until(async {
            drop(signal.await);
        }));
        let body = r#"{"name":"Ada","api_key":"sk_test"}"#;
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        stream
            .write_all(
                format!(
                    "POST /v1/customers?expand=name HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let _ = shutdown.send(());
        task.await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|error| panic!("{error}"));

        let response = String::from_utf8_lossy(&response);
        let status = response
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1));
        assert_eq!(status, Some("201"), "unexpected proxy response: {response}");
        let request = upstream_task
            .join()
            .unwrap_or_else(|_| panic!("upstream task failed"));
        assert!(request.contains("POST /v1/customers?expand=name HTTP/1.1"));
        assert_eq!(session.exchange_count(), 1);
        let evidence = std::fs::read_to_string(root.join("evidence/observations.jsonl"))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(evidence.contains("<redacted>"));
        assert!(!evidence.contains("sk_test"));
        drop(std::fs::remove_dir_all(root));
    }

    #[tokio::test]
    async fn relays_and_learns_webhooks_on_the_reserved_path() {
        let (destination, destination_task) = upstream(concat!(
            "HTTP/1.1 202 Accepted\r\n",
            "Content-Length: 0\r\n",
            "Connection: close\r\n\r\n"
        ));
        let root =
            std::env::temp_dir().join(format!("simile-proxy-webhook-test-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        let session = Session::create("acme", "learn_webhook", &root)
            .unwrap_or_else(|error| panic!("{error}"));
        let server = ProxyServer::bind(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            ProxyOptions {
                upstream: "https://api.acme.test".to_owned(),
                relay_path: Some("/.simile/webhooks/acme".to_owned()),
                webhook_destination: Some(format!("{destination}/hooks/acme")),
                learning: Some(session.clone()),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        let address = server.local_addr();
        let (shutdown, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(server.run_until(async {
            drop(signal.await);
        }));
        let body = r#"{"type":"payment.succeeded","data":{"id":"pay_123"}}"#;
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        stream
            .write_all(
                format!(
                    "POST /.simile/webhooks/acme HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let _ = shutdown.send(());
        task.await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|error| panic!("{error}"));

        assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 202 Accepted"));
        let forwarded = destination_task
            .join()
            .unwrap_or_else(|_| panic!("destination task failed"));
        assert!(forwarded.contains("POST /hooks/acme HTTP/1.1"));
        let report = session
            .finish("https://api.acme.test", address.port())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(report.webhook_deliveries, 1);
        let contract =
            std::fs::read_to_string(report.openapi).unwrap_or_else(|error| panic!("{error}"));
        assert!(contract.contains("payment.succeeded"));
        drop(std::fs::remove_dir_all(root));
    }

    #[tokio::test]
    async fn returns_upstream_redirects_to_the_application() {
        let (upstream, upstream_task) = upstream(concat!(
            "HTTP/1.1 303 See Other\r\n",
            "Location: https://app.acme.test/complete\r\n",
            "Content-Length: 0\r\n",
            "Connection: close\r\n\r\n"
        ));
        let server = ProxyServer::bind(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            ProxyOptions {
                upstream,
                relay_path: None,
                webhook_destination: None,
                learning: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        let address = server.local_addr();
        let (shutdown, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(server.run_until(async {
            drop(signal.await);
        }));
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        stream
            .write_all(b"GET /checkout HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let _ = shutdown.send(());
        task.await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|error| panic!("{error}"));
        drop(upstream_task.join());

        let response = String::from_utf8_lossy(&response);
        assert!(response.starts_with("HTTP/1.1 303 See Other"));
        assert!(response.contains("location: https://app.acme.test/complete"));
    }
}
