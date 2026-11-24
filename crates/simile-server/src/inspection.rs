//! Loopback-only runtime inspection, exchange capture, and report preparation.

use std::collections::BTreeMap;
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
    ConfirmDestructiveActionBody, INSPECTION_BODY_LIMIT, INSPECTION_EXCHANGE_LIMIT,
    InspectionExchange, InspectionMessage, Inspector, PrepareRuntimeReportBody, RecordedExchange,
    RuntimeExchangeReport, RuntimeIncident, RuntimeReportCommand, RuntimeReportError,
    RuntimeReportSummary, ServerState, SubmitRuntimeReportBody,
};

impl Inspector {
    pub(super) fn next_id(&self, started_at_ms: u64) -> String {
        let sequence = self.next_exchange.fetch_add(1, Ordering::Relaxed) + 1;
        format!("ex_{started_at_ms}_{sequence}")
    }

    pub(super) fn record(&self, exchange: RecordedExchange) {
        let Ok(mut exchanges) = self.exchanges.lock() else {
            tracing::warn!(target: "simile::inspection", "exchange recorder is unavailable");
            return;
        };
        exchanges.push_front(exchange);
        exchanges.truncate(INSPECTION_EXCHANGE_LIMIT);
    }

    fn snapshot(&self) -> Option<Vec<InspectionExchange>> {
        self.exchanges.lock().ok().map(|exchanges| {
            exchanges
                .iter()
                .map(|recorded| recorded.exchange.clone())
                .collect()
        })
    }

    fn clear(&self) -> Option<usize> {
        self.exchanges.lock().ok().map(|mut exchanges| {
            let removed = exchanges.len();
            exchanges.clear();
            removed
        })
    }

    fn report_capture(&self, exchange_id: &str) -> Option<RuntimeExchangeReport> {
        self.exchanges.lock().ok().and_then(|exchanges| {
            exchanges
                .iter()
                .find(|recorded| recorded.exchange.id == exchange_id)
                .and_then(|recorded| recorded.report_capture.clone())
        })
    }

    fn report(&self, exchange_id: &str) -> Option<RuntimeReportSummary> {
        self.exchanges.lock().ok().and_then(|exchanges| {
            exchanges
                .iter()
                .find(|recorded| recorded.exchange.id == exchange_id)
                .and_then(|recorded| recorded.report.clone())
        })
    }

    fn set_report(&self, exchange_id: &str, report: RuntimeReportSummary) {
        let Ok(mut exchanges) = self.exchanges.lock() else {
            tracing::warn!(target: "simile::inspection", "exchange recorder is unavailable");
            return;
        };
        if let Some(recorded) = exchanges
            .iter_mut()
            .find(|recorded| recorded.exchange.id == exchange_id)
        {
            recorded.report = Some(report);
        }
    }

    fn update_report(&self, report: RuntimeReportSummary) {
        let Ok(mut exchanges) = self.exchanges.lock() else {
            tracing::warn!(target: "simile::inspection", "exchange recorder is unavailable");
            return;
        };
        for recorded in &mut *exchanges {
            if recorded
                .report
                .as_ref()
                .is_some_and(|prepared| prepared.id == report.id)
            {
                recorded.report = Some(report.clone());
            }
        }
    }
}

pub(super) async fn inspection_response(state: &ServerState, request: Request<Body>) -> Response {
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
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let response = match path.as_str() {
        "/.simile/inspection" if method == Method::GET => {
            match state.service.inspect_state(false) {
                Ok(inspection) => {
                    let summary = state.service.artifact().summary();
                    let exchange_count =
                        inspector.snapshot().map_or(0, |exchanges| exchanges.len());
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
                Some(exchanges) => json_response(StatusCode::OK, json!({ "exchanges": exchanges })),
                None => error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "inspection_unavailable",
                    "runtime exchange inspection is unavailable",
                ),
            },
            Method::DELETE => match inspector.clear() {
                Some(removed) => json_response(StatusCode::OK, json!({ "removed": removed })),
                None => error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "inspection_unavailable",
                    "runtime exchange inspection is unavailable",
                ),
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
                Method::GET => inspector.report(exchange_id).map_or_else(
                    || {
                        error_response(
                            StatusCode::NOT_FOUND,
                            "report_not_prepared",
                            "this exchange has no prepared report",
                        )
                    },
                    |report| json_response(StatusCode::OK, json!(report)),
                ),
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
    if let Some(report) = inspector.report(exchange_id) {
        return json_response(StatusCode::OK, json!(report));
    }
    let Some(exchange) = inspector.report_capture(exchange_id) else {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "report_evidence_unavailable",
            "this exchange has no safely reproducible request evidence",
        );
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

async fn decode_inspection_json<T>(request: Request<Body>) -> std::result::Result<T, Response>
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
            HeaderValue::from_static("Content-Type"),
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

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_incident(
    state: &ServerState,
    profile: Option<String>,
    operation: &str,
    seed: u64,
    content_type: Option<String>,
    body: &[u8],
    parameters: IncomingParameters,
    error: &simile_runtime::Error,
) {
    if !error.is_reportable() {
        return;
    }
    let Some(sender) = state.reporting_sender.as_ref() else {
        return;
    };
    let Some(body) = sanitize_body(content_type.as_deref(), body) else {
        tracing::debug!(
            target: "simile::reporting",
            operation,
            content_type,
            "automatic report skipped because the request body cannot be safely sanitized"
        );
        return;
    };
    let incident = RuntimeIncident {
        profile,
        operation: operation.to_owned(),
        seed,
        content_type,
        body,
        parameters: sanitize_parameters(parameters),
        scenario: state.scenario.clone(),
        fault_profile: state.fault_profile.clone(),
        code: error.code().to_owned(),
        message: error.to_string(),
        issues: error
            .issues()
            .iter()
            .filter_map(|issue| serde_json::to_value(issue).ok())
            .collect(),
    };
    if let Err(error) = sender.try_send(RuntimeReportCommand::Automatic(incident)) {
        tracing::debug!(
            target: "simile::reporting",
            %error,
            "automatic report skipped because the reporting queue is unavailable"
        );
    }
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
    let media_type = content_type?.split(';').next()?.trim();
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
