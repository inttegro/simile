#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use simile_heuristics::Policy;
use simile_spec::{
    Behavior, BehaviorOrigin, CompiledOperation, CompiledService, Operation, Outcome,
    OutcomeProvenance, ResponseStatus,
};
use thiserror::Error;

#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub name: &'a str,
    pub contents: &'a str,
}

impl<'a> Source<'a> {
    #[must_use]
    pub const fn new(name: &'a str, contents: &'a str) -> Self {
        Self { name, contents }
    }
}

#[derive(Debug)]
pub struct CompileRequest<'a> {
    pub openapi: Source<'a>,
    pub behaviors: Vec<Source<'a>>,
    pub heuristic_policy: Policy,
}

#[derive(Debug)]
pub struct ParsedCompileRequest<'a> {
    pub openapi: Source<'a>,
    pub behaviors: Vec<simile_hcl::Module>,
    pub heuristic_policy: Policy,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Compiler;

impl Compiler {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn compile(&self, request: CompileRequest<'_>) -> Result<CompiledService> {
        let modules = request
            .behaviors
            .into_iter()
            .map(|source| simile_hcl::parse(simile_hcl::Source::new(source.name, source.contents)))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.compile_parsed(ParsedCompileRequest {
            openapi: request.openapi,
            behaviors: modules,
            heuristic_policy: request.heuristic_policy,
        })
    }

    pub fn compile_parsed(&self, request: ParsedCompileRequest<'_>) -> Result<CompiledService> {
        let contract =
            simile_openapi::parse(request.openapi.contents).map_err(|source| Error::OpenApi {
                name: request.openapi.name.to_owned(),
                source,
            })?;
        let behaviors = simile_hcl::compile(request.behaviors, &contract)?;

        let operations = contract
            .operations
            .iter()
            .map(|(key, operation)| {
                let behavior = operation
                    .operation_id
                    .as_ref()
                    .and_then(|id| behaviors.behaviors.get(id))
                    .cloned();
                let origin = if behavior.is_some() {
                    BehaviorOrigin::Hcl
                } else {
                    BehaviorOrigin::OpenApi
                };
                let outcomes = compile_outcomes(operation, behavior.as_ref(), &behaviors)?;
                Ok((
                    key.clone(),
                    CompiledOperation {
                        contract: operation.clone(),
                        origin,
                        behavior,
                        outcomes,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        validate_scenario_outcomes(&operations, &behaviors.scenarios)?;

        Ok(CompiledService {
            contract,
            heuristic_policy: request.heuristic_policy,
            operations,
            protocol_errors: behaviors.protocol_errors,
            scenarios: behaviors.scenarios,
            faults: behaviors.faults,
            fault_profiles: behaviors.fault_profiles,
            resources: behaviors.resources,
            async_operations: behaviors.async_operations,
        })
    }
}

fn validate_scenario_outcomes(
    operations: &BTreeMap<simile_spec::OperationKey, CompiledOperation>,
    scenarios: &BTreeMap<String, simile_spec::Scenario>,
) -> Result<()> {
    for scenario in scenarios.values() {
        for (operation_id, outcome) in &scenario.selections {
            let operation = operations
                .values()
                .find(|operation| operation.contract.operation_id.as_deref() == Some(operation_id))
                .ok_or_else(|| Error::UnknownScenarioOperation {
                    scenario: scenario.name.clone(),
                    operation_id: operation_id.clone(),
                })?;
            if !operation
                .outcomes
                .iter()
                .any(|candidate| candidate.name == *outcome)
            {
                return Err(Error::UnknownScenarioOutcome {
                    scenario: scenario.name.clone(),
                    operation_id: operation_id.clone(),
                    outcome: outcome.clone(),
                });
            }
        }
    }
    Ok(())
}

fn compile_outcomes(
    operation: &Operation,
    behavior: Option<&Behavior>,
    behaviors: &simile_spec::BehaviorSpec,
) -> Result<Vec<Outcome>> {
    let mut outcomes = openapi_outcomes(operation);
    for application in &behaviors.error_set_applications {
        if application
            .tags
            .iter()
            .any(|tag| operation.tags.contains(tag))
        {
            if let Some(set) = behaviors.error_sets.get(&application.error_set) {
                for outcome in &set.outcomes {
                    upsert_outcome(&mut outcomes, outcome.clone())?;
                }
            }
        }
    }
    if let Some(behavior) = behavior {
        for set in &behavior.error_sets {
            if let Some(set) = behaviors.error_sets.get(set) {
                for outcome in &set.outcomes {
                    upsert_outcome(&mut outcomes, outcome.clone())?;
                }
            }
        }
        for outcome in &behavior.outcomes {
            upsert_outcome(&mut outcomes, outcome.clone())?;
        }
    }
    Ok(outcomes)
}

fn openapi_outcomes(operation: &Operation) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    for response in &operation.responses {
        let status = representative_status(&response.status);
        let Some(body) = &response.body else {
            outcomes.push(Outcome {
                name: status_name(&response.status),
                status,
                content_type: None,
                body: None,
                provenance: OutcomeProvenance::OpenApiDerived,
                when: Vec::new(),
            });
            continue;
        };
        let mut produced = false;
        for (content_type, media) in &body.content {
            for (name, example) in &media.examples {
                outcomes.push(Outcome {
                    name: unique_example_name(name, &response.status, content_type, &outcomes),
                    status,
                    content_type: Some(content_type.clone()),
                    body: Some(example.clone()),
                    provenance: OutcomeProvenance::OpenApiDerived,
                    when: Vec::new(),
                });
                produced = true;
            }
            if media.examples.is_empty() {
                if let Some(example) = &media.example {
                    outcomes.push(Outcome {
                        name: unique_example_name(
                            &status_name(&response.status),
                            &response.status,
                            content_type,
                            &outcomes,
                        ),
                        status,
                        content_type: Some(content_type.clone()),
                        body: Some(example.clone()),
                        provenance: OutcomeProvenance::OpenApiDerived,
                        when: Vec::new(),
                    });
                    produced = true;
                }
            }
        }
        if !produced {
            outcomes.push(Outcome {
                name: status_name(&response.status),
                status,
                content_type: body.content.keys().next().cloned(),
                body: None,
                provenance: OutcomeProvenance::SchemaSynthesized,
                when: Vec::new(),
            });
        }
    }
    outcomes
}

fn representative_status(status: &ResponseStatus) -> u16 {
    match status {
        ResponseStatus::Exact(status) => *status,
        ResponseStatus::Range(hundreds) => hundreds * 100,
        ResponseStatus::Default => 500,
    }
}

fn status_name(status: &ResponseStatus) -> String {
    match status {
        ResponseStatus::Exact(status) => format!("http_{status}"),
        ResponseStatus::Range(hundreds) => format!("http_{hundreds}xx"),
        ResponseStatus::Default => "default".to_owned(),
    }
}

fn unique_example_name(
    candidate: &str,
    status: &ResponseStatus,
    content_type: &str,
    outcomes: &[Outcome],
) -> String {
    let candidate = public_name(candidate);
    if !outcomes.iter().any(|outcome| outcome.name == candidate) {
        return candidate;
    }
    let media = public_name(content_type);
    format!("{}_{}_{}", status_name(status), media, candidate)
}

fn public_name(value: &str) -> String {
    let mut name = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    while name.contains("__") {
        name = name.replace("__", "_");
    }
    name.trim_matches('_').to_owned()
}

fn upsert_outcome(outcomes: &mut Vec<Outcome>, outcome: Outcome) -> Result<()> {
    if let Some(existing) = outcomes
        .iter_mut()
        .find(|existing| existing.name == outcome.name)
    {
        if existing.provenance == OutcomeProvenance::ProviderAuthored {
            if existing == &outcome {
                return Ok(());
            }
            return Err(Error::DuplicateOutcome { name: outcome.name });
        }
        *existing = outcome;
    } else {
        outcomes.push(outcome);
    }
    Ok(())
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not compile OpenAPI source `{name}`: {source}")]
    OpenApi {
        name: String,
        #[source]
        source: simile_openapi::Error,
    },

    #[error(transparent)]
    Behavior(#[from] simile_hcl::Error),

    #[error("provider-authored outcome `{name}` is declared more than once for one operation")]
    DuplicateOutcome { name: String },

    #[error("scenario `{scenario}` selects unknown operationId `{operation_id}`")]
    UnknownScenarioOperation {
        scenario: String,
        operation_id: String,
    },

    #[error(
        "scenario `{scenario}` selects unknown outcome `{outcome}` for operationId `{operation_id}`"
    )]
    UnknownScenarioOutcome {
        scenario: String,
        operation_id: String,
        outcome: String,
    },
}
