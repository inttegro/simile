//! Adapt runtime executions and failures into HTTP responses.

use std::collections::BTreeSet;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::header::{ALLOW, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::stream;
use serde_json::json;
use simile_runtime::{OperationExecution, Runtime, TransportPlan};

pub(super) async fn operation_execution_response(
    execution: OperationExecution,
    head: bool,
) -> Response {
    if let Some(fault) = execution.fault() {
        tracing::info!(
            target: "simile::fault",
            operation = execution.operation(),
            fault = fault.name(),
            source = ?fault.source(),
            phase = ?fault.phase(),
            "activated environmental fault"
        );
    }
    match execution.transport() {
        TransportPlan::Respond => operation_response(execution.response(), head),
        TransportPlan::RespondAfter { milliseconds } => {
            tokio::time::sleep(Duration::from_millis(*milliseconds)).await;
            operation_response(execution.response(), head)
        }
        TransportPlan::DisconnectAfterCommit => disconnect_after_commit(execution.response()),
    }
}

fn disconnect_after_commit(response: &simile_runtime::SynthesizedResponse) -> Response {
    let status =
        StatusCode::from_u16(response.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let error = std::io::Error::new(
        std::io::ErrorKind::ConnectionReset,
        "simile injected a disconnect after commit",
    );
    let body = Body::from_stream(stream::once(
        async move { Err::<Bytes, std::io::Error>(error) },
    ));
    let mut outgoing = (status, body).into_response();
    if let Some(content_type) = response
        .media_type()
        .and_then(|value| HeaderValue::from_str(value).ok())
    {
        outgoing.headers_mut().insert(CONTENT_TYPE, content_type);
    }
    if let Err(error) = apply_response_headers(&mut outgoing, response) {
        return error;
    }
    outgoing
}

pub(super) fn header(
    headers: &HeaderMap,
    name: &str,
) -> std::result::Result<Option<String>, Response> {
    headers
        .get(name)
        .map(|value| {
            value.to_str().map(str::to_owned).map_err(|_| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_header",
                    "request contains a non-text header value",
                )
            })
        })
        .transpose()
}

fn operation_response(response: &simile_runtime::SynthesizedResponse, head: bool) -> Response {
    let status =
        StatusCode::from_u16(response.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = if head {
        Body::empty()
    } else {
        Body::from(response.bytes().to_vec())
    };
    let mut outgoing = (status, body).into_response();
    if let Some(content_type) = response.media_type() {
        let Ok(value) = HeaderValue::from_str(content_type) else {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_response_header",
                "compiled response contains an invalid content type",
            );
        };
        outgoing.headers_mut().insert(CONTENT_TYPE, value);
    }
    if let Err(error) = apply_response_headers(&mut outgoing, response) {
        return error;
    }
    outgoing
}

fn apply_response_headers(
    outgoing: &mut Response,
    response: &simile_runtime::SynthesizedResponse,
) -> std::result::Result<(), Response> {
    for (name, value) in response.headers() {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_response_header",
                "compiled response contains an invalid header name",
            )
        })?;
        let value = HeaderValue::from_str(value).map_err(|_| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_response_header",
                "compiled response contains an invalid header value",
            )
        })?;
        outgoing.headers_mut().append(name, value);
    }
    Ok(())
}

pub(super) fn operation_error(
    runtime: &Runtime,
    operation: &str,
    error: simile_runtime::Error,
    head: bool,
) -> Response {
    let Some(cause) = service_error_cause(&error) else {
        return runtime_error(error);
    };
    match runtime.synthesize_protocol_error(operation, cause) {
        Ok(Some(response)) => {
            tracing::debug!(
                target: "simile::http",
                operation,
                status = response.status(),
                error = %error,
                issues = ?error.issues(),
                "request rejected using the emulated service contract"
            );
            operation_response(&response, head)
        }
        Ok(None) => runtime_error(error),
        Err(synthesis_error) => {
            tracing::warn!(
                target: "simile::http",
                operation,
                error = %synthesis_error,
                "could not synthesize the emulated service's declared error response"
            );
            runtime_error(error)
        }
    }
}

fn service_error_cause(error: &simile_runtime::Error) -> Option<&'static str> {
    use simile_runtime::Error as RuntimeError;

    match error {
        RuntimeError::InvalidBody { .. } => Some("invalid_body"),
        RuntimeError::MalformedJson { .. }
        | RuntimeError::MalformedBody { .. }
        | RuntimeError::InvalidParameters { .. }
        | RuntimeError::BodyRequired { .. }
        | RuntimeError::BodyNotDeclared { .. }
        | RuntimeError::EmptyReplayIdentity => Some("malformed_request"),
        RuntimeError::MissingContentType { .. }
        | RuntimeError::UnsupportedMediaType { .. }
        | RuntimeError::UnsupportedEncoding { .. } => Some("unsupported_media_type"),
        RuntimeError::IdempotencyConflict { .. } => Some("idempotency_conflict"),
        RuntimeError::AuthenticationRequired { .. } => Some("authentication_required"),
        RuntimeError::PermissionDenied { .. } => Some("permission_denied"),
        _ => None,
    }
}

fn runtime_error(error: simile_runtime::Error) -> Response {
    use simile_runtime::Error as RuntimeError;

    if let RuntimeError::InvalidParameters { issues } = &error {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({
                "error": {
                    "code": "invalid_parameters",
                    "message": "request parameters do not satisfy the operation contract",
                    "issues": issues,
                }
            }),
        );
    }

    if let RuntimeError::InvalidBody { issues } = &error {
        let message = validation_error_message(issues.len());
        return json_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "error": {
                    "code": "invalid_body",
                    "message": message,
                    "issues": issues,
                }
            }),
        );
    }

    let (status, code) = match &error {
        RuntimeError::MalformedJson { .. } => (StatusCode::BAD_REQUEST, "malformed_json"),
        RuntimeError::MalformedBody { .. } => (StatusCode::BAD_REQUEST, "malformed_body"),
        RuntimeError::InvalidParameters { .. } => (StatusCode::BAD_REQUEST, "invalid_parameters"),
        RuntimeError::MissingContentType { .. }
        | RuntimeError::UnsupportedMediaType { .. }
        | RuntimeError::UnsupportedEncoding { .. } => {
            (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type")
        }
        RuntimeError::BodyRequired { .. } | RuntimeError::BodyNotDeclared { .. } => {
            (StatusCode::BAD_REQUEST, "invalid_body")
        }
        RuntimeError::IdempotencyConflict { .. } => (StatusCode::CONFLICT, "idempotency_conflict"),
        RuntimeError::AuthenticationRequired { .. } => {
            (StatusCode::UNAUTHORIZED, "authentication_required")
        }
        RuntimeError::PermissionDenied { .. } => (StatusCode::FORBIDDEN, "permission_denied"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "emulation_failed"),
    };
    error_response(status, code, &error.to_string())
}

pub(super) fn validation_error_message(issue_count: usize) -> String {
    if issue_count == 1 {
        "request body is invalid; fix the issue listed in `issues` and retry".to_owned()
    } else {
        format!(
            "request body is invalid; fix the {issue_count} issues listed in `issues` and retry"
        )
    }
}

pub(super) fn method_not_allowed(methods: BTreeSet<&str>) -> Response {
    let mut response = error_response(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "method is not declared for this route",
    );
    let allow = methods.into_iter().collect::<Vec<_>>().join(", ");
    if let Ok(value) = HeaderValue::from_str(&allow) {
        response.headers_mut().insert(ALLOW, value);
    }
    response
}

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
