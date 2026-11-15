use simile_artifact::Artifact;
use simile_spec::ResourceRef;

use crate::{
    IncomingBody, ResponseSynthesisRequest, Result, SynthesizedResponse, ValidatedBody,
    synthesize_response, validate_request_body,
};

/// Executes compiled service operations without depending on a transport framework.
#[derive(Clone, Copy, Debug)]
pub struct Runtime<'a> {
    artifact: &'a Artifact,
}

/// One operation invocation selected by a transport or another runtime client.
#[derive(Clone, Copy, Debug)]
pub struct OperationCall<'a> {
    pub operation: &'a str,
    pub content_type: Option<&'a str>,
    pub body: &'a [u8],
}

/// Optional controls for scenarios that need a specific declared response.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExecutionOptions<'a> {
    pub response_status: Option<u16>,
    pub response_content_type: Option<&'a str>,
}

/// The validated request, immediate response, and declared follow-up work for an operation.
#[derive(Clone, Debug, PartialEq)]
pub struct OperationOutcome {
    operation: String,
    request_body: Option<ValidatedBody>,
    response: SynthesizedResponse,
    async_operations: Vec<PlannedAsyncOperation>,
}

/// One asynchronous operation declared by the selected behavior.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedAsyncOperation {
    name: String,
    resources: Vec<ResourceRef>,
}

impl<'a> Runtime<'a> {
    #[must_use]
    pub const fn new(artifact: &'a Artifact) -> Self {
        Self { artifact }
    }

    /// Execute an operation using the response selected by its compiled contract and behavior.
    pub fn execute(&self, call: OperationCall<'_>) -> Result<OperationOutcome> {
        self.execute_with_options(call, ExecutionOptions::default())
    }

    /// Execute an operation with explicit response controls for a test scenario.
    pub fn execute_with_options(
        &self,
        call: OperationCall<'_>,
        options: ExecutionOptions<'_>,
    ) -> Result<OperationOutcome> {
        let operation = self.artifact.operation(call.operation).ok_or_else(|| {
            crate::Error::UnknownOperation {
                operation: call.operation.to_owned(),
            }
        })?;
        let request_body = validate_request_body(
            self.artifact,
            IncomingBody {
                operation: call.operation,
                content_type: call.content_type,
                bytes: call.body,
            },
        )?;
        let response = synthesize_response(
            self.artifact,
            ResponseSynthesisRequest {
                operation: call.operation,
                status: options.response_status,
                content_type: options.response_content_type,
                request_body: request_body.as_ref(),
            },
        )?;
        let async_operations = operation
            .triggers()
            .map(|trigger| PlannedAsyncOperation {
                name: trigger.name().to_owned(),
                resources: trigger
                    .resources()
                    .map(|resource| resource.reference())
                    .collect(),
            })
            .collect();

        Ok(OperationOutcome {
            operation: call.operation.to_owned(),
            request_body,
            response,
            async_operations,
        })
    }
}

impl OperationOutcome {
    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }

    #[must_use]
    pub const fn request_body(&self) -> Option<&ValidatedBody> {
        self.request_body.as_ref()
    }

    #[must_use]
    pub const fn response(&self) -> &SynthesizedResponse {
        &self.response
    }

    #[must_use]
    pub fn async_operations(&self) -> &[PlannedAsyncOperation] {
        &self.async_operations
    }

    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        String,
        Option<ValidatedBody>,
        SynthesizedResponse,
        Vec<PlannedAsyncOperation>,
    ) {
        (
            self.operation,
            self.request_body,
            self.response,
            self.async_operations,
        )
    }
}

impl PlannedAsyncOperation {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn resources(&self) -> &[ResourceRef] {
        &self.resources
    }
}
