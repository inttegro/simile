use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;

use crate::response::error_response;

pub(super) fn unavailable() -> Response {
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "console_unavailable",
        "console control state is unavailable",
    )
}

pub(super) fn request_path(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

pub(super) fn decode_response_body(content_type: Option<&str>, body: &[u8]) -> Option<Value> {
    if body.is_empty() {
        return None;
    }
    let media_type = content_type
        .and_then(|content_type| content_type.split(';').next())
        .map(str::trim);
    if media_type
        .is_some_and(|media_type| media_type == "application/json" || media_type.ends_with("+json"))
    {
        return serde_json::from_slice(body).ok();
    }
    std::str::from_utf8(body)
        .ok()
        .map(|body| Value::String(body.to_owned()))
}
