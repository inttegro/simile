use std::net::SocketAddr;

use serde::Serialize;
use simile_artifact::Artifact;
use simile_spec::{DescriptionKind, FaultAction, FaultPhase, WebhookOrigin};

use super::start::prepare_loaded_service;
use crate::registry::Registry;
use crate::service_config::{LoadedServiceConfig, ServiceConfigSource};
use crate::{Context, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescribeServiceRequest {
    pub service: String,
    pub config: Option<String>,
    pub verbose: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeServiceReport {
    pub service: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub service_version: String,
    pub openapi_version: String,
    pub listen: SocketAddr,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub operation_count: usize,
    pub operations: Vec<DescribedOperation>,
    pub scenarios: Vec<DescribedScenario>,
    pub faults: Vec<DescribedFault>,
    pub fault_profiles: Vec<DescribedFaultProfile>,
    pub backends: Vec<DescribedBackend>,
    pub async_operations: Vec<DescribedAsyncOperation>,
    pub protocol_errors: Vec<DescribedProtocolError>,
    pub webhooks: DescribedWebhookPolicy,
    pub webhook_contracts: Vec<DescribedWebhook>,
    pub authoring_objects: Vec<DescribedObject>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribedWebhookPolicy {
    pub ordering: String,
    pub out_of_order_probability_ppm: u32,
    pub reorder_window_milliseconds: u64,
    pub concurrency: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribedWebhook {
    pub name: String,
    pub method: String,
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub content_types: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribedOperation {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub outcomes: Vec<DescribedOutcome>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribedOutcome {
    pub name: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribedScenario {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub selections: usize,
    pub faults: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribedFault {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub phase: String,
    pub action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribedFaultProfile {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub probability_ppm: u32,
    pub faults: Vec<String>,
    pub operations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribedBackend {
    pub reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribedAsyncOperation {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub backends: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribedProtocolError {
    pub cause: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub statuses: Vec<u16>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribedObject {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub name: String,
    pub description: String,
}

pub fn describe_service(
    ctx: &Context,
    request: &DescribeServiceRequest,
) -> Result<DescribeServiceReport> {
    let (artifact, listen, source, revision) = if let Some(config) = &request.config {
        let loaded =
            LoadedServiceConfig::load(ctx, ServiceConfigSource::from_argument(config.clone()))?;
        let prepared = prepare_loaded_service(&request.service, None, &loaded)?.service;
        (
            prepared.artifact,
            prepared.report.listen,
            config.clone(),
            None,
        )
    } else {
        let registered = Registry::new(ctx.simile_home()?).load(&request.service)?;
        let listen = SocketAddr::new(
            registered.manifest.listen.host,
            registered.manifest.listen.port,
        );
        (
            registered.artifact,
            listen,
            "registry".to_owned(),
            Some(registered.revision),
        )
    };

    Ok(report(request, &artifact, listen, source, revision))
}

fn report(
    request: &DescribeServiceRequest,
    artifact: &Artifact,
    listen: SocketAddr,
    source: String,
    revision: Option<String>,
) -> DescribeServiceReport {
    let summary = artifact.summary();
    let operations = if request.verbose {
        artifact
            .operations()
            .map(|operation| {
                let operation_id = operation.operation_id().map(str::to_owned);
                let description = operation_id
                    .as_deref()
                    .and_then(|id| artifact.description(DescriptionKind::Behavior, None, id))
                    .map(str::to_owned)
                    .or_else(|| operation.summary().map(str::to_owned));
                let outcome_parent = operation_id.as_deref().map(|id| format!("behavior:{id}"));
                let outcomes = operation
                    .outcomes()
                    .map(|outcome| DescribedOutcome {
                        name: outcome.name().to_owned(),
                        status: outcome.status(),
                        description: outcome_parent
                            .as_deref()
                            .and_then(|parent| {
                                artifact.description(
                                    DescriptionKind::Outcome,
                                    Some(parent),
                                    outcome.name(),
                                )
                            })
                            .map(str::to_owned),
                    })
                    .collect();
                DescribedOperation {
                    key: operation.key().to_owned(),
                    operation_id,
                    description,
                    outcomes,
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    let scenarios = artifact
        .scenarios()
        .map(|scenario| DescribedScenario {
            name: scenario.name().to_owned(),
            description: description(artifact, DescriptionKind::Scenario, scenario.name()),
            selections: scenario.selections().len(),
            faults: scenario.faults().len(),
        })
        .collect();
    let faults = artifact
        .faults()
        .map(|fault| DescribedFault {
            name: fault.name().to_owned(),
            description: description(artifact, DescriptionKind::Fault, fault.name()),
            phase: match fault.phase() {
                FaultPhase::BeforeResponse => "before_response",
                FaultPhase::AfterCommit => "after_commit",
            }
            .to_owned(),
            action: match fault.action() {
                FaultAction::Delay { milliseconds } => format!("delay {milliseconds}ms"),
                FaultAction::Disconnect => "disconnect".to_owned(),
            },
        })
        .collect();
    let fault_profiles = artifact
        .fault_profiles()
        .map(|profile| DescribedFaultProfile {
            name: profile.name().to_owned(),
            description: description(artifact, DescriptionKind::FaultProfile, profile.name()),
            probability_ppm: profile.probability_ppm(),
            faults: profile.faults().to_vec(),
            operations: profile.operations().to_vec(),
        })
        .collect();
    let backends = artifact
        .resources()
        .map(|resource| {
            let reference = resource.reference().to_string();
            DescribedBackend {
                description: description(artifact, DescriptionKind::Backend, &reference),
                reference,
            }
        })
        .collect();
    let async_operations = artifact
        .async_operations()
        .map(|operation| DescribedAsyncOperation {
            name: operation.name().to_owned(),
            description: description(artifact, DescriptionKind::AsyncOperation, operation.name()),
            backends: operation
                .resources()
                .map(|resource| resource.reference().to_string())
                .collect(),
        })
        .collect();
    let protocol_errors = artifact
        .protocol_errors()
        .map(|error| DescribedProtocolError {
            cause: error.cause().to_owned(),
            description: description(artifact, DescriptionKind::ProtocolError, error.cause()),
            statuses: error.statuses().to_vec(),
        })
        .collect();
    let authoring_objects = artifact
        .descriptions()
        .filter(|entry| {
            matches!(
                entry.kind(),
                DescriptionKind::ErrorSet | DescriptionKind::Outcome
            )
        })
        .map(|entry| DescribedObject {
            kind: kind_name(entry.kind()).to_owned(),
            parent: entry.parent().map(str::to_owned),
            name: entry.name().to_owned(),
            description: entry.text().to_owned(),
        })
        .collect();
    let webhook_policy = artifact.webhook_policy();
    let webhook_contracts = artifact
        .webhooks()
        .map(|webhook| DescribedWebhook {
            name: webhook.name().to_owned(),
            method: webhook.method().to_owned(),
            origin: match webhook.origin() {
                WebhookOrigin::OpenApiCallback => "openapi_callback",
                WebhookOrigin::OpenApiWebhook => "openapi_webhook",
                WebhookOrigin::ProviderAuthored => "provider_authored",
            }
            .to_owned(),
            operation_id: webhook.operation_id().map(str::to_owned),
            description: description(artifact, DescriptionKind::Webhook, webhook.name())
                .or_else(|| webhook.summary().map(str::to_owned)),
            target: webhook.target().map(str::to_owned),
            content_types: webhook
                .request_body()
                .map(|body| body.media_types().map(str::to_owned).collect())
                .unwrap_or_default(),
        })
        .collect();

    DescribeServiceReport {
        service: request.service.clone(),
        description: artifact
            .description(DescriptionKind::Service, None, &request.service)
            .map(str::to_owned)
            .or_else(|| artifact.service_description().map(str::to_owned)),
        service_version: summary.service_version,
        openapi_version: summary.openapi_version,
        listen,
        source,
        revision,
        operation_count: summary.operations,
        operations,
        scenarios,
        faults,
        fault_profiles,
        backends,
        async_operations,
        protocol_errors,
        webhooks: DescribedWebhookPolicy {
            ordering: webhook_policy.ordering.as_str().to_owned(),
            out_of_order_probability_ppm: webhook_policy.out_of_order_probability_ppm,
            reorder_window_milliseconds: webhook_policy.reorder_window_milliseconds,
            concurrency: webhook_policy.concurrency,
        },
        webhook_contracts,
        authoring_objects,
    }
}

fn description(artifact: &Artifact, kind: DescriptionKind, name: &str) -> Option<String> {
    artifact.description(kind, None, name).map(str::to_owned)
}

const fn kind_name(kind: DescriptionKind) -> &'static str {
    match kind {
        DescriptionKind::Service => "service",
        DescriptionKind::Behavior => "behavior",
        DescriptionKind::Outcome => "outcome",
        DescriptionKind::ErrorSet => "error_set",
        DescriptionKind::ProtocolError => "protocol_error",
        DescriptionKind::Scenario => "scenario",
        DescriptionKind::Backend => "backend",
        DescriptionKind::AsyncOperation => "async",
        DescriptionKind::Fault => "fault",
        DescriptionKind::FaultProfile => "fault_profile",
        DescriptionKind::Effect => "effect",
        DescriptionKind::Webhook => "webhook",
    }
}
