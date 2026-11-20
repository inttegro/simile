use simile_artifact::{Artifact, ArtifactOutcome};

use crate::{
    Error, ExecutionOptions, OperationOutcome, ResponseSynthesisRequest, Result, ValidatedBody,
    synthesize_response, synthesize_response_with_seed,
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
        body: selected
            .as_ref()
            .and_then(ArtifactOutcome::body)
            .or(options.response_body),
    };
    let response = match options.response_seed {
        Some(seed) => synthesize_response_with_seed(artifact, synthesis, seed),
        None => synthesize_response(artifact, synthesis),
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
