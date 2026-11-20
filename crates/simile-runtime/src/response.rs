use serde_json::Value;
use simile_artifact::{Artifact, ArtifactOperation, ArtifactResponse};
use simile_schema::{BoundResponseBody, RequestValues};
use simile_spec::{ParameterLocation, ResponseStatus};

use crate::{Error, Result, ValidatedBody, ValidatedParameters, codec};

/// The strongest proof Simile established for a synthesized response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseSoundness {
    /// The response satisfies its standalone OpenAPI response schema.
    SchemaOnly,
    /// The response also satisfies the service's compiled request-to-response relation.
    RequestConstrained,
}

/// Selects an operation response and optionally supplies its validated request context.
#[derive(Clone, Copy, Debug)]
pub struct ResponseSynthesisRequest<'a> {
    pub operation: &'a str,
    pub status: Option<u16>,
    pub content_type: Option<&'a str>,
    pub request_body: Option<&'a ValidatedBody>,
    pub request_parameters: Option<&'a ValidatedParameters>,
    /// An exact public example selected by a named outcome.
    pub body: Option<&'a Value>,
}

/// An HTTP response synthesized and proven sound against a compiled OpenAPI contract.
#[derive(Clone, Debug, PartialEq)]
pub struct SynthesizedResponse {
    status: u16,
    headers: Vec<(String, String)>,
    media_type: Option<String>,
    bytes: Vec<u8>,
    value: Option<Value>,
    soundness: ResponseSoundness,
}

impl SynthesizedResponse {
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    pub fn headers(&self) -> impl Iterator<Item = (&str, &str)> {
        self.headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
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

    /// Return the strongest proof established for this response.
    #[must_use]
    pub const fn soundness(&self) -> ResponseSoundness {
        self.soundness
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

    let headers = synthesize_headers(&response, seed)?;
    let Some(body) = response.body() else {
        if request.body.is_some() {
            return Err(Error::ResponseBodyNotDeclared {
                operation: request.operation.to_owned(),
                status,
            });
        }
        return Ok(SynthesizedResponse {
            status,
            headers,
            media_type: None,
            bytes: Vec::new(),
            value: None,
            soundness: ResponseSoundness::SchemaOnly,
        });
    };
    synthesize_body(request, status, headers, body, seed)
}

fn synthesize_headers(response: &ArtifactResponse<'_>, seed: u64) -> Result<Vec<(String, String)>> {
    response
        .headers()
        .enumerate()
        .filter(|(_, header)| {
            !header.name().eq_ignore_ascii_case("content-type")
                && !header.name().eq_ignore_ascii_case("content-length")
        })
        .map(|(index, header)| {
            let name = header.name().to_owned();
            let value = if let Some(example) = header.example() {
                if let Some(schema) = header.schema() {
                    let issues = schema.validate(&example);
                    if !issues.is_empty() {
                        return Err(Error::InvalidResponseHeader { name, issues });
                    }
                }
                example
            } else if let Some(schema) = header.schema() {
                schema
                    .synthesize_fuzz(seed.wrapping_add(index as u64 + 1))
                    .map_err(|source| Error::ResponseHeaderSynthesis {
                        name: name.clone(),
                        source,
                    })?
            } else {
                return Ok(None);
            };
            let value = match value {
                Value::String(value) => value,
                Value::Bool(value) => value.to_string(),
                Value::Number(value) => value.to_string(),
                Value::Null | Value::Array(_) | Value::Object(_) => {
                    return Err(Error::EncodeResponseHeader { name });
                }
            };
            Ok(Some((name, value)))
        })
        .filter_map(|result| result.transpose())
        .collect()
}

fn verify_request_context(request: ResponseSynthesisRequest<'_>) -> Result<()> {
    if let Some(request_body) = request.request_body {
        if request_body.operation() != request.operation {
            return Err(Error::RequestContextMismatch {
                request_operation: request_body.operation().to_owned(),
                response_operation: request.operation.to_owned(),
            });
        }
    }
    if let Some(request_parameters) = request.request_parameters {
        if request_parameters
            .operation()
            .is_some_and(|operation| operation != request.operation)
        {
            return Err(Error::RequestContextMismatch {
                request_operation: request_parameters
                    .operation()
                    .unwrap_or_default()
                    .to_owned(),
                response_operation: request.operation.to_owned(),
            });
        }
    }
    Ok(())
}

fn synthesize_body(
    request: ResponseSynthesisRequest<'_>,
    status: u16,
    headers: Vec<(String, String)>,
    body: BoundResponseBody<'_>,
    seed: u64,
) -> Result<SynthesizedResponse> {
    let content_type = request
        .content_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| preferred_response_media_type(&body));
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
    let request_values = request_values(request.request_body, request.request_parameters);
    let soundness = if request_values.is_some() && selected.is_request_constrained() {
        ResponseSoundness::RequestConstrained
    } else {
        ResponseSoundness::SchemaOnly
    };
    let value = if let Some(body) = request.body {
        // Why: provider-authored outcomes remain exact except for relationships the provider
        // explicitly declared; this preserves fidelity while still reflecting caller-owned data.
        request_values.as_ref().map_or_else(
            || Ok(body.clone()),
            |request| selected.project_exact_from_request(body, request),
        )
    } else {
        request_values.as_ref().map_or_else(
            || selected.synthesize_fuzz_minimal(seed),
            |request| selected.synthesize_fuzz_from_request(request, seed),
        )
    }
    .map_err(|source| Error::ResponseSynthesis { source })?;
    let issues = selected.validate(&value);
    if !issues.is_empty() {
        return Err(Error::InvalidResponseBody { issues });
    }
    let (media_type, bytes) = codec::encode_response(selected.requested(), &value)?;
    Ok(SynthesizedResponse {
        status,
        headers,
        media_type: Some(media_type),
        bytes,
        value: Some(value),
        soundness,
    })
}

fn request_values<'a>(
    body: Option<&'a ValidatedBody>,
    parameters: Option<&'a ValidatedParameters>,
) -> Option<RequestValues<'a>> {
    if body.is_none() && parameters.is_none() {
        return None;
    }
    let mut values = RequestValues::new(body.map(ValidatedBody::value));
    if let Some(parameters) = parameters {
        for location in [
            ParameterLocation::Path,
            ParameterLocation::Query,
            ParameterLocation::Header,
            ParameterLocation::Cookie,
        ] {
            for (name, value) in parameters.at(location) {
                values.insert_parameter(location, name, value);
            }
        }
    }
    Some(values)
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

fn preferred_response_media_type(plan: &BoundResponseBody<'_>) -> Option<String> {
    plan.media_types()
        .find(|media_type| crate::is_json(media_type))
        .map(str::to_owned)
        .or_else(|| {
            plan.media_types()
                .find(|media_type| codec::supports_response(media_type))
                .map(str::to_owned)
        })
        .or_else(|| {
            plan.media_types()
                .any(|media_type| media_type == "application/*" || media_type == "*/*")
                .then(|| "application/json".to_owned())
        })
}
