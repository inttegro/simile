use std::sync::atomic::Ordering;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::json;
use simile_service::StackSnapshot;

use super::http::unavailable as control_unavailable;
use crate::inspection::{decode_inspection_json, unix_time_millis};
use crate::response::{error_response, json_response};
use crate::{Inspector, ServerState};

pub(super) struct StoredSession {
    summary: SessionSummary,
    snapshot: StackSnapshot,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSummary {
    name: String,
    created_at_ms: u64,
    logical_time_milliseconds: i64,
    artifact_digest: String,
    exchange_count: usize,
}

#[derive(Debug, Default, Deserialize)]
struct CreateSessionRequest {
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RestoreSessionRequest {
    #[serde(default)]
    confirm: bool,
}

#[derive(Debug, Default, Deserialize)]
struct AdvanceClockRequest {
    milliseconds: u64,
}

pub(super) fn list(inspector: &Inspector) -> Response {
    let Ok(sessions) = inspector.control.sessions.lock() else {
        return control_unavailable();
    };
    let sessions = sessions
        .values()
        .map(|session| session.summary.clone())
        .collect::<Vec<_>>();
    json_response(StatusCode::OK, json!({ "sessions": sessions }))
}

pub(super) async fn create(
    state: &ServerState,
    inspector: &Inspector,
    request: Request<Body>,
) -> Response {
    let body = match decode_inspection_json::<CreateSessionRequest>(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let generated = format!(
        "checkpoint-{}",
        inspector.control.next_event.load(Ordering::Relaxed) + 1
    );
    let name = body
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(generated);
    if !valid_session_name(&name) {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_session_name",
            "session names use lowercase letters, digits, dots, underscores, or hyphens",
        );
    }
    let snapshot = match state.service.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => return state_error(error),
    };
    let exchange_count = match inspector.snapshot() {
        Ok(exchanges) => exchanges.len(),
        Err(_) => return control_unavailable(),
    };
    let summary = SessionSummary {
        name: name.clone(),
        created_at_ms: unix_time_millis(),
        logical_time_milliseconds: snapshot.now().as_unix_millis(),
        artifact_digest: state.service.artifact().summary().payload_sha256,
        exchange_count,
    };
    let Ok(mut sessions) = inspector.control.sessions.lock() else {
        return control_unavailable();
    };
    if sessions.contains_key(&name) {
        return error_response(
            StatusCode::CONFLICT,
            "session_exists",
            "a checkpoint with this name already exists",
        );
    }
    sessions.insert(
        name.clone(),
        StoredSession {
            summary: summary.clone(),
            snapshot,
        },
    );
    drop(sessions);
    inspector.control.record(
        "checkpoint",
        format!("captured {name}"),
        json!({ "session": name }),
    );
    json_response(StatusCode::CREATED, json!(summary))
}

pub(super) async fn restore(
    state: &ServerState,
    inspector: &Inspector,
    name: &str,
    request: Request<Body>,
) -> Response {
    let body = match decode_inspection_json::<RestoreSessionRequest>(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    if !body.confirm {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "restore_confirmation_required",
            "restoring a checkpoint requires explicit confirmation",
        );
    }
    let snapshot = {
        let Ok(sessions) = inspector.control.sessions.lock() else {
            return control_unavailable();
        };
        let Some(session) = sessions.get(name) else {
            return error_response(
                StatusCode::NOT_FOUND,
                "session_not_found",
                "the requested checkpoint does not exist",
            );
        };
        session.snapshot.clone()
    };
    if let Err(error) = state.service.restore_snapshot(snapshot) {
        return state_error(error);
    }
    inspector.control.record(
        "restore",
        format!("restored {name}"),
        json!({ "session": name }),
    );
    json_response(StatusCode::OK, json!({ "restored": name }))
}

pub(super) fn delete(inspector: &Inspector, name: &str) -> Response {
    let Ok(mut sessions) = inspector.control.sessions.lock() else {
        return control_unavailable();
    };
    if sessions.remove(name).is_none() {
        return error_response(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "the requested checkpoint does not exist",
        );
    }
    drop(sessions);
    inspector.control.record(
        "delete_checkpoint",
        format!("deleted {name}"),
        json!({ "session": name }),
    );
    json_response(StatusCode::OK, json!({ "deleted": name }))
}

pub(super) async fn advance_clock(
    state: &ServerState,
    inspector: &Inspector,
    request: Request<Body>,
) -> Response {
    let body = match decode_inspection_json::<AdvanceClockRequest>(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    if body.milliseconds == 0 {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "clock_advance_required",
            "clock advance must be greater than zero milliseconds",
        );
    }
    match state.service.advance_clock(body.milliseconds) {
        Ok(report) => {
            inspector.control.record(
                "clock",
                format!("advanced clock by {}ms", body.milliseconds),
                json!({ "milliseconds": body.milliseconds, "report": report }),
            );
            json_response(StatusCode::OK, json!(report))
        }
        Err(error) => error_response(
            StatusCode::CONFLICT,
            "clock_not_controllable",
            &error.to_string(),
        ),
    }
}

fn state_error(error: simile_service::Error) -> Response {
    tracing::error!(target: "simile::console", %error, "console state action failed");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "console_state_failed",
        &error.to_string(),
    )
}

fn valid_session_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        })
}
