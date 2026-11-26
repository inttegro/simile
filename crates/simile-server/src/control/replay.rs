use std::collections::BTreeSet;

use axum::http::StatusCode;
use axum::response::Response;
use serde::Serialize;
use serde_json::{Value, json};
use simile_runtime::{ExecutionOptions, OperationCall};

use super::http::unavailable as control_unavailable;
use super::proof::execution_proof;
use crate::response::{error_response, json_response};
use crate::{Inspector, ServerState};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SemanticDifference {
    path: String,
    kind: String,
    expected: Option<Value>,
    actual: Option<Value>,
}

pub(super) fn response(state: &ServerState, inspector: &Inspector, exchange_id: &str) -> Response {
    let recorded = match inspector.recorded(exchange_id) {
        Ok(Some(recorded)) => recorded,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "exchange_not_found",
                "the requested exchange is not retained",
            );
        }
        Err(_) => return control_unavailable(),
    };
    let Some(capture) = recorded.report_capture.as_ref() else {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "replay_evidence_unavailable",
            "this exchange has no safely reproducible request evidence",
        );
    };
    let parameters = match simile_runtime::validate_parameters(
        state.service.artifact(),
        &capture.operation,
        &capture.parameters,
    ) {
        Ok(parameters) => parameters,
        Err(error) => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "replay_parameters_invalid",
                &error.to_string(),
            );
        }
    };
    let execution = match state.service.runtime().execute_with_options(
        OperationCall {
            operation: &capture.operation,
            content_type: capture.content_type.as_deref(),
            body: &capture.body,
        },
        ExecutionOptions {
            outcome: capture.outcome.as_deref(),
            scenario: capture.scenario.as_deref(),
            fault_profile: capture.fault_profile.as_deref(),
            parameters: Some(&parameters),
            response_seed: Some(capture.response_seed),
            logical_time_milliseconds: capture.logical_time_milliseconds,
            ..ExecutionOptions::default()
        },
    ) {
        Ok(execution) => execution,
        Err(error) => {
            return error_response(StatusCode::CONFLICT, "replay_failed", &error.to_string());
        }
    };

    let actual = execution.response().value().cloned();
    let mut differences = Vec::new();
    compare_value(
        "/body",
        capture.expected.body.as_ref(),
        actual.as_ref(),
        &mut differences,
    );
    if capture.expected.status != execution.response().status() {
        differences.push(SemanticDifference {
            path: "/status".to_owned(),
            kind: "changed".to_owned(),
            expected: Some(json!(capture.expected.status)),
            actual: Some(json!(execution.response().status())),
        });
    }
    let actual_content_type = execution.response().media_type().map(str::to_owned);
    if capture.expected.content_type != actual_content_type {
        differences.push(SemanticDifference {
            path: "/contentType".to_owned(),
            kind: "changed".to_owned(),
            expected: capture.expected.content_type.clone().map(Value::String),
            actual: actual_content_type.clone().map(Value::String),
        });
    }
    let proof = execution_proof(
        &capture.operation,
        &recorded.exchange.method,
        &recorded.exchange.path,
        capture.scenario.as_deref(),
        capture.fault_profile.as_deref(),
        capture.profile.as_deref(),
        capture.response_seed,
        capture.logical_time_milliseconds.unwrap_or_default(),
        &execution,
    );
    inspector.control.record(
        "replay",
        format!("replayed {exchange_id}"),
        json!({ "exchangeId": exchange_id, "matches": differences.is_empty() }),
    );
    json_response(
        StatusCode::OK,
        json!({
            "exchangeId": exchange_id,
            "matches": differences.is_empty(),
            "differences": differences,
            "expected": capture.expected,
            "actual": {
                "status": execution.response().status(),
                "contentType": actual_content_type,
                "body": actual,
            },
            "proof": proof,
        }),
    )
}

fn compare_value(
    path: &str,
    expected: Option<&Value>,
    actual: Option<&Value>,
    differences: &mut Vec<SemanticDifference>,
) {
    if differences.len() >= 200 || expected == actual {
        return;
    }
    match (expected, actual) {
        (Some(Value::Object(expected)), Some(Value::Object(actual))) => {
            let keys = expected
                .keys()
                .chain(actual.keys())
                .collect::<BTreeSet<_>>();
            for key in keys {
                let next = format!("{path}/{}", escape_pointer(key));
                compare_value(&next, expected.get(key), actual.get(key), differences);
            }
        }
        (Some(Value::Array(expected)), Some(Value::Array(actual))) => {
            for index in 0..expected.len().max(actual.len()) {
                compare_value(
                    &format!("{path}/{index}"),
                    expected.get(index),
                    actual.get(index),
                    differences,
                );
            }
        }
        (None, Some(actual)) => differences.push(SemanticDifference {
            path: path.to_owned(),
            kind: "added".to_owned(),
            expected: None,
            actual: Some(actual.clone()),
        }),
        (Some(expected), None) => differences.push(SemanticDifference {
            path: path.to_owned(),
            kind: "removed".to_owned(),
            expected: Some(expected.clone()),
            actual: None,
        }),
        (expected, actual) => differences.push(SemanticDifference {
            path: path.to_owned(),
            kind: "changed".to_owned(),
            expected: expected.cloned(),
            actual: actual.cloned(),
        }),
    }
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
