//! Loopback-only runtime inspection, exchange capture, and report preparation.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, ALLOW,
    ORIGIN, VARY,
};
use axum::http::{HeaderName, HeaderValue, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde_json::json;
use simile_runtime::IncomingParameters;
use tokio::sync::oneshot;

use crate::access_log::text_header;
use crate::response::{error_response, json_response};
use crate::{
    ConfirmDestructiveActionBody, INSPECTION_BODY_LIMIT, InspectionExchange, InspectionMessage,
    Inspector, PrepareRuntimeReportBody, RecordedExchange, RuntimeExchangeReport,
    RuntimeReportCommand, RuntimeReportError, RuntimeReportSummary, ServerState,
    SubmitRuntimeReportBody,
};

impl Inspector {
    pub(super) fn memory() -> Self {
        Self {
            next_exchange: 0.into(),
            exchanges: crate::exchange_store::ExchangeStore::memory(),
            control: crate::control::ConsoleControl::new(),
        }
    }

    pub(super) fn durable(
        path: impl AsRef<std::path::Path>,
    ) -> crate::exchange_store::Result<Self> {
        let (exchanges, latest_sequence) = crate::exchange_store::ExchangeStore::durable(path)?;
        Ok(Self {
            next_exchange: latest_sequence.into(),
            exchanges,
            control: crate::control::ConsoleControl::new(),
        })
    }

    pub(super) fn next_id(&self, started_at_ms: u64) -> (String, u64) {
        let sequence = self.next_exchange.fetch_add(1, Ordering::Relaxed) + 1;
        (format!("ex_{started_at_ms}_{sequence}"), sequence)
    }

    pub(super) fn record(&self, sequence: u64, exchange: RecordedExchange) {
        if let Err(error) = self.exchanges.record(sequence, exchange) {
            tracing::warn!(target: "simile::inspection", %error, "could not persist exchange");
        }
    }

    pub(super) fn snapshot(&self) -> crate::exchange_store::Result<Vec<InspectionExchange>> {
        self.exchanges.snapshot().map(|exchanges| {
            exchanges
                .into_iter()
                .map(|recorded| recorded.exchange)
                .collect()
        })
    }

    pub(super) fn recorded(
        &self,
        exchange_id: &str,
    ) -> crate::exchange_store::Result<Option<RecordedExchange>> {
        self.exchanges.recorded(exchange_id)
    }

    pub(super) fn recorded_for_control(
        &self,
        control_request_id: &str,
    ) -> crate::exchange_store::Result<Option<RecordedExchange>> {
        self.exchanges.snapshot().map(|exchanges| {
            exchanges.into_iter().find(|recorded| {
                recorded.exchange.control_request_id.as_deref() == Some(control_request_id)
            })
        })
    }

    fn clear(&self) -> crate::exchange_store::Result<usize> {
        self.exchanges.clear()
    }

    fn report_capture(
        &self,
        exchange_id: &str,
    ) -> crate::exchange_store::Result<Option<RuntimeExchangeReport>> {
        self.exchanges.report_capture(exchange_id)
    }

    fn report(
        &self,
        exchange_id: &str,
    ) -> crate::exchange_store::Result<Option<RuntimeReportSummary>> {
        self.exchanges.report(exchange_id)
    }

    fn set_report(&self, exchange_id: &str, report: RuntimeReportSummary) {
        if let Err(error) = self.exchanges.set_report(exchange_id, report) {
            tracing::warn!(target: "simile::inspection", %error, "could not persist prepared report");
        }
    }

    fn update_report(&self, report: RuntimeReportSummary) {
        if let Err(error) = self.exchanges.update_report(report) {
            tracing::warn!(target: "simile::inspection", %error, "could not persist submitted report");
        }
    }
}

pub(super) async fn inspection_response(
    state: &Arc<ServerState>,
    request: Request<Body>,
) -> Response {
    let Some(inspector) = state.inspector.as_ref() else {
        return error_response(StatusCode::NOT_FOUND, "not_found", "route not found");
    };
    let origin = text_header(request.headers(), ORIGIN.as_str());
    if origin
        .as_deref()
        .is_some_and(|origin| !is_loopback_origin(origin))
    {
        return error_response(
            StatusCode::FORBIDDEN,
            "inspection_origin_forbidden",
            "runtime inspection accepts browser requests only from a loopback origin",
        );
    }
    if request.method() == Method::OPTIONS {
        return with_inspection_cors(StatusCode::NO_CONTENT.into_response(), origin.as_deref());
    }
    if request
        .uri()
        .path()
        .starts_with(crate::control::CONTROL_PREFIX)
    {
        let response = crate::control::response(state, inspector, request)
            .await
            .unwrap_or_else(|| {
                error_response(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "console route not found",
                )
            });
        return with_inspection_cors(response, origin.as_deref());
    }
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let response = match path.as_str() {
        "/.simile/inspection" if method == Method::GET => {
            match state.service.inspect_state(false) {
                Ok(inspection) => {
                    let summary = state.service.artifact().summary();
                    let exchange_count = match inspector.snapshot() {
                        Ok(exchanges) => exchanges.len(),
                        Err(error) => return inspection_history_error(error),
                    };
                    json_response(
                        StatusCode::OK,
                        json!({
                            "status": "ready",
                            "service": summary.service,
                            "serviceVersion": summary.service_version,
                            "artifactDigest": summary.payload_sha256,
                            "operations": summary.operations,
                            "resources": inspection.resources.len(),
                            "events": inspection.events.len(),
                            "exchanges": exchange_count,
                        }),
                    )
                }
                Err(error) => inspection_state_error(error),
            }
        }
        "/.simile/exchanges" => match method {
            Method::GET => match inspector.snapshot() {
                Ok(exchanges) => json_response(StatusCode::OK, json!({ "exchanges": exchanges })),
                Err(error) => inspection_history_error(error),
            },
            Method::DELETE => match inspector.clear() {
                Ok(removed) => json_response(StatusCode::OK, json!({ "removed": removed })),
                Err(error) => inspection_history_error(error),
            },
            _ => inspection_method_not_allowed("GET, DELETE, OPTIONS"),
        },
        "/.simile/state" if method == Method::GET => match state.service.inspect_state(true) {
            Ok(inspection) => json_response(StatusCode::OK, json!(inspection)),
            Err(error) => inspection_state_error(error),
        },
        "/.simile/state/reset" if method == Method::POST => {
            reset_state_response(state, request).await
        }
        "/.simile/deferred" if method == Method::GET => {
            match state.service.inspect_deferred_work() {
                Ok(inspection) => json_response(StatusCode::OK, json!(inspection)),
                Err(error) => inspection_state_error(error),
            }
        }
        _ if exchange_report_id(&path).is_some() => {
            let exchange_id = exchange_report_id(&path).unwrap_or_default();
            match method {
                Method::GET => match inspector.report(exchange_id) {
                    Ok(Some(report)) => json_response(StatusCode::OK, json!(report)),
                    Ok(None) => error_response(
                        StatusCode::NOT_FOUND,
                        "report_not_prepared",
                        "this exchange has no prepared report",
                    ),
                    Err(error) => inspection_history_error(error),
                },
                Method::POST => {
                    prepare_exchange_report_response(state, inspector, exchange_id, request).await
                }
                _ => inspection_method_not_allowed("GET, POST, OPTIONS"),
            }
        }
        _ if report_submit_id(&path).is_some() => {
            let report_id = report_submit_id(&path).unwrap_or_default();
            if method == Method::POST {
                submit_exchange_report_response(state, inspector, report_id, request).await
            } else {
                inspection_method_not_allowed("POST, OPTIONS")
            }
        }
        _ if method != Method::GET => inspection_method_not_allowed("GET, OPTIONS"),
        _ => error_response(StatusCode::NOT_FOUND, "not_found", "route not found"),
    };
    with_inspection_cors(response, origin.as_deref())
}

async fn reset_state_response(state: &ServerState, request: Request<Body>) -> Response {
    let body = match decode_inspection_json::<ConfirmDestructiveActionBody>(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    if !body.confirm {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "reset_confirmation_required",
            "resetting service state requires explicit confirmation",
        );
    }
    match state.service.reset_state() {
        Ok(()) => json_response(StatusCode::OK, json!({ "reset": true })),
        Err(error) => inspection_state_error(error),
    }
}

async fn prepare_exchange_report_response(
    state: &ServerState,
    inspector: &Inspector,
    exchange_id: &str,
    request: Request<Body>,
) -> Response {
    match inspector.report(exchange_id) {
        Ok(Some(report)) => return json_response(StatusCode::OK, json!(report)),
        Ok(None) => {}
        Err(error) => return inspection_history_error(error),
    }
    let exchange = match inspector.report_capture(exchange_id) {
        Ok(Some(exchange)) => exchange,
        Ok(None) => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "report_evidence_unavailable",
                "this exchange has no safely reproducible request evidence",
            );
        }
        Err(error) => return inspection_history_error(error),
    };
    let body = match decode_inspection_json::<PrepareRuntimeReportBody>(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let title = body
        .title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| format!("Unexpected behavior in {}", exchange.operation));
    let Some(sender) = state.reporting_sender.as_ref() else {
        return reporting_unavailable();
    };
    let (reply, receiver) = oneshot::channel();
    if sender
        .send(RuntimeReportCommand::Prepare {
            exchange,
            title,
            description: body.description,
            reply,
        })
        .await
        .is_err()
    {
        return reporting_unavailable();
    }
    match tokio::time::timeout(Duration::from_secs(30), receiver).await {
        Ok(Ok(Ok(report))) => {
            inspector.set_report(exchange_id, report.clone());
            json_response(StatusCode::CREATED, json!(report))
        }
        Ok(Ok(Err(error))) => runtime_report_error_response(error),
        Ok(Err(_)) | Err(_) => reporting_unavailable(),
    }
}

fn inspection_history_error(error: crate::exchange_store::Error) -> Response {
    tracing::error!(target: "simile::inspection", %error, "exchange history is unavailable");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "inspection_unavailable",
        "runtime exchange history is unavailable",
    )
}

async fn submit_exchange_report_response(
    state: &ServerState,
    inspector: &Inspector,
    report_id: &str,
    request: Request<Body>,
) -> Response {
    let body = match decode_inspection_json::<SubmitRuntimeReportBody>(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let contact = body.contact.filter(|contact| !contact.trim().is_empty());
    let Some(sender) = state.reporting_sender.as_ref() else {
        return reporting_unavailable();
    };
    let (reply, receiver) = oneshot::channel();
    if sender
        .send(RuntimeReportCommand::Submit {
            report: report_id.to_owned(),
            contact,
            current_version: body.current_version,
            reply,
        })
        .await
        .is_err()
    {
        return reporting_unavailable();
    }
    match tokio::time::timeout(Duration::from_secs(30), receiver).await {
        Ok(Ok(Ok(report))) => {
            inspector.update_report(report.clone());
            json_response(StatusCode::OK, json!(report))
        }
        Ok(Ok(Err(error))) => runtime_report_error_response(error),
        Ok(Err(_)) | Err(_) => reporting_unavailable(),
    }
}

pub(super) async fn decode_inspection_json<T>(
    request: Request<Body>,
) -> std::result::Result<T, Response>
where
    T: DeserializeOwned + Default,
{
    let bytes = to_bytes(request.into_body(), 64 * 1024)
        .await
        .map_err(|_| {
            error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "inspection_request_too_large",
                "inspection request exceeds the configured limit",
            )
        })?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        error_response(
            StatusCode::BAD_REQUEST,
            "invalid_inspection_request",
            "inspection request body must be valid JSON",
        )
    })
}

fn exchange_report_id(path: &str) -> Option<&str> {
    path.strip_prefix("/.simile/exchanges/")
        .and_then(|path| path.strip_suffix("/report"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

fn report_submit_id(path: &str) -> Option<&str> {
    path.strip_prefix("/.simile/reports/")
        .and_then(|path| path.strip_suffix("/submit"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

fn inspection_method_not_allowed(allow: &'static str) -> Response {
    let mut response = error_response(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "runtime inspection does not support this method",
    );
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(allow));
    response
}

fn reporting_unavailable() -> Response {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "reporting_unavailable",
        "runtime reporting is unavailable",
    )
}

fn runtime_report_error_response(error: RuntimeReportError) -> Response {
    let status = match error.code.as_str() {
        "report_contact_required" | "reporting_not_declared" => StatusCode::UNPROCESSABLE_ENTITY,
        "report_not_found" => StatusCode::NOT_FOUND,
        "report_verification_failed" => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error_response(status, &error.code, &error.message)
}

fn inspection_state_error(error: simile_service::Error) -> Response {
    tracing::error!(target: "simile::inspection", %error, "could not inspect live service state");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "inspection_unavailable",
        "runtime state inspection is unavailable",
    )
}

fn with_inspection_cors(mut response: Response, origin: Option<&str>) -> Response {
    if let Some(origin) = origin.and_then(|origin| HeaderValue::from_str(origin).ok()) {
        response
            .headers_mut()
            .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        response
            .headers_mut()
            .insert(VARY, HeaderValue::from_static("Origin"));
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, DELETE, OPTIONS"),
        );
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Authorization, Content-Type"),
        );
        response.headers_mut().insert(
            HeaderName::from_static("access-control-allow-private-network"),
            HeaderValue::from_static("true"),
        );
    }
    response
}

fn is_loopback_origin(origin: &str) -> bool {
    let Some(authority) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .and_then(|rest| rest.split('/').next())
    else {
        return false;
    };
    ["localhost", "127.0.0.1", "[::1]"]
        .iter()
        .any(|host| authority == *host || authority.strip_prefix(host).is_some_and(valid_port))
}

fn valid_port(suffix: &str) -> bool {
    suffix
        .strip_prefix(':')
        .is_some_and(|port| !port.is_empty() && port.parse::<u16>().is_ok())
}

pub(super) fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().try_into().unwrap_or(u64::MAX)
        })
}

pub(super) fn inspection_request_body(
    content_type: Option<&str>,
    bytes: &[u8],
) -> InspectionMessage {
    let truncated = bytes.len() > INSPECTION_BODY_LIMIT;
    let body = (!truncated)
        .then(|| sanitize_body(content_type, bytes))
        .flatten()
        .and_then(|body| inspection_json_or_text(content_type, &body));
    InspectionMessage {
        content_type: content_type.map(str::to_owned),
        body,
        truncated,
    }
}

pub(super) fn inspection_response_body(
    content_type: Option<&str>,
    bytes: &[u8],
) -> InspectionMessage {
    let truncated = bytes.len() > INSPECTION_BODY_LIMIT;
    let body = (!truncated)
        .then(|| inspection_json_or_text(content_type, bytes))
        .flatten();
    InspectionMessage {
        content_type: content_type.map(str::to_owned),
        body,
        truncated,
    }
}

fn inspection_json_or_text(content_type: Option<&str>, bytes: &[u8]) -> Option<serde_json::Value> {
    if bytes.is_empty() {
        return None;
    }
    let media_type = content_type
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if media_type.is_some_and(|value| value == "application/json" || value.ends_with("+json")) {
        return serde_json::from_slice(bytes).ok();
    }
    std::str::from_utf8(bytes)
        .ok()
        .map(|value| serde_json::Value::String(value.to_owned()))
}

pub(super) fn sanitize_parameters(mut parameters: IncomingParameters) -> IncomingParameters {
    redact_parameter_map(&mut parameters.path);
    redact_parameter_map(&mut parameters.query);
    redact_parameter_map(&mut parameters.headers);
    redact_parameter_map(&mut parameters.cookies);
    parameters
}

fn redact_parameter_map(values: &mut BTreeMap<String, Vec<String>>) {
    for (name, values) in values {
        if sensitive_name(name) {
            values.clear();
            values.push("<redacted>".to_owned());
        }
    }
}

pub(super) fn sanitize_body(content_type: Option<&str>, body: &[u8]) -> Option<Vec<u8>> {
    if body.is_empty() {
        return Some(Vec::new());
    }
    let content_type = content_type?;
    let media_type = content_type.split(';').next()?.trim();
    if media_type == "multipart/form-data" {
        return simile_runtime::sanitize_multipart_for_replay(content_type, body, sensitive_name)
            .ok();
    }
    if media_type != "application/json" && !media_type.ends_with("+json") {
        return None;
    }
    let mut value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    redact_json(&mut value);
    serde_json::to_vec(&value).ok()
}

pub(super) fn redact_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (name, value) in object {
                if sensitive_name(name) {
                    *value = serde_json::Value::String("<redacted>".to_owned());
                } else {
                    redact_json(value);
                }
            }
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(redact_json),
        _ => {}
    }
}

fn sensitive_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace(['-', '.'], "_");
    [
        "authorization",
        "cookie",
        "password",
        "secret",
        "token",
        "api_key",
        "apikey",
        "access_key",
        "private_key",
        "card_number",
        "account_number",
        "cvc",
        "cvv",
    ]
    .iter()
    .any(|sensitive| normalized == *sensitive || normalized.ends_with(&format!("_{sensitive}")))
}
