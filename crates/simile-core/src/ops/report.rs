use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use semver::Version;
use serde::{Deserialize, Serialize};

use super::profile::profile_environment;
use super::reproduce::{
    ExchangeBundleRequest, IncidentBundleRequest, ReproductionDiagnostic, verify_case,
    write_exchange, write_incident,
};
use super::run::ExecutedResponse;
use crate::registry::Registry;
use crate::reporting::{ReportingStore, validate_contact, validate_endpoint};
use crate::{
    Context, Error, ReportDiagnostic, ReportDocument, ReportKind, ReportStatus, ReportTrigger,
    ReportingPreference, Result, UpgradeRecommendation,
};
use simile_runtime::IncomingParameters;

const MAX_SUBMISSION_BYTES: usize = 8 * 1024 * 1024;
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigureReportingRequest {
    pub service: String,
    pub profile: Option<String>,
    pub automatic: bool,
    pub contact: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureReportingReport {
    pub service: String,
    pub profile: String,
    pub automatic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportingPreferenceSummary {
    pub service: String,
    pub profile: String,
    pub automatic: bool,
    pub endpoint: String,
    pub contact: String,
    pub consented_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareReportRequest {
    pub service: String,
    pub profile: Option<String>,
    pub bundle: PathBuf,
    pub title: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedReport {
    pub report: ReportDocument,
    pub directory: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportsReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    pub reports: Vec<ReportDocument>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendReportRequest {
    pub report: String,
    pub current_version: bool,
    pub contact: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendReportReport {
    pub report: ReportDocument,
    pub sent: bool,
    pub directory: PathBuf,
}

#[derive(Clone, Debug)]
pub struct AutomaticReportRequest {
    pub service: String,
    pub profile: Option<String>,
    pub artifact_bytes: Vec<u8>,
    pub operation: String,
    pub seed: u64,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub parameters: IncomingParameters,
    pub scenario: Option<String>,
    pub fault_profile: Option<String>,
    pub diagnostic: ReportDiagnostic,
}

#[derive(Clone, Debug)]
pub struct RuntimeExchangeReportRequest {
    pub service: String,
    pub profile: Option<String>,
    pub artifact_bytes: Vec<u8>,
    pub operation: String,
    pub response_seed: u64,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub parameters: IncomingParameters,
    pub outcome: Option<String>,
    pub scenario: Option<String>,
    pub fault_profile: Option<String>,
    pub expected: ExecutedResponse,
    pub title: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreflightRequest<'a> {
    simile_version: &'a str,
    service: &'a str,
    service_version: &'a str,
    artifact_digest: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreflightResponse {
    #[serde(default)]
    recommendations: Vec<UpgradeRecommendation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmissionResponse {
    id: String,
    status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Reporter<'a> {
    contact: ReporterContact<'a>,
    consent: ReporterConsent,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReporterContact<'a> {
    channel: &'static str,
    address: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReporterConsent {
    clarification: bool,
    resolution_updates: bool,
}

struct SubmissionIdentity {
    endpoint: String,
    contact: String,
}

pub fn configure_reporting(
    ctx: &Context,
    request: &ConfigureReportingRequest,
) -> Result<ConfigureReportingReport> {
    let profile = resolve_profile(ctx, &request.service, request.profile.as_deref())?;
    let store = ReportingStore::new(ctx.simile_home()?);
    if !request.automatic {
        let path = store.remove_preference(&request.service, &profile)?;
        return Ok(ConfigureReportingReport {
            service: request.service.clone(),
            profile,
            automatic: false,
            endpoint: None,
            contact: None,
            path,
        });
    }

    let contact = request
        .contact
        .as_deref()
        .ok_or(Error::ReportContactRequired)?;
    validate_contact(contact)?;
    let endpoint =
        reporting_endpoint(ctx, &request.service)?.ok_or_else(|| Error::ReportingNotDeclared {
            service: request.service.clone(),
        })?;
    validate_endpoint(&endpoint)?;
    let preference =
        ReportingPreference::automatic(&request.service, &profile, endpoint.clone(), contact)?;
    let path = store.save_preference(&preference)?;
    Ok(ConfigureReportingReport {
        service: request.service.clone(),
        profile,
        automatic: true,
        endpoint: Some(endpoint),
        contact: Some(mask_contact(contact)),
        path,
    })
}

pub fn reporting_preferences(
    ctx: &Context,
    service: &str,
) -> Result<Vec<ReportingPreferenceSummary>> {
    Registry::new(ctx.simile_home()?).load(service)?;
    ReportingStore::new(ctx.simile_home()?)
        .preferences(service)
        .map(|preferences| {
            preferences
                .into_iter()
                .map(|preference| ReportingPreferenceSummary {
                    service: preference.service,
                    profile: preference.profile,
                    automatic: preference.automatic,
                    endpoint: preference.endpoint,
                    contact: mask_contact(&preference.contact),
                    consented_at: preference.consented_at,
                })
                .collect()
        })
}

pub fn reporting_destination(ctx: &Context, service: &str) -> Result<Option<String>> {
    reporting_endpoint(ctx, service)
}

pub fn prepare_report(ctx: &Context, request: &PrepareReportRequest) -> Result<PreparedReport> {
    let verified = verify_case(&request.bundle)?;
    if !verified.reproduced {
        return Err(Error::ReportDidNotReproduce {
            bundle: request.bundle.clone(),
        });
    }
    if verified.service != request.service {
        return Err(Error::ReportServiceMismatch {
            requested: request.service.clone(),
            reproduced: verified.service,
        });
    }
    // Why: a reproduction bundle carries the exact artifact needed for independent verification;
    // requiring a local registry entry would make a clean receiver trust provider-side setup.
    let profile = request
        .profile
        .as_deref()
        .map(|profile| profile_environment(ctx, &request.service, Some(profile)))
        .transpose()?
        .flatten()
        .map(|profile| profile.profile);
    let store = ReportingStore::new(ctx.simile_home()?);
    let report = ReportDocument::verified(
        ReportKind::EmulationInconsistency,
        ReportTrigger::User,
        &request.title,
        request.description.clone(),
        &request.service,
        profile,
        &ctx.build().version,
        verified.service_version,
        verified.operation,
        verified.artifact_digest,
        ReportDiagnostic {
            code: "emulation_inconsistency".to_owned(),
            message: request.title.clone(),
            issues: Vec::new(),
        },
        verified.fingerprint,
        false,
    )?;
    let directory = store.install_report(&report, &request.bundle)?;
    Ok(PreparedReport { report, directory })
}

/// Reproduce one sanitized runtime exchange and install it as a canonical verified report.
pub fn prepare_runtime_exchange_report(
    ctx: &Context,
    request: RuntimeExchangeReportRequest,
) -> Result<PreparedReport> {
    let capture = ctx.simile_home()?.join("tmp").join(format!(
        ".report-case-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let artifact = simile_artifact::decode(&request.artifact_bytes)?;
    let summary = artifact.summary();
    write_exchange(&ExchangeBundleRequest {
        path: &capture,
        artifact_bytes: &request.artifact_bytes,
        service: &request.service,
        service_version: &summary.service_version,
        operation: &request.operation,
        response_seed: request.response_seed,
        content_type: request.content_type.as_deref(),
        body: &request.body,
        parameters: &request.parameters,
        outcome: request.outcome.as_deref(),
        scenario: request.scenario.as_deref(),
        fault_profile: request.fault_profile.as_deref(),
        expected: &request.expected,
    })?;
    let result = prepare_report(
        ctx,
        &PrepareReportRequest {
            service: request.service,
            profile: request.profile,
            bundle: capture.clone(),
            title: request.title,
            description: request.description,
        },
    );
    drop(fs::remove_dir_all(&capture));
    result
}

/// Prepare, verify, preflight, and submit an automatically observed emulator defect.
///
/// A missing preference is an ordinary no-op. This lets transports emit candidate incidents
/// without learning consent or contact policy.
pub fn process_automatic_report(
    ctx: &Context,
    request: AutomaticReportRequest,
) -> Result<Option<SendReportReport>> {
    let Some(profile) = request.profile.as_deref() else {
        return Ok(None);
    };
    let store = ReportingStore::new(ctx.simile_home()?);
    let Some(preference) = store.preference(&request.service, profile)? else {
        return Ok(None);
    };
    if !preference.automatic {
        return Ok(None);
    }
    let capture = ctx.simile_home()?.join("tmp").join(format!(
        ".report-case-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let artifact = simile_artifact::decode(&request.artifact_bytes)?;
    let summary = artifact.summary();
    write_incident(&IncidentBundleRequest {
        path: &capture,
        artifact_bytes: &request.artifact_bytes,
        service: &request.service,
        service_version: &summary.service_version,
        operation: &request.operation,
        seed: request.seed,
        content_type: request.content_type.as_deref(),
        body: &request.body,
        parameters: &request.parameters,
        scenario: request.scenario.as_deref(),
        fault_profile: request.fault_profile.as_deref(),
        expected: ReproductionDiagnostic {
            code: request.diagnostic.code.clone(),
            message: request.diagnostic.message.clone(),
            issues: request.diagnostic.issues.clone(),
        },
    })?;
    let result = (|| {
        let verified = verify_case(&capture)?;
        if !verified.reproduced {
            return Err(Error::ReportDidNotReproduce {
                bundle: capture.clone(),
            });
        }
        if store.has_fingerprint(&request.service, &verified.fingerprint)? {
            return Ok(None);
        }
        let report = ReportDocument::verified(
            ReportKind::InvariantViolation,
            ReportTrigger::Automatic,
            format!(
                "{} while running {}",
                request.diagnostic.code, request.operation
            ),
            None,
            &request.service,
            Some(profile.to_owned()),
            &ctx.build().version,
            verified.service_version,
            verified.operation,
            verified.artifact_digest,
            request.diagnostic,
            verified.fingerprint,
            true,
        )?;
        store.install_report(&report, &capture)?;
        send_report(
            ctx,
            &SendReportRequest {
                report: report.id,
                current_version: false,
                contact: None,
            },
        )
        .map(Some)
    })();
    drop(fs::remove_dir_all(&capture));
    result
}

pub fn report(ctx: &Context, id: &str) -> Result<PreparedReport> {
    let (report, directory) = ReportingStore::new(ctx.simile_home()?).report(id)?;
    Ok(PreparedReport { report, directory })
}

pub fn reports(ctx: &Context, service: Option<&str>) -> Result<ReportsReport> {
    if let Some(service) = service {
        Registry::new(ctx.simile_home()?).load(service)?;
    }
    Ok(ReportsReport {
        service: service.map(str::to_owned),
        reports: ReportingStore::new(ctx.simile_home()?).reports(service)?,
    })
}

pub fn send_report(ctx: &Context, request: &SendReportRequest) -> Result<SendReportReport> {
    let store = ReportingStore::new(ctx.simile_home()?);
    let (mut report, directory) = store.report(&request.report)?;
    let verified = verify_case(&directory.join(&report.reproduction))?;
    if !verified.reproduced {
        report.status = ReportStatus::VerificationFailed;
        report.verification.reproduced = false;
        store.save_report(&report)?;
        return Err(Error::ReportDidNotReproduce {
            bundle: directory.join(&report.reproduction),
        });
    }

    let submission = submission_identity(ctx, &report, request.contact.as_deref())?;
    let mut recommendations = local_recommendations(ctx, &report, &verified.bundle_simile_version)?;
    recommendations.extend(remote_preflight(&submission, &report)?);
    recommendations.sort_by(|left, right| left.component.cmp(&right.component));
    recommendations.dedup_by(|left, right| left.component == right.component);
    report.upgrade_recommendations = recommendations;
    if !report.upgrade_recommendations.is_empty() && !request.current_version {
        report.status = ReportStatus::AwaitingUpgrade;
        store.save_report(&report)?;
        return Ok(SendReportReport {
            report,
            sent: false,
            directory,
        });
    }

    report.contactable = true;
    let response = submit(&submission, &report, &directory)?;
    if response.id != report.id {
        return Err(Error::ReportSubmissionResponse(
            "the report service returned a different report identifier".to_owned(),
        ));
    }
    report.status = match response.status.as_str() {
        "accepted" => ReportStatus::Accepted,
        "submitted" | "pending_verification" => ReportStatus::Submitted,
        other => {
            return Err(Error::ReportSubmissionResponse(format!(
                "unknown report status `{other}`"
            )));
        }
    };
    store.save_report(&report)?;
    Ok(SendReportReport {
        report,
        sent: true,
        directory,
    })
}

pub(crate) fn reporting_endpoint(ctx: &Context, service: &str) -> Result<Option<String>> {
    let registered = Registry::new(ctx.simile_home()?).load(service)?;
    let document = simile_hcl::parse_service(simile_hcl::Source::new(
        &registered.manifest.config_source.to_string(),
        &registered.config_contents,
    ))?;
    Ok(document
        .service
        .reporting
        .map(|reporting| reporting.endpoint))
}

fn resolve_profile(ctx: &Context, service: &str, requested: Option<&str>) -> Result<String> {
    profile_environment(ctx, service, requested)?
        .map(|profile| profile.profile)
        .ok_or_else(|| Error::ReportProfileRequired {
            service: service.to_owned(),
        })
}

fn submission_identity(
    ctx: &Context,
    report: &ReportDocument,
    one_time_contact: Option<&str>,
) -> Result<SubmissionIdentity> {
    if let Some(contact) = one_time_contact {
        validate_contact(contact)?;
        let endpoint = reporting_endpoint(ctx, &report.service)?.ok_or_else(|| {
            Error::ReportingNotDeclared {
                service: report.service.clone(),
            }
        })?;
        return Ok(SubmissionIdentity {
            endpoint,
            contact: contact.to_owned(),
        });
    }

    let profile = report
        .profile
        .as_deref()
        .ok_or(Error::ReportContactRequired)?;
    let preference = ReportingStore::new(ctx.simile_home()?)
        .preference(&report.service, profile)?
        .ok_or(Error::ReportContactRequired)?;
    Ok(SubmissionIdentity {
        endpoint: preference.endpoint,
        contact: preference.contact,
    })
}

fn local_recommendations(
    ctx: &Context,
    report: &ReportDocument,
    bundle_simile_version: &Option<String>,
) -> Result<Vec<UpgradeRecommendation>> {
    let mut recommendations = Vec::new();
    if let Some(bundle_version) = bundle_simile_version {
        if version_is_newer(&ctx.build().version, bundle_version) {
            recommendations.push(UpgradeRecommendation {
                component: "simile".to_owned(),
                current: bundle_version.clone(),
                available: ctx.build().version.clone(),
                command: "simile version".to_owned(),
            });
        }
    }
    if let Ok(registered) = Registry::new(ctx.simile_home()?).load(&report.service) {
        let available = registered.manifest.summary.service_version;
        if version_is_newer(&available, &report.service_version) {
            recommendations.push(UpgradeRecommendation {
                component: "service".to_owned(),
                current: report.service_version.clone(),
                available,
                command: format!("simile update {}", report.service),
            });
        }
    }
    Ok(recommendations)
}

fn version_is_newer(candidate: &str, current: &str) -> bool {
    match (Version::parse(candidate), Version::parse(current)) {
        (Ok(candidate), Ok(current)) => candidate > current,
        _ => candidate != current,
    }
}

fn remote_preflight(
    submission: &SubmissionIdentity,
    report: &ReportDocument,
) -> Result<Vec<UpgradeRecommendation>> {
    let url = format!("{}/preflight", submission.endpoint);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent(concat!("simile/", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let request_body = serde_json::to_vec(&PreflightRequest {
        simile_version: &report.simile_version,
        service: &report.service,
        service_version: &report.service_version,
        artifact_digest: &report.artifact_digest,
    })
    .map_err(|source| Error::ReportSubmissionResponse(source.to_string()))?;
    let mut response = agent
        .post(&url)
        .header("content-type", "application/json")
        .send(request_body)
        .map_err(|source| Error::ReportPreflight {
            endpoint: url,
            source,
        })?;
    read_response_json::<PreflightResponse>(&mut response).map(|response| response.recommendations)
}

fn submit(
    submission: &SubmissionIdentity,
    report: &ReportDocument,
    directory: &Path,
) -> Result<SubmissionResponse> {
    let boundary = format!("simile-{:016x}", rand::random::<u64>());
    let mut body = Vec::new();
    let reporter = Reporter {
        contact: ReporterContact {
            channel: "email",
            address: &submission.contact,
        },
        consent: ReporterConsent {
            clarification: true,
            resolution_updates: true,
        },
    };
    // Why: the durable local record becomes `submitted` after a successful attempt, but every
    // wire submission is a freshly verifiable report. Normalizing the transport copy keeps an
    // interrupted retry valid without weakening the receiver's initial-state validation.
    let mut transport_report = report.clone();
    transport_report.status = ReportStatus::Verified;
    multipart_json(&mut body, &boundary, "report", &transport_report)?;
    multipart_json(&mut body, &boundary, "reporter", &reporter)?;
    let reproduction = directory.join(&report.reproduction);
    for (name, content_type) in [
        ("bundle.json", "application/json"),
        ("request.json", "application/json"),
        ("request.body", "application/octet-stream"),
        ("service.similec", "application/octet-stream"),
    ] {
        let path = reproduction.join(name);
        if path.is_file() {
            multipart_file(
                &mut body,
                &boundary,
                name,
                content_type,
                &fs::read(&path).map_err(|source| Error::ReportingIo {
                    action: "read report attachment",
                    path: path.clone(),
                    source,
                })?,
            );
        }
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    if body.len() > MAX_SUBMISSION_BYTES {
        return Err(Error::ReportTooLarge {
            bytes: body.len(),
            limit: MAX_SUBMISSION_BYTES,
        });
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent(concat!("simile/", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut response = agent
        .post(&submission.endpoint)
        .header(
            "content-type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send(body)
        .map_err(|source| Error::ReportSubmission {
            endpoint: submission.endpoint.clone(),
            source,
        })?;
    read_response_json::<SubmissionResponse>(&mut response)
}

fn read_response_json<T: for<'de> Deserialize<'de>>(
    response: &mut ureq::http::Response<ureq::Body>,
) -> Result<T> {
    let mut reader = response.body_mut().as_reader();
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| Error::ReportSubmissionResponse(source.to_string()))?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(Error::ReportSubmissionResponse(
            "response exceeded the 64 KiB limit".to_owned(),
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|source| Error::ReportSubmissionResponse(source.to_string()))
}

fn multipart_json(
    body: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    value: &impl Serialize,
) -> Result<()> {
    let json = serde_json::to_vec(value)
        .map_err(|source| Error::ReportSubmissionResponse(source.to_string()))?;
    multipart_file(body, boundary, name, "application/json", &json);
    Ok(())
}

fn multipart_file(
    body: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    content_type: &str,
    bytes: &[u8],
) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{name}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
}

fn mask_contact(contact: &str) -> String {
    let Some((local, domain)) = contact.split_once('@') else {
        return "***".to_owned();
    };
    let first = local.chars().next().unwrap_or('*');
    format!("{first}***@{domain}")
}

impl std::fmt::Display for crate::registry::SourceLocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path { value } | Self::Url { value } => formatter.write_str(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::sync::mpsc;

    #[test]
    fn submission_keeps_contact_out_of_the_technical_report_part() {
        let root = std::env::temp_dir().join(format!(
            "simile-report-submit-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(root.join("case.simile-repro"))
            .unwrap_or_else(|error| panic!("test case directory must exist: {error}"));
        fs::write(root.join("case.simile-repro/bundle.json"), b"{}")
            .unwrap_or_else(|error| panic!("test manifest must exist: {error}"));
        let mut report = ReportDocument::verified(
            ReportKind::EmulationInconsistency,
            ReportTrigger::User,
            "response relation mismatch",
            None,
            "inttegro",
            None,
            "0.3.0",
            "2026-11-22",
            "POST /orders",
            "abc123",
            ReportDiagnostic {
                code: "emulation_inconsistency".to_owned(),
                message: "response relation mismatch".to_owned(),
                issues: Vec::new(),
            },
            "abc123:POST /orders:42",
            true,
        )
        .unwrap_or_else(|error| panic!("test report must be valid: {error}"));
        // A retry reads the durable report after its first successful submission marked it sent.
        report.status = ReportStatus::Submitted;

        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|error| panic!("test receiver must bind: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("test receiver must have an address: {error}"));
        let (sender, receiver) = mpsc::channel();
        let report_id = report.id.clone();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener
                .accept()
                .unwrap_or_else(|error| panic!("test receiver must accept: {error}"));
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            let mut expected = None;
            loop {
                let read = stream
                    .read(&mut buffer)
                    .unwrap_or_else(|error| panic!("test receiver must read: {error}"));
                bytes.extend_from_slice(&buffer[..read]);
                if expected.is_none() {
                    if let Some(header_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n")
                    {
                        let headers = String::from_utf8_lossy(&bytes[..header_end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or_default();
                        expected = Some(header_end + 4 + length);
                    }
                }
                if read == 0 || expected.is_some_and(|expected| bytes.len() >= expected) {
                    break;
                }
            }
            sender
                .send(String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_else(|error| panic!("test receiver must return request: {error}"));
            let response = format!(r#"{{"id":"{report_id}","status":"pending_verification"}}"#);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                response.len()
            )
            .unwrap_or_else(|error| panic!("test receiver must respond: {error}"));
        });
        let response = submit(
            &SubmissionIdentity {
                endpoint: format!("http://{address}/v1/reports"),
                contact: "reporter@example.test".to_owned(),
            },
            &report,
            &root,
        )
        .unwrap_or_else(|error| panic!("report must submit: {error}"));
        assert_eq!(response.id, report.id);
        assert_eq!(response.status, "pending_verification");
        let request = receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|error| panic!("test receiver timed out: {error}"));
        let (_, body) = request
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("multipart request must have a body"));
        let report_start = body
            .find("name=\"report\"")
            .unwrap_or_else(|| panic!("report part must exist"));
        let reporter_start = body
            .find("name=\"reporter\"")
            .unwrap_or_else(|| panic!("reporter part must exist"));
        assert!(report_start < reporter_start);
        assert!(body[report_start..reporter_start].contains(r#""status":"verified""#));
        assert!(!body[report_start..reporter_start].contains(r#""status":"submitted""#));
        assert!(!body[report_start..reporter_start].contains("reporter@example.test"));
        assert!(body[reporter_start..].contains("reporter@example.test"));
        worker
            .join()
            .unwrap_or_else(|error| std::panic::resume_unwind(error));
        drop(fs::remove_dir_all(root));
    }
}
