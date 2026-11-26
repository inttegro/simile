use serde::{Deserialize, Serialize};
use serde_json::Value;
use simile_runtime::{FaultSource, OperationExecution};
use simile_spec::OutcomeProvenance;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecutionProof {
    version: u16,
    operation: ProofOperation,
    controls: ProofControls,
    selection: ProofSelection,
    validation: Vec<ProofCheck>,
    effects: ProofEffects,
    #[serde(skip_serializing_if = "Option::is_none")]
    fault: Option<ProofFault>,
    delivery: ProofDelivery,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProofOperation {
    pub key: String,
    pub method: String,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProofControls {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub response_seed: u64,
    pub logical_time_milliseconds: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProofSelection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    pub source: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProofCheck {
    pub name: String,
    pub result: String,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProofEffects {
    pub synchronous: Vec<ProofAction>,
    pub asynchronous: Vec<ProofAsyncOperation>,
    pub webhooks: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProofAction {
    pub resource: String,
    pub command: String,
    pub arguments: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProofAsyncOperation {
    pub name: String,
    pub actions: Vec<ProofAction>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProofFault {
    pub name: String,
    pub source: String,
    pub phase: String,
    pub action: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProofDelivery {
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    pub transport: String,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execution_proof(
    operation: &str,
    method: &str,
    path: &str,
    scenario: Option<&str>,
    fault_profile: Option<&str>,
    profile: Option<&str>,
    response_seed: u64,
    logical_time_milliseconds: i64,
    execution: &OperationExecution,
) -> ExecutionProof {
    let source = execution
        .provenance()
        .map(provenance_label)
        .unwrap_or("runtime_default")
        .to_owned();
    let reason = if let Some(scenario) = scenario {
        format!("scenario `{scenario}` selected the operation result")
    } else if let Some(outcome) = execution.selected_outcome() {
        format!("named outcome `{outcome}` satisfied the request and operation controls")
    } else {
        "the compiled contract synthesized the default reachable result".to_owned()
    };
    let action = |action: &simile_runtime::PlannedAction| ProofAction {
        resource: action.resource().as_str().to_owned(),
        command: json_string(action.kind()),
        arguments: action.arguments().keys().cloned().collect(),
    };
    let effects = execution.effects();
    let fault = execution.fault().map(|fault| ProofFault {
        name: fault.name().to_owned(),
        source: match fault.source() {
            FaultSource::Scenario(name) => format!("scenario:{name}"),
            FaultSource::Profile(name) => format!("profile:{name}"),
        },
        phase: json_string(fault.phase()),
        action: serde_json::to_value(fault.action()).unwrap_or(Value::Null),
    });
    ExecutionProof {
        version: 1,
        operation: ProofOperation {
            key: operation.to_owned(),
            method: method.to_owned(),
            path: path.to_owned(),
        },
        controls: ProofControls {
            scenario: scenario.map(str::to_owned),
            fault_profile: fault_profile.map(str::to_owned),
            profile: profile.map(str::to_owned),
            response_seed,
            logical_time_milliseconds,
        },
        selection: ProofSelection {
            outcome: execution.selected_outcome().map(str::to_owned),
            source,
            reason,
        },
        validation: vec![
            ProofCheck {
                name: "request".to_owned(),
                result: "passed".to_owned(),
                detail: "parameters and body satisfied the compiled request contract".to_owned(),
            },
            ProofCheck {
                name: "relations".to_owned(),
                result: "passed".to_owned(),
                detail: "request-response bindings and applicable constraints held together"
                    .to_owned(),
            },
            ProofCheck {
                name: "response".to_owned(),
                result: "passed".to_owned(),
                detail: "the final response satisfied its selected schema and outcome".to_owned(),
            },
        ],
        effects: ProofEffects {
            synchronous: effects.synchronous().iter().map(action).collect(),
            asynchronous: effects
                .asynchronous()
                .iter()
                .map(|operation| ProofAsyncOperation {
                    name: operation.name().to_owned(),
                    actions: operation.actions().iter().map(action).collect(),
                })
                .collect(),
            webhooks: effects
                .webhooks()
                .iter()
                .map(|webhook| webhook.name().to_owned())
                .collect(),
        },
        fault,
        delivery: ProofDelivery {
            status: execution.response().status(),
            content_type: execution.response().media_type().map(str::to_owned),
            transport: match execution.transport() {
                simile_runtime::TransportPlan::Respond => "respond".to_owned(),
                simile_runtime::TransportPlan::RespondAfter { milliseconds } => {
                    format!("respond_after:{milliseconds}ms")
                }
                simile_runtime::TransportPlan::DisconnectAfterCommit => {
                    "disconnect_after_commit".to_owned()
                }
            },
        },
    }
}

fn provenance_label(provenance: OutcomeProvenance) -> &'static str {
    match provenance {
        OutcomeProvenance::OpenApiDerived => "open_api_derived",
        OutcomeProvenance::ProviderAuthored => "provider_authored",
        OutcomeProvenance::RecordedReviewed => "recorded_reviewed",
        OutcomeProvenance::SchemaSynthesized => "schema_synthesized",
    }
}

fn json_string(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}
