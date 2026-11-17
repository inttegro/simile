use std::collections::BTreeMap;

use simile_spec::{
    ApiContract, AsyncOperation, Behavior, BehaviorSpec, ErrorSet, ErrorSetApplication, Outcome,
    OutcomeProvenance, ProtocolError, Resource, Scenario,
};

use crate::{Error, Module, Result};

pub fn compile(
    modules: impl IntoIterator<Item = Module>,
    api: &ApiContract,
) -> Result<BehaviorSpec> {
    let mut spec = BehaviorSpec::default();
    install_builtin_protocol_errors(&mut spec);

    for module in modules {
        collect_resources(&mut spec, module.resources)?;
        collect_async_operations(&mut spec, module.async_operations)?;
        collect_error_sets(&mut spec, module.error_sets)?;
        collect_error_set_applications(&mut spec, module.error_set_applications);
        collect_protocol_errors(&mut spec, module.protocol_errors)?;
        collect_behaviors(&mut spec, module.behaviors)?;
        collect_scenarios(&mut spec, module.scenarios)?;
    }

    validate_resources(&spec)?;
    validate_behaviors(&spec, api)?;
    validate_scenarios(&spec, api)?;
    Ok(spec)
}

fn collect_error_set_applications(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::ErrorSetApplicationDeclaration>,
) {
    spec.error_set_applications
        .extend(
            declarations
                .into_iter()
                .map(|declaration| ErrorSetApplication {
                    error_set: declaration.error_set,
                    tags: declaration.tags,
                }),
        );
}

fn install_builtin_protocol_errors(spec: &mut BehaviorSpec) {
    for (cause, statuses) in [
        ("malformed_request", vec![400]),
        ("invalid_body", vec![422, 400]),
        ("unsupported_media_type", vec![415, 400]),
        ("authentication_required", vec![401]),
        ("permission_denied", vec![403]),
        ("idempotency_conflict", vec![409]),
        ("rate_limited", vec![429]),
    ] {
        spec.protocol_errors.insert(
            cause.to_owned(),
            ProtocolError {
                cause: cause.to_owned(),
                statuses,
                content_type: None,
                body: None,
                provenance: OutcomeProvenance::OpenApiDerived,
            },
        );
    }
}

fn collect_resources(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::ResourceDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let reference = declaration.reference();
        let resource = Resource {
            kind: declaration.kind,
            name: declaration.name,
        };
        if spec.resources.insert(reference.clone(), resource).is_some() {
            return Err(Error::DuplicateResource {
                resource: reference.to_string(),
            });
        }
    }
    Ok(())
}

fn collect_async_operations(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::AsyncDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let name = declaration.name;
        let operation = AsyncOperation {
            name: name.clone(),
            uses: declaration.uses,
        };
        if spec
            .async_operations
            .insert(name.clone(), operation)
            .is_some()
        {
            return Err(Error::DuplicateAsyncOperation { name });
        }
    }
    Ok(())
}

fn collect_behaviors(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::BehaviorDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let operation_id = declaration.operation_id;
        let behavior = Behavior {
            operation_id: operation_id.clone(),
            response_status: declaration.response_status,
            error_sets: declaration.error_sets,
            outcomes: declaration
                .outcomes
                .into_iter()
                .map(provider_outcome)
                .collect(),
            triggers: declaration.triggers,
        };
        if spec
            .behaviors
            .insert(operation_id.clone(), behavior)
            .is_some()
        {
            return Err(Error::DuplicateBehavior { operation_id });
        }
    }
    Ok(())
}

fn collect_error_sets(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::ErrorSetDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let name = declaration.name;
        let set = ErrorSet {
            name: name.clone(),
            outcomes: declaration
                .outcomes
                .into_iter()
                .map(provider_outcome)
                .collect(),
        };
        if spec.error_sets.insert(name.clone(), set).is_some() {
            return Err(Error::DuplicateErrorSet { name });
        }
    }
    Ok(())
}

fn provider_outcome(declaration: crate::OutcomeDeclaration) -> Outcome {
    Outcome {
        name: declaration.name,
        status: declaration.status,
        content_type: declaration.content_type,
        body: declaration.body,
        provenance: OutcomeProvenance::ProviderAuthored,
        when: declaration.when,
    }
}

fn collect_protocol_errors(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::ProtocolErrorDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let cause = declaration.cause;
        spec.protocol_errors.insert(
            cause.clone(),
            ProtocolError {
                cause,
                statuses: declaration.statuses,
                content_type: declaration.content_type,
                body: declaration.body,
                provenance: OutcomeProvenance::ProviderAuthored,
            },
        );
    }
    Ok(())
}

fn collect_scenarios(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::ScenarioDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let name = declaration.name;
        let selections = declaration
            .selections
            .into_iter()
            .map(|selection| (selection.operation_id, selection.outcome))
            .collect::<BTreeMap<_, _>>();
        let scenario = Scenario {
            name: name.clone(),
            selections,
        };
        if spec.scenarios.insert(name.clone(), scenario).is_some() {
            return Err(Error::DuplicateScenario { name });
        }
    }
    Ok(())
}

fn validate_resources(spec: &BehaviorSpec) -> Result<()> {
    for operation in spec.async_operations.values() {
        for resource in &operation.uses {
            if !spec.resources.contains_key(resource) {
                return Err(Error::UnknownResource {
                    operation: operation.name.clone(),
                    resource: resource.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn validate_behaviors(spec: &BehaviorSpec, api: &ApiContract) -> Result<()> {
    for application in &spec.error_set_applications {
        let error_set = spec.error_sets.get(&application.error_set).ok_or_else(|| {
            Error::UnknownAppliedErrorSet {
                name: application.error_set.clone(),
            }
        })?;
        for tag in &application.tags {
            if !api
                .operations
                .values()
                .any(|operation| operation.tags.contains(tag))
            {
                return Err(Error::UnknownOpenApiTag { tag: tag.clone() });
            }
        }
        for operation in api.operations.values().filter(|operation| {
            application
                .tags
                .iter()
                .any(|tag| operation.tags.contains(tag))
        }) {
            validate_outcomes(
                operation,
                operation
                    .operation_id
                    .as_deref()
                    .unwrap_or_else(|| operation.key.as_str()),
                &error_set.outcomes,
            )?;
        }
    }
    for behavior in spec.behaviors.values() {
        let operation =
            api.operation_by_id(&behavior.operation_id)
                .ok_or_else(|| Error::UnknownOperation {
                    operation_id: behavior.operation_id.clone(),
                })?;
        if let Some(status) = behavior.response_status {
            if !operation.declares_status(status) {
                return Err(Error::UndeclaredResponse {
                    operation_id: behavior.operation_id.clone(),
                    status,
                });
            }
        }
        for set in &behavior.error_sets {
            let error_set = spec
                .error_sets
                .get(set)
                .ok_or_else(|| Error::UnknownErrorSet {
                    operation_id: behavior.operation_id.clone(),
                    name: set.clone(),
                })?;
            validate_outcomes(operation, &behavior.operation_id, &error_set.outcomes)?;
        }
        validate_outcomes(operation, &behavior.operation_id, &behavior.outcomes)?;
        for target in &behavior.triggers {
            if !spec.async_operations.contains_key(target) {
                return Err(Error::UnknownAsyncTarget {
                    operation_id: behavior.operation_id.clone(),
                    target: target.clone(),
                });
            }
        }
    }
    Ok(())
}

fn validate_outcomes(
    operation: &simile_spec::Operation,
    operation_id: &str,
    outcomes: &[Outcome],
) -> Result<()> {
    for outcome in outcomes {
        if !operation.declares_status(outcome.status) {
            return Err(Error::UndeclaredOutcomeResponse {
                operation_id: operation_id.to_owned(),
                outcome: outcome.name.clone(),
                status: outcome.status,
            });
        }
    }
    Ok(())
}

fn validate_scenarios(spec: &BehaviorSpec, api: &ApiContract) -> Result<()> {
    for scenario in spec.scenarios.values() {
        for operation_id in scenario.selections.keys() {
            if api.operation_by_id(operation_id).is_none() {
                return Err(Error::UnknownScenarioOperation {
                    scenario: scenario.name.clone(),
                    operation_id: operation_id.clone(),
                });
            }
        }
    }
    Ok(())
}
