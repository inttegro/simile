use simile_artifact::{ArtifactOperation, ArtifactResponse};
use simile_spec::ResponseStatus;

use crate::{
    Error, ResponseSynthesisRequest, Result, Runtime, SynthesizedResponse, synthesize_response,
    synthesize_response_with_seed,
};

impl Runtime {
    /// Synthesize the first service response declared for one of `statuses`.
    ///
    /// Exact responses win over class responses such as `4XX`, and class
    /// responses win over `default`. Candidate order is retained within each
    /// tier. This lets a transport ask the emulated service to own a protocol
    /// failure without mistaking an unrelated success response for an error.
    pub fn synthesize_first_declared_response(
        &self,
        operation_key: &str,
        statuses: &[u16],
    ) -> Result<Option<SynthesizedResponse>> {
        let operation =
            self.artifact()
                .operation(operation_key)
                .ok_or_else(|| Error::UnknownOperation {
                    operation: operation_key.to_owned(),
                })?;
        let responses = operation.responses().collect::<Vec<_>>();
        let status = statuses
            .iter()
            .copied()
            .find(|status| {
                responses
                    .iter()
                    .any(|response| response.status() == &ResponseStatus::Exact(*status))
            })
            .or_else(|| {
                statuses.iter().copied().find(|status| {
                    responses.iter().any(|response| {
                        matches!(response.status(), ResponseStatus::Range(_))
                            && response.status().matches(*status)
                    })
                })
            })
            .or_else(|| {
                responses
                    .iter()
                    .any(|response| response.status() == &ResponseStatus::Default)
                    .then(|| statuses.first().copied())
                    .flatten()
            });
        let Some(status) = status else {
            return Ok(None);
        };
        synthesize_response(
            self.artifact(),
            ResponseSynthesisRequest {
                operation: operation_key,
                status: Some(status),
                content_type: None,
                request_body: None,
                request_parameters: None,
                body: None,
            },
        )
        .map(Some)
    }

    /// Render the service-declared response for a transport/runtime failure.
    pub fn synthesize_protocol_error(
        &self,
        operation_key: &str,
        cause: &str,
    ) -> Result<Option<SynthesizedResponse>> {
        self.synthesize_protocol_error_with_seed(operation_key, cause, rand::random())
    }

    /// Render a declared protocol error using the caller's deterministic seed.
    pub fn synthesize_protocol_error_with_seed(
        &self,
        operation_key: &str,
        cause: &str,
        seed: u64,
    ) -> Result<Option<SynthesizedResponse>> {
        let Some(mapping) = self.artifact().protocol_error(cause) else {
            return Ok(None);
        };
        let operation =
            self.artifact()
                .operation(operation_key)
                .ok_or_else(|| Error::UnknownOperation {
                    operation: operation_key.to_owned(),
                })?;
        let status = mapping.statuses().iter().copied().find(|status| {
            operation
                .responses()
                .any(|response| response.status().matches(*status))
        });
        let Some(status) = status else {
            return Ok(None);
        };
        // A status-only mapping cannot identify one error branch when a provider groups several
        // public failures under the same response status. It may use the response schema only when
        // that schema admits one possible body; otherwise Simile's precise diagnostic is safer.
        if mapping.body().is_none()
            && !response_has_unique_body(operation, status, mapping.content_type())
        {
            return Ok(None);
        }
        synthesize_response_with_seed(
            self.artifact(),
            ResponseSynthesisRequest {
                operation: operation_key,
                status: Some(status),
                content_type: mapping.content_type(),
                request_body: None,
                request_parameters: None,
                body: mapping.body(),
            },
            seed,
        )
        .map(Some)
    }
}

fn response_has_unique_body(
    operation: ArtifactOperation<'_>,
    status: u16,
    requested_content_type: Option<&str>,
) -> bool {
    let Some(response) = select_response(operation, status) else {
        return false;
    };
    let Some(body) = response.body() else {
        return false;
    };
    let content_type = requested_content_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            body.media_types()
                .find(|media_type| crate::is_json(media_type))
                .map(str::to_owned)
        })
        .or_else(|| {
            body.media_types()
                .find(|media_type| crate::codec::supports_response(media_type))
                .map(str::to_owned)
        });
    content_type
        .and_then(|content_type| body.select(&content_type))
        .is_some_and(|media| media.has_unique_schema_value())
}

fn select_response(operation: ArtifactOperation<'_>, status: u16) -> Option<ArtifactResponse<'_>> {
    let responses = operation.responses().collect::<Vec<_>>();
    responses
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
}
