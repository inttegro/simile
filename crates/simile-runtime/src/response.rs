use serde_json::Value;
use simile_artifact::{Artifact, ArtifactOperation, ArtifactResponse};
use simile_schema::BoundResponseBody;
use simile_spec::ResponseStatus;

use crate::{Error, Result, ValidatedBody, is_json};

/// Selects an operation response and optionally supplies its validated request context.
#[derive(Clone, Copy, Debug)]
pub struct ResponseSynthesisRequest<'a> {
    pub operation: &'a str,
    pub status: Option<u16>,
    pub content_type: Option<&'a str>,
    pub request_body: Option<&'a ValidatedBody>,
    /// An exact public example selected by a named outcome.
    pub body: Option<&'a Value>,
}

/// An HTTP response synthesized and proven sound against a compiled OpenAPI contract.
#[derive(Clone, Debug, PartialEq)]
pub struct SynthesizedResponse {
    status: u16,
    media_type: Option<String>,
    bytes: Vec<u8>,
    value: Option<Value>,
}

impl SynthesizedResponse {
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    #[must_use]
    pub fn media_type(&self) -> Option<&str> {
        self.media_type.as_deref()
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn value(&self) -> Option<&Value> {
        self.value.as_ref()
    }

    #[must_use]
    pub fn into_parts(self) -> (u16, Option<String>, Vec<u8>, Option<Value>) {
        (self.status, self.media_type, self.bytes, self.value)
    }
}

/// Synthesize a fresh response, optionally projecting compatible values from its request.
pub fn synthesize_response(
    artifact: &Artifact,
    request: ResponseSynthesisRequest<'_>,
) -> Result<SynthesizedResponse> {
    synthesize_response_with_seed(artifact, request, rand::random())
}

/// Synthesize a reproducible response from a caller-provided seed.
pub fn synthesize_response_with_seed(
    artifact: &Artifact,
    request: ResponseSynthesisRequest<'_>,
    seed: u64,
) -> Result<SynthesizedResponse> {
    let operation =
        artifact
            .operation(request.operation)
            .ok_or_else(|| Error::UnknownOperation {
                operation: request.operation.to_owned(),
            })?;
    verify_request_context(request)?;
    let desired_status = request.status.or(operation.behavior_response_status());
    let (response, status) = select_response(operation, desired_status).ok_or_else(|| {
        desired_status.map_or_else(
            || Error::NoResponseStatus {
                operation: request.operation.to_owned(),
            },
            |status| Error::ResponseNotDeclared {
                operation: request.operation.to_owned(),
                status,
            },
        )
    })?;

    let Some(body) = response.body() else {
        if request.body.is_some() {
            return Err(Error::ResponseBodyNotDeclared {
                operation: request.operation.to_owned(),
                status,
            });
        }
        return Ok(SynthesizedResponse {
            status,
            media_type: None,
            bytes: Vec::new(),
            value: None,
        });
    };
    synthesize_body(request, status, body, seed)
}

fn verify_request_context(request: ResponseSynthesisRequest<'_>) -> Result<()> {
    let Some(request_body) = request.request_body else {
        return Ok(());
    };
    if request_body.operation() == request.operation {
        return Ok(());
    }
    Err(Error::RequestContextMismatch {
        request_operation: request_body.operation().to_owned(),
        response_operation: request.operation.to_owned(),
    })
}

fn synthesize_body(
    request: ResponseSynthesisRequest<'_>,
    status: u16,
    body: BoundResponseBody<'_>,
    seed: u64,
) -> Result<SynthesizedResponse> {
    let content_type = request
        .content_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| preferred_json_media_type(&body));
    let content_type = content_type.ok_or_else(|| Error::NoResponseMediaType {
        operation: request.operation.to_owned(),
        expected: body.media_types().map(str::to_owned).collect(),
    })?;
    let selected =
        body.select(&content_type)
            .ok_or_else(|| Error::UnsupportedResponseMediaType {
                operation: request.operation.to_owned(),
                content_type: content_type.clone(),
                expected: body.media_types().map(str::to_owned).collect(),
            })?;
    if !is_json(selected.requested()) {
        return Err(Error::UnsupportedResponseEncoding {
            content_type: selected.requested().to_owned(),
        });
    }
    let value = if let Some(body) = request.body {
        body.clone()
    } else {
        request
            .request_body
            .map_or_else(
                || selected.synthesize_fuzz(seed),
                |request_body| selected.synthesize_fuzz_from(request_body.value(), seed),
            )
            .map_err(|source| Error::ResponseSynthesis { source })?
    };
    let issues = selected.validate(&value);
    if !issues.is_empty() {
        return Err(Error::InvalidResponseBody { issues });
    }
    let bytes = serde_json::to_vec(&value).map_err(|error| Error::EncodeResponseJson {
        message: error.to_string(),
    })?;
    Ok(SynthesizedResponse {
        status,
        media_type: Some(selected.requested().to_owned()),
        bytes,
        value: Some(value),
    })
}

fn select_response(
    operation: ArtifactOperation<'_>,
    desired_status: Option<u16>,
) -> Option<(ArtifactResponse<'_>, u16)> {
    let responses = operation.responses().collect::<Vec<_>>();
    if let Some(status) = desired_status {
        return responses
            .iter()
            .find(|response| response.status() == &ResponseStatus::Exact(status))
            .or_else(|| {
                responses.iter().find(|response| {
                    matches!(response.status(), ResponseStatus::Range(_))
                        && response.status().matches(status)
                })
            })
            .or_else(|| {
                responses
                    .iter()
                    .find(|response| response.status() == &ResponseStatus::Default)
            })
            .cloned()
            .map(|response| (response, status));
    }

    responses
        .iter()
        .filter_map(|response| match response.status() {
            ResponseStatus::Exact(status) if (200..300).contains(status) => {
                Some((response, *status))
            }
            _ => None,
        })
        .min_by_key(|(_, status)| *status)
        .or_else(|| {
            responses
                .iter()
                .find_map(|response| match response.status() {
                    ResponseStatus::Range(2) => Some((response, 200)),
                    _ => None,
                })
        })
        .or_else(|| {
            responses
                .iter()
                .filter_map(|response| match response.status() {
                    ResponseStatus::Exact(status) => Some((response, *status)),
                    _ => None,
                })
                .min_by_key(|(_, status)| *status)
        })
        .or_else(|| {
            responses
                .iter()
                .filter_map(|response| match response.status() {
                    ResponseStatus::Range(hundreds) => Some((response, hundreds * 100)),
                    _ => None,
                })
                .min_by_key(|(_, status)| *status)
        })
        .or_else(|| {
            responses
                .iter()
                .find(|response| response.status() == &ResponseStatus::Default)
                .map(|response| (response, 500))
        })
        .map(|(response, status)| (response.clone(), status))
}

fn preferred_json_media_type(plan: &BoundResponseBody<'_>) -> Option<String> {
    plan.media_types()
        .find(|media_type| is_json(media_type))
        .map(str::to_owned)
        .or_else(|| {
            plan.media_types()
                .any(|media_type| media_type == "application/*" || media_type == "*/*")
                .then(|| "application/json".to_owned())
        })
}
