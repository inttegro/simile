//! Structured HTTP access logging without request or response payloads.

use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use axum::body::{Body, HttpBody};
use axum::extract::ConnectInfo;
use axum::http::header::{CONTENT_TYPE, USER_AGENT};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

pub(super) struct AccessLine<'a> {
    pub(super) method: &'a Method,
    pub(super) path: &'a str,
    pub(super) status: StatusCode,
    pub(super) origin_ip: Option<IpAddr>,
    pub(super) user_agent: Option<&'a str>,
    pub(super) request_bytes: Option<u64>,
    pub(super) request_content_type: Option<&'a str>,
    pub(super) response_bytes: Option<u64>,
    pub(super) response_content_type: Option<&'a str>,
    pub(super) elapsed: Duration,
}

impl fmt::Display for AccessLine<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {} {} origin=",
            self.method,
            self.path,
            self.status.as_u16()
        )?;
        write_ip(formatter, self.origin_ip)?;
        formatter.write_str(" user-agent=")?;
        write_label(formatter, self.user_agent)?;
        formatter.write_str(" request=")?;
        write_size(formatter, self.request_bytes)?;
        formatter.write_str(" request-type=")?;
        write_label(formatter, self.request_content_type)?;
        formatter.write_str(" response=")?;
        write_size(formatter, self.response_bytes)?;
        formatter.write_str(" response-type=")?;
        write_label(formatter, self.response_content_type)?;
        write!(formatter, " elapsed={}us", self.elapsed.as_micros())
    }
}

fn write_ip(formatter: &mut fmt::Formatter<'_>, address: Option<IpAddr>) -> fmt::Result {
    match address {
        Some(address) => write!(formatter, "{address}"),
        None => formatter.write_str("-"),
    }
}

fn write_size(formatter: &mut fmt::Formatter<'_>, bytes: Option<u64>) -> fmt::Result {
    match bytes {
        Some(bytes) => write!(formatter, "{bytes}B"),
        None => formatter.write_str("?"),
    }
}

fn write_label(formatter: &mut fmt::Formatter<'_>, value: Option<&str>) -> fmt::Result {
    match value {
        Some(value) => write!(formatter, "{value:?}"),
        None => formatter.write_str("-"),
    }
}

pub(super) async fn access_log(request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let origin_ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connection| connection.0.ip());
    let user_agent = text_header(request.headers(), USER_AGENT.as_str());
    let request_bytes = request.body().size_hint().exact();
    let request_content_type = text_header(request.headers(), CONTENT_TYPE.as_str());
    let started = Instant::now();
    let response = next.run(request).await;
    let response_content_type = text_header(response.headers(), CONTENT_TYPE.as_str());
    let line = AccessLine {
        method: &method,
        path: &path,
        status: response.status(),
        origin_ip,
        user_agent: user_agent.as_deref(),
        request_bytes,
        request_content_type: request_content_type.as_deref(),
        response_bytes: response.body().size_hint().exact(),
        response_content_type: response_content_type.as_deref(),
        elapsed: started.elapsed(),
    };
    let origin = line.origin_ip.map(|value| value.to_string());
    tracing::info!(
        target: "simile::http",
        method = %line.method,
        path = line.path,
        status = line.status.as_u16(),
        origin = origin.as_deref().unwrap_or("-"),
        user_agent = line.user_agent.unwrap_or("-"),
        request_bytes = line.request_bytes,
        request_type = line.request_content_type.unwrap_or("-"),
        response_bytes = line.response_bytes,
        response_type = line.response_content_type.unwrap_or("-"),
        elapsed_us = line.elapsed.as_micros(),
        "request completed"
    );
    response
}

pub(super) fn text_header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
