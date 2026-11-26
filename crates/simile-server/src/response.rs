//! Responses for the native inspection endpoints.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

pub(super) fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    json_response(
        status,
        json!({
            "error": {
                "code": code,
                "message": message,
            }
        }),
    )
}

pub(super) fn json_response(status: StatusCode, payload: serde_json::Value) -> Response {
    (status, axum::Json(payload)).into_response()
}
