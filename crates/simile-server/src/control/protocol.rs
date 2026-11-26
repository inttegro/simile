use std::sync::Arc;

use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderValue, Method, Request, StatusCode};
use axum::response::Response;
use serde_json::json;

use super::{CONTROL_PREFIX, execute, replay, session};
use crate::response::{error_response, json_response};
use crate::{Inspector, ServerState};

pub(crate) async fn response(
    state: &Arc<ServerState>,
    inspector: &Inspector,
    request: Request<Body>,
) -> Option<Response> {
    let path = request.uri().path().to_owned();
    if !path.starts_with(CONTROL_PREFIX) {
        return None;
    }
    let method = request.method().clone();
    let authorized = authorized(state, &request);
    let response = match path.as_str() {
        CONTROL_PREFIX if method == Method::GET => capabilities(state, inspector, authorized),
        "/.simile/console/v1/events" if method == Method::GET => events(inspector, &request),
        "/.simile/console/v1/execute" if method == Method::POST => {
            if authorized {
                execute::response(state, inspector, request).await
            } else {
                unauthorized()
            }
        }
        "/.simile/console/v1/sessions" if method == Method::GET => session::list(inspector),
        "/.simile/console/v1/sessions" if method == Method::POST => {
            if authorized {
                session::create(state, inspector, request).await
            } else {
                unauthorized()
            }
        }
        "/.simile/console/v1/clock/advance" if method == Method::POST => {
            if authorized {
                session::advance_clock(state, inspector, request).await
            } else {
                unauthorized()
            }
        }
        _ if replay_exchange_id(&path).is_some() && method == Method::POST => {
            if authorized {
                replay::response(
                    state,
                    inspector,
                    replay_exchange_id(&path).unwrap_or_default(),
                )
            } else {
                unauthorized()
            }
        }
        _ if session_action(&path).is_some() => {
            if !authorized {
                unauthorized()
            } else {
                let (name, action) = session_action(&path).unwrap_or_default();
                match (method, action) {
                    (Method::POST, "restore") => {
                        session::restore(state, inspector, name, request).await
                    }
                    (Method::DELETE, "") => session::delete(inspector, name),
                    _ => method_not_allowed("POST, DELETE, OPTIONS"),
                }
            }
        }
        _ => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "console route not found",
        ),
    };
    Some(response)
}

fn capabilities(state: &ServerState, inspector: &Inspector, authorized: bool) -> Response {
    let sessions = inspector
        .control
        .sessions
        .lock()
        .map_or(0, |sessions| sessions.len());
    json_response(
        StatusCode::OK,
        json!({
            "protocol": "simile.console",
            "version": 1,
            "writable": authorized,
            "authorization": "bearer",
            "features": {
                "execute": true,
                "replay": true,
                "executionProof": true,
                "sessions": true,
                "clockControl": true,
                "eventFeed": true,
            },
            "sessions": sessions,
            "artifactDigest": state.service.artifact().summary().payload_sha256,
        }),
    )
}

fn events(inspector: &Inspector, request: &Request<Body>) -> Response {
    let after = request
        .uri()
        .query()
        .and_then(|query| {
            form_urlencoded::parse(query.as_bytes())
                .find(|(name, _)| name == "after")
                .and_then(|(_, value)| value.parse::<u64>().ok())
        })
        .unwrap_or_default();
    let Ok(events) = inspector.control.events.lock() else {
        return super::http::unavailable();
    };
    let events = events
        .iter()
        .filter(|event| event.sequence > after)
        .cloned()
        .collect::<Vec<_>>();
    let cursor = events.last().map_or(after, |event| event.sequence);
    json_response(
        StatusCode::OK,
        json!({ "events": events, "cursor": cursor }),
    )
}

fn authorized(state: &ServerState, request: &Request<Body>) -> bool {
    let Some(expected) = state.console_token.as_deref() else {
        return false;
    };
    request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| value == expected)
}

fn unauthorized() -> Response {
    error_response(
        StatusCode::UNAUTHORIZED,
        "console_authorization_required",
        "this console is attached read-only; start it with `simile console` to enable controls",
    )
}

fn method_not_allowed(allow: &'static str) -> Response {
    let mut response = error_response(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "console route does not support this method",
    );
    response
        .headers_mut()
        .insert(axum::http::header::ALLOW, HeaderValue::from_static(allow));
    response
}

fn replay_exchange_id(path: &str) -> Option<&str> {
    path.strip_prefix("/.simile/console/v1/exchanges/")
        .and_then(|path| path.strip_suffix("/replay"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

fn session_action(path: &str) -> Option<(&str, &str)> {
    let path = path.strip_prefix("/.simile/console/v1/sessions/")?;
    let (name, action) = path.split_once('/').unwrap_or((path, ""));
    (!name.is_empty() && !name.contains('/')).then_some((name, action))
}
