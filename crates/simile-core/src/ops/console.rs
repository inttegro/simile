use std::collections::BTreeMap;
use std::net::SocketAddr;

use serde::Serialize;
use serde_json::Value;
use simile_artifact::Artifact;
use simile_spec::{BehaviorOrigin, OutcomeProvenance};

use super::audit::{AuditFinding, audit_artifact};
use super::describe::{
    DescribeServiceReport, DescribedAsyncOperation, DescribedBackend, DescribedFault,
    DescribedFaultProfile, DescribedPrincipal, DescribedProfile, DescribedScenario, report,
};
use crate::registry::{RegisteredService, Registry, SourceLocation};
use crate::{Context, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsoleRequest {
    pub service: Option<String>,
}

/// One complete, sanitized view of the local service registry.
///
/// Terminal and browser presentations consume this same model. It contains
/// public contract metadata and local runtime addresses, never profile secret
/// values or raw configuration contents.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleCatalog {
    pub services: Vec<ConsoleServiceEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<ConsoleService>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleServiceEntry {
    pub name: String,
    pub version: String,
    pub listen: SocketAddr,
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleService {
    pub service: ConsoleServiceIdentity,
    pub artifact: ConsoleArtifact,
    pub metrics: ConsoleMetrics,
    pub operations: Vec<ConsoleOperation>,
    pub scenarios: Vec<DescribedScenario>,
    pub faults: Vec<DescribedFault>,
    pub fault_profiles: Vec<DescribedFaultProfile>,
    pub backends: Vec<DescribedBackend>,
    pub async_operations: Vec<DescribedAsyncOperation>,
    pub principals: Vec<DescribedPrincipal>,
    pub profiles: Vec<DescribedProfile>,
    pub findings: Vec<AuditFinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleServiceIdentity {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub version: String,
    pub openapi_version: String,
    pub listen: SocketAddr,
    pub source: String,
    pub contract: String,
    pub revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleArtifact {
    pub compiler_version: String,
    pub format_version: u16,
    pub bytes: usize,
    pub sha256: String,
    pub public_export_safe: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleMetrics {
    pub operations: usize,
    pub responses: usize,
    pub outcomes: usize,
    pub provider_authored_outcomes: usize,
    pub schema_synthesized_outcomes: usize,
    pub underspecified_operations: usize,
    pub scenario_selections: usize,
    pub scenarios: usize,
    pub faults: usize,
    pub fault_profiles: usize,
    pub backends: usize,
    pub async_operations: usize,
    pub principals: usize,
    pub profiles: usize,
    pub errors: usize,
    pub warnings: usize,
    pub provenance_counts: BTreeMap<String, usize>,
    pub status_counts: BTreeMap<String, usize>,
    pub finding_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleOperation {
    pub id: String,
    pub key: String,
    pub method: String,
    pub path: String,
    pub summary: String,
    pub description: String,
    pub secured: bool,
    pub behavior_source: String,
    pub success: u16,
    pub outcomes: Vec<ConsoleOutcome>,
    pub underspecified: bool,
    pub request_schema: String,
    pub response_schema: String,
    pub request_example: Value,
    pub response_example: Value,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleOutcome {
    pub name: String,
    pub status: u16,
    pub provenance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,
    pub description: String,
    pub response_schema: String,
    pub response_variants: usize,
    pub headers: Vec<String>,
}

pub fn console(ctx: &Context, request: &ConsoleRequest) -> Result<ConsoleCatalog> {
    let registered = Registry::new(ctx.simile_home()?).list()?;
    let services = registered.iter().map(service_entry).collect::<Vec<_>>();
    let selected = match request.service.as_deref() {
        Some(name) => {
            let registered = registered
                .iter()
                .find(|candidate| candidate.manifest.name == name)
                .cloned()
                .map_or_else(|| Registry::new(ctx.simile_home()?).load(name), Ok)?;
            Some(service_model(registered))
        }
        None => registered.into_iter().next().map(service_model),
    };
    Ok(ConsoleCatalog { services, selected })
}

fn service_entry(registered: &RegisteredService) -> ConsoleServiceEntry {
    ConsoleServiceEntry {
        name: registered.manifest.name.clone(),
        version: registered.manifest.summary.service_version.clone(),
        listen: SocketAddr::new(
            registered.manifest.listen.host,
            registered.manifest.listen.port,
        ),
        revision: registered.revision.clone(),
    }
}

fn service_model(registered: RegisteredService) -> ConsoleService {
    let listen = SocketAddr::new(
        registered.manifest.listen.host,
        registered.manifest.listen.port,
    );
    let source = source_label(&registered.manifest.config_source);
    let contract = source_label(&registered.manifest.openapi_source);
    let described = report(
        &registered.manifest.name,
        true,
        &registered.artifact,
        listen,
        source.clone(),
        None,
        Some(registered.revision.clone()),
    );
    model_from_parts(
        &registered.artifact,
        registered.artifact_bytes.len(),
        source,
        contract,
        registered.revision,
        described,
    )
}

fn model_from_parts(
    artifact: &Artifact,
    encoded_bytes: usize,
    source: String,
    contract: String,
    revision: String,
    described: DescribeServiceReport,
) -> ConsoleService {
    let summary = artifact.summary();
    let audit = audit_artifact(artifact);
    let described_operations = described
        .operations
        .iter()
        .filter_map(|operation| operation.operation_id.as_deref().map(|id| (id, operation)))
        .collect::<BTreeMap<_, _>>();
    let operation_summaries = summary
        .operation_outcomes
        .iter()
        .map(|operation| (operation.operation.as_str(), operation))
        .collect::<BTreeMap<_, _>>();
    let operations = artifact
        .operations()
        .map(|operation| {
            let id = operation
                .operation_id()
                .unwrap_or_else(|| operation.key())
                .to_owned();
            let described = described_operations.get(id.as_str()).copied();
            let outcome_summary = operation_summaries.get(operation.key()).copied();
            let outcomes = operation
                .outcomes()
                .map(|outcome| ConsoleOutcome {
                    name: outcome.name().to_owned(),
                    status: outcome.status(),
                    provenance: provenance(outcome.provenance()).to_owned(),
                    content_type: outcome.content_type().map(str::to_owned),
                    example: outcome.body().cloned(),
                    description: described
                        .and_then(|operation| {
                            operation
                                .outcomes
                                .iter()
                                .find(|candidate| candidate.name == outcome.name())
                        })
                        .and_then(|outcome| outcome.description.clone())
                        .unwrap_or_default(),
                    response_schema: format!("HTTP {} response", outcome.status()),
                    response_variants: 1,
                    headers: Vec::new(),
                })
                .collect::<Vec<_>>();
            let success = outcomes
                .iter()
                .find(|outcome| (200..400).contains(&outcome.status))
                .map_or(200, |outcome| outcome.status);
            let response_example = outcomes
                .iter()
                .find(|outcome| outcome.status == success)
                .and_then(|outcome| outcome.example.clone())
                .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
            let secured = described.map_or_else(
                || {
                    operation
                        .security_requirements()
                        .any(|requirement| !requirement.is_empty())
                },
                |operation| operation.secured,
            );
            ConsoleOperation {
                id,
                key: operation.key().to_owned(),
                method: operation.method().to_owned(),
                path: operation.path().to_owned(),
                summary: operation
                    .summary()
                    .or_else(|| described.and_then(|operation| operation.description.as_deref()))
                    .unwrap_or(operation.key())
                    .to_owned(),
                description: described
                    .and_then(|operation| operation.description.clone())
                    .unwrap_or_default(),
                secured,
                behavior_source: match operation.origin() {
                    BehaviorOrigin::OpenApi => "contract_derived",
                    BehaviorOrigin::Hcl => "provider_authored",
                }
                .to_owned(),
                success,
                outcomes,
                underspecified: outcome_summary.is_none_or(|operation| operation.underspecified),
                request_schema: operation
                    .request_body()
                    .map(|body| body.media_types().collect::<Vec<_>>().join(", "))
                    .unwrap_or_else(|| "No request body".to_owned()),
                response_schema: format!("HTTP {success} response"),
                request_example: Value::Object(serde_json::Map::new()),
                response_example,
                tags: Vec::new(),
            }
        })
        .collect::<Vec<_>>();

    let provenance_counts = summary
        .operation_outcomes
        .iter()
        .flat_map(|operation| &operation.outcomes)
        .fold(BTreeMap::new(), |mut counts, outcome| {
            *counts
                .entry(provenance(outcome.provenance).to_owned())
                .or_default() += 1;
            counts
        });
    let status_counts = summary
        .operation_outcomes
        .iter()
        .flat_map(|operation| &operation.outcomes)
        .fold(BTreeMap::new(), |mut counts, outcome| {
            *counts
                .entry(format!("{}xx", outcome.status / 100))
                .or_default() += 1;
            counts
        });
    let finding_counts = audit
        .findings
        .iter()
        .fold(BTreeMap::new(), |mut counts, finding| {
            *counts.entry(finding.code.clone()).or_default() += 1;
            counts
        });

    ConsoleService {
        service: ConsoleServiceIdentity {
            name: described.service,
            description: described.description,
            version: described.service_version,
            openapi_version: described.openapi_version,
            listen: described.listen,
            source,
            contract,
            revision,
        },
        artifact: ConsoleArtifact {
            compiler_version: summary.compiler_version,
            format_version: summary.format_version,
            bytes: encoded_bytes,
            sha256: summary.payload_sha256,
            public_export_safe: summary.public_export_safe,
        },
        metrics: ConsoleMetrics {
            operations: summary.operations,
            responses: summary.responses,
            outcomes: summary.outcomes,
            provider_authored_outcomes: summary.provider_authored_outcomes,
            schema_synthesized_outcomes: summary.schema_synthesized_outcomes,
            underspecified_operations: summary.underspecified_operations,
            scenario_selections: summary.scenario_selections,
            scenarios: described.scenarios.len(),
            faults: summary.faults,
            fault_profiles: summary.fault_profiles,
            backends: described.backends.len(),
            async_operations: described.async_operations.len(),
            principals: described.principals.len(),
            profiles: described.profiles.len(),
            errors: audit.errors,
            warnings: audit.warnings,
            provenance_counts,
            status_counts,
            finding_counts,
        },
        operations,
        scenarios: described.scenarios,
        faults: described.faults,
        fault_profiles: described.fault_profiles,
        backends: described.backends,
        async_operations: described.async_operations,
        principals: described.principals,
        profiles: described.profiles,
        findings: audit.findings,
    }
}

fn source_label(source: &SourceLocation) -> String {
    match source {
        SourceLocation::Path { value } | SourceLocation::Url { value } => value.clone(),
    }
}

const fn provenance(provenance: OutcomeProvenance) -> &'static str {
    match provenance {
        OutcomeProvenance::OpenApiDerived => "open_api_derived",
        OutcomeProvenance::ProviderAuthored => "provider_authored",
        OutcomeProvenance::RecordedReviewed => "recorded_reviewed",
        OutcomeProvenance::SchemaSynthesized => "schema_synthesized",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::BuildInfo;
    use crate::ops::{RegisterRequest, register};

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn discovers_registered_services_and_selects_by_name() {
        let root = std::env::temp_dir().join(format!("simile-console-{}", std::process::id()));
        drop(fs::remove_dir_all(&root));
        let ctx = Context::with_simile_home(
            BuildInfo::new("test", None::<String>, "test", "test"),
            &root,
            root.join("home"),
        );
        register(
            &ctx,
            &RegisterRequest {
                service: Some("orders".to_owned()),
                config: Some(fixture("service-orders.hcl").to_string_lossy().into_owned()),
                force: false,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

        let catalog = console(
            &ctx,
            &ConsoleRequest {
                service: Some("orders".to_owned()),
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(catalog.services.len(), 1);
        let selected = catalog
            .selected
            .unwrap_or_else(|| panic!("selected service"));
        assert_eq!(selected.service.name, "orders");
        assert_eq!(selected.metrics.operations, 3);
        assert!(
            selected
                .operations
                .iter()
                .any(|operation| operation.id == "createOrder")
        );

        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("{error}"));
    }
}
