use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderName, HeaderValue, Method, Request, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};

use super::http::{decode_response_body, request_path};
use crate::inspection::{decode_inspection_json, unix_time_millis};
use crate::response::{error_response, json_response};
use crate::{DEFAULT_BODY_LIMIT, Inspector, ServerState};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteRequest {
    method: String,
    path: String,
    #[serde(default)]
    query: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    headers: BTreeMap<String, Vec<String>>,
    body: Option<Value>,
    body_text: Option<String>,
    content_type: Option<String>,
    scenario: Option<String>,
    fault_profile: Option<String>,
    profile: Option<String>,
    response_seed: Option<u64>,
}

pub(super) async fn response(
    state: &Arc<ServerState>,
    inspector: &Inspector,
    request: Request<Body>,
) -> Response {
    let request = match decode_inspection_json::<ExecuteRequest>(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let method = match Method::from_bytes(request.method.as_bytes()) {
        Ok(method) => method,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_console_method",
                "request method is invalid",
            );
        }
    };
    if !request.path.starts_with('/') || request.path.starts_with("/.simile/") {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_console_path",
            "request path must be a public service path",
        );
    }
    let query = {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for (name, values) in &request.query {
            for value in values {
                serializer.append_pair(name, value);
            }
        }
        serializer.finish()
    };
    let uri = if query.is_empty() {
        request.path.clone()
    } else {
        format!("{}?{query}", request.path)
    };
    let request_path_label = request.path.clone();
    let control_id = format!("ctl_{}_{}", unix_time_millis(), rand::random::<u32>());
    let body = match (request.body_text, request.body) {
        (Some(body), _) => body.into_bytes(),
        (None, Some(body)) => match serde_json::to_vec(&body) {
            Ok(body) => body,
            Err(error) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_console_body",
                    &error.to_string(),
                );
            }
        },
        (None, None) => Vec::new(),
    };
    let mut builder = Request::builder().method(method.clone()).uri(uri);
    for (name, values) in &request.headers {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_console_header",
                "request contains an invalid header name",
            );
        };
        for value in values {
            let Ok(value) = HeaderValue::from_str(value) else {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_console_header",
                    "request contains an invalid header value",
                );
            };
            if let Some(headers) = builder.headers_mut() {
                headers.append(name.clone(), value);
            }
        }
    }
    if let Some(headers) = builder.headers_mut() {
        if !body.is_empty() {
            let content_type = request
                .content_type
                .as_deref()
                .unwrap_or("application/json");
            let Ok(content_type) = HeaderValue::from_str(content_type) else {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_console_content_type",
                    "request content type is invalid",
                );
            };
            headers.insert(CONTENT_TYPE, content_type);
        }
    }
    let options = simile_http::HttpOptions {
        body_limit: state.body_limit,
        scenario: request.scenario,
        fault_profile: request.fault_profile,
        profile: request.profile,
        response_seed: request.response_seed,
    };
    let request = match builder.body(Body::from(body)) {
        Ok(request) => request,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_console_request",
                &error.to_string(),
            );
        }
    };
    let response = crate::handle_public(
        Arc::clone(state),
        request,
        options,
        Some(control_id.clone()),
    )
    .await;
    let (parts, body) = response.into_parts();
    let bytes = match to_bytes(body, DEFAULT_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "console_response_unavailable",
                "the executed response could not be collected",
            );
        }
    };
    let content_type = parts
        .headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = decode_response_body(content_type.as_deref(), &bytes);
    let exchange = inspector.recorded_for_control(&control_id).ok().flatten();
    inspector.control.record(
        "execute",
        format!(
            "{} {} returned {}",
            method,
            request_path(&request_path_label),
            parts.status.as_u16()
        ),
        json!({ "exchangeId": exchange.as_ref().map(|exchange| &exchange.exchange.id) }),
    );
    json_response(
        StatusCode::OK,
        json!({
            "response": {
                "status": parts.status.as_u16(),
                "contentType": content_type,
                "body": body,
            },
            "exchange": exchange.map(|recorded| recorded.exchange),
        }),
    )
}
