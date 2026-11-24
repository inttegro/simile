use simile_artifact::{Artifact, ArtifactOutcome};

use crate::response::{
    synthesize_response_with_candidate_context, synthesize_response_with_context,
    synthesize_response_with_seed_and_context,
};
use crate::response_time;
use crate::{
    Error, ExecutionOptions, OperationOutcome, ResponseSynthesisRequest, Result, ValidatedBody,
};

pub(crate) fn produce(
    artifact: &Artifact,
    operation_key: &str,
    request_body: Option<ValidatedBody>,
    options: ExecutionOptions<'_>,
) -> Result<OperationOutcome> {
    let selected = select(artifact, operation_key, request_body.as_ref(), options)?;
    let response_status = selected
        .as_ref()
        .map_or(options.response_status, |outcome| Some(outcome.status()));
    let response_content_type = selected
        .as_ref()
        .map_or(options.response_content_type, |outcome| {
            outcome.content_type().or(options.response_content_type)
        });
    let synthesis = ResponseSynthesisRequest {
        operation: operation_key,
        status: response_status,
        content_type: response_content_type,
        request_body: request_body.as_ref(),
        request_parameters: options.parameters,
        body: selected.as_ref().and_then(ArtifactOutcome::body),
    };
    let selected_has_body = selected.as_ref().and_then(ArtifactOutcome::body).is_some();
    let response_time = artifact
        .operation(operation_key)
        .and_then(|operation| response_time::plan(operation, options.logical_time_milliseconds));
    let response = if selected_has_body {
        match options.response_seed {
            Some(seed) => synthesize_response_with_seed_and_context(
                artifact,
                synthesis,
                seed,
                options.response_request_body,
                response_time.as_ref(),
            ),
            None => synthesize_response_with_context(
                artifact,
                synthesis,
                options.response_request_body,
                response_time.as_ref(),
            ),
        }
    } else if let Some(candidate) = options.response_body {
        synthesize_response_with_candidate_context(
            artifact,
            synthesis,
            options.response_request_body,
            candidate,
            options.response_seed,
            response_time.as_ref(),
        )
    } else {
        match options.response_seed {
            Some(seed) => synthesize_response_with_seed_and_context(
                artifact,
                synthesis,
                seed,
                options.response_request_body,
                response_time.as_ref(),
            ),
            None => synthesize_response_with_context(
                artifact,
                synthesis,
                options.response_request_body,
                response_time.as_ref(),
            ),
        }
    }?;
    Ok(OperationOutcome::new(
        operation_key.to_owned(),
        request_body,
        response,
        selected.as_ref().map(|outcome| outcome.name().to_owned()),
        selected.as_ref().map(ArtifactOutcome::provenance),
    ))
}

fn select(
    artifact: &Artifact,
    operation_key: &str,
    request_body: Option<&ValidatedBody>,
    options: ExecutionOptions<'_>,
) -> Result<Option<ArtifactOutcome>> {
    let operation = artifact
        .operation(operation_key)
        .ok_or_else(|| Error::UnknownOperation {
            operation: operation_key.to_owned(),
        })?;
    let named = if let Some(outcome) = options.outcome {
        Some(outcome)
    } else if let Some(scenario) = options.scenario {
        if !artifact.has_scenario(scenario) {
            return Err(Error::UnknownScenario {
                scenario: scenario.to_owned(),
            });
        }
        artifact.scenario_outcome(scenario, operation_key)
    } else {
        None
    };
    if let Some(name) = named {
        return operation
            .outcomes()
            .find(|outcome| outcome.name() == name)
            .map(Some)
            .ok_or_else(|| Error::UnknownOutcome {
                operation: operation_key.to_owned(),
                outcome: name.to_owned(),
            });
    }
    let Some(body) = request_body.map(ValidatedBody::value) else {
        return Ok(None);
    };
    Ok(operation.outcomes().find(|outcome| {
        !outcome.conditions().is_empty()
            && outcome
                .conditions()
                .iter()
                .all(|condition| body.pointer(condition.path()) == Some(condition.equals()))
    }))
}
