use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use simile_spec::{EffectOrigin, ParameterLocation};

use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditRequest {
    pub artifact: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditFinding {
    pub severity: AuditSeverity,
    pub code: String,
    pub subject: String,
    pub message: String,
    pub remediation: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditReport {
    pub service: String,
    pub service_version: String,
    pub artifact_sha256: String,
    pub operations: usize,
    pub provider_authored_outcomes: usize,
    pub provider_authored_effects: usize,
    pub convention_inferred_effects: usize,
    pub errors: usize,
    pub warnings: usize,
    pub findings: Vec<AuditFinding>,
}

/// Inspect where an artifact relies on convention or unconstrained synthesis.
///
/// This is deliberately an explainable audit, not a single opaque score. A
/// provider can fix each finding without learning Simile's internal compiler
/// decisions, and CI can fail only on the severities that matter to it.
pub fn audit(request: &AuditRequest) -> Result<AuditReport> {
    let encoded = fs::read(&request.artifact).map_err(|source| Error::ReadArtifact {
        path: request.artifact.clone(),
        source,
    })?;
    let artifact = simile_artifact::decode(&encoded)?;
    let summary = artifact.summary();
    let mut findings = Vec::new();
    let effects = artifact.effects().collect::<Vec<_>>();
    let effected_operations = effects
        .iter()
        .flat_map(|effect| effect.operations().iter().cloned())
        .collect::<BTreeSet<_>>();

    for operation in artifact.operations() {
        if operation.operation_id().is_none() {
            findings.push(finding(
                AuditSeverity::Warning,
                "operation.missing_id",
                operation.key(),
                "The operation has no stable operationId.",
                "Give the operation a unique operationId so HCL behavior, effects, and scenarios can name it.",
            ));
        }
        if summary
            .operation_outcomes
            .iter()
            .any(|candidate| candidate.operation == operation.key() && candidate.underspecified)
        {
            findings.push(finding(
                AuditSeverity::Warning,
                "outcome.schema_only",
                operation.key(),
                "Every outcome is selected and synthesized from schema alone.",
                "Declare public outcomes or examples for meaningful success and error paths.",
            ));
        }
        if mutates(operation.method())
            && summary.resources > 0
            && !effected_operations.contains(operation.key())
        {
            findings.push(finding(
                AuditSeverity::Warning,
                "state.no_effect",
                operation.key(),
                "The operation appears state-mutating but commits no resource effect.",
                "Make the response entity unambiguous for convention inference or declare an HCL effect.",
            ));
        }
        for parameter in operation.parameters() {
            if parameter.schema().is_none() {
                findings.push(finding(
                    AuditSeverity::Warning,
                    "parameter.untyped",
                    &format!("{} {} {}", operation.key(), location(parameter.location()), parameter.name()),
                    "The parameter has no schema, so Simile can prove presence but not value soundness.",
                    "Add an OpenAPI schema to the parameter.",
                ));
            }
            if !supported_style(parameter.location(), parameter.style()) {
                findings.push(finding(
                    AuditSeverity::Error,
                    "parameter.unsupported_style",
                    &format!("{} {}", operation.key(), parameter.name()),
                    &format!(
                        "The runtime does not yet decode the {} style for this parameter location.",
                        parameter.style()
                    ),
                    "Use a currently supported style or retain this finding until the decoder supports it.",
                ));
            }
        }
    }

    let mut used_resources = BTreeSet::new();
    for effect in &effects {
        for action in effect.actions() {
            used_resources.insert(action.resource.to_string());
        }
        if effect.origin() == EffectOrigin::ConventionInferred {
            findings.push(finding(
                AuditSeverity::Info,
                "effect.inferred",
                effect.name(),
                "Simile inferred this effect from the public operation and resource contract.",
                "Keep the convention when it matches the service, or replace it with an authored effect.",
            ));
        }
    }
    for operation in artifact.async_operations() {
        for action in operation.actions() {
            used_resources.insert(action.resource.to_string());
        }
    }
    for resource in artifact.resources() {
        let name = resource.reference().to_string();
        if !used_resources.contains(&name) {
            findings.push(finding(
                AuditSeverity::Warning,
                "resource.unused",
                &name,
                "The resource is declared but no synchronous or asynchronous action uses it.",
                "Remove the resource or connect it through an inferred or authored effect.",
            ));
        }
    }

    for cause in [
        "malformed_request",
        "invalid_body",
        "unsupported_media_type",
        "idempotency_conflict",
    ] {
        if artifact.protocol_error(cause).is_none() {
            findings.push(finding(
                AuditSeverity::Info,
                "error.unmapped_protocol_cause",
                cause,
                "This public protocol failure falls back to Simile's diagnostic envelope.",
                "Map the cause to a public service error when provider-faithful errors matter.",
            ));
        }
    }

    findings.sort_by(|left, right| {
        right
            .severity
            .cmp(&left.severity)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.subject.cmp(&right.subject))
    });
    let errors = findings
        .iter()
        .filter(|finding| finding.severity == AuditSeverity::Error)
        .count();
    let warnings = findings
        .iter()
        .filter(|finding| finding.severity == AuditSeverity::Warning)
        .count();
    Ok(AuditReport {
        service: summary.service,
        service_version: summary.service_version,
        artifact_sha256: summary.payload_sha256,
        operations: summary.operations,
        provider_authored_outcomes: summary.provider_authored_outcomes,
        provider_authored_effects: summary.provider_authored_effects,
        convention_inferred_effects: summary.convention_inferred_effects,
        errors,
        warnings,
        findings,
    })
}

fn finding(
    severity: AuditSeverity,
    code: &str,
    subject: &str,
    message: &str,
    remediation: &str,
) -> AuditFinding {
    AuditFinding {
        severity,
        code: code.to_owned(),
        subject: subject.to_owned(),
        message: message.to_owned(),
        remediation: remediation.to_owned(),
    }
}

fn mutates(method: &str) -> bool {
    matches!(method, "POST" | "PUT" | "PATCH" | "DELETE")
}

fn supported_style(location: ParameterLocation, style: &str) -> bool {
    match location {
        // The runtime currently decodes the common OpenAPI styles below. The
        // audit must describe executable support, not merely spec validity.
        ParameterLocation::Path => style == "simple",
        ParameterLocation::Query => {
            matches!(
                style,
                "form" | "spaceDelimited" | "pipeDelimited" | "deepObject"
            )
        }
        ParameterLocation::Header => style == "simple",
        ParameterLocation::Cookie => style == "form",
    }
}

const fn location(location: ParameterLocation) -> &'static str {
    match location {
        ParameterLocation::Path => "path",
        ParameterLocation::Query => "query",
        ParameterLocation::Header => "header",
        ParameterLocation::Cookie => "cookie",
    }
}
