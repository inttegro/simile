use std::collections::{BTreeMap, BTreeSet};

use simile_spec::{
    ApiContract, AsyncOperation, Behavior, BehaviorSpec, DescriptionEntry, DescriptionKind, Effect,
    EffectAction, EffectOrigin, ErrorSet, ErrorSetApplication, Fault, FaultAction, FaultPhase,
    FaultProfile, MediaTypeContract, Outcome, OutcomeProvenance, ProtocolError,
    RequestBodyContract, Resource, Scenario, WebhookContract, WebhookEmission, WebhookOrdering,
    WebhookOrigin, WebhookPolicy,
};

use crate::{Error, Module, Result};

pub fn compile(
    modules: impl IntoIterator<Item = Module>,
    api: &ApiContract,
) -> Result<BehaviorSpec> {
    let mut spec = BehaviorSpec::default();
    let mut fault_uses = Vec::new();
    let mut webhook_policy = None;
    install_builtin_protocol_errors(&mut spec);

    for module in modules {
        collect_backends(&mut spec, module.backends)?;
        collect_async_operations(&mut spec, module.async_operations)?;
        collect_webhooks(&mut spec, module.webhooks, api)?;
        collect_effects(&mut spec, module.effects)?;
        collect_error_sets(&mut spec, module.error_sets)?;
        collect_error_set_applications(&mut spec, module.error_set_applications);
        collect_protocol_errors(&mut spec, module.protocol_errors)?;
        collect_behaviors(&mut spec, module.behaviors)?;
        fault_uses.extend(module.fault_uses);
        collect_faults(&mut spec, module.faults)?;
        collect_fault_profiles(&mut spec, module.fault_profiles)?;
        collect_scenarios(&mut spec, module.scenarios)?;
        collect_webhook_policy(&mut webhook_policy, module.webhook_policies)?;
    }

    if let Some(policy) = webhook_policy {
        spec.webhook_policy = policy;
    }

    collect_fault_uses(&mut spec, fault_uses)?;

    validate_resources(&spec)?;
    validate_effects(&spec, api)?;
    validate_behaviors(&spec, api)?;
    validate_faults(&spec, api)?;
    validate_scenarios(&spec, api)?;
    Ok(spec)
}

fn collect_webhook_policy(
    policy: &mut Option<WebhookPolicy>,
    declarations: impl IntoIterator<Item = crate::WebhookPolicyDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        if policy.is_some() {
            return Err(Error::DuplicateWebhookPolicy);
        }
        let defaults = WebhookPolicy::default();
        let strict = declaration.ordering == WebhookOrdering::Strict;
        *policy = Some(WebhookPolicy {
            ordering: declaration.ordering,
            out_of_order_probability_ppm: declaration.out_of_order_probability_ppm.unwrap_or(
                if strict {
                    0
                } else {
                    defaults.out_of_order_probability_ppm
                },
            ),
            reorder_window_milliseconds: declaration.reorder_window_milliseconds.unwrap_or(
                if strict {
                    0
                } else {
                    defaults.reorder_window_milliseconds
                },
            ),
            concurrency: declaration.concurrency.unwrap_or(defaults.concurrency),
        });
    }
    Ok(())
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

fn collect_backends(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::BackendDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let reference = declaration.reference();
        insert_description(
            spec,
            DescriptionKind::Backend,
            None,
            reference.as_str(),
            declaration.description.clone(),
        );
        let resource = Resource {
            kind: declaration.kind,
            name: declaration.name,
        };
        if spec.resources.insert(reference.clone(), resource).is_some() {
            return Err(Error::DuplicateBackend {
                backend: reference.to_string(),
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
        insert_description(
            spec,
            DescriptionKind::AsyncOperation,
            None,
            &name,
            declaration.description,
        );
        let operation = AsyncOperation {
            name: name.clone(),
            uses: {
                let mut uses = declaration.uses;
                uses.extend(
                    declaration
                        .actions
                        .iter()
                        .map(|action| action.backend.clone()),
                );
                uses.sort();
                uses.dedup();
                uses
            },
            actions: declaration
                .actions
                .into_iter()
                .map(compiled_action)
                .collect(),
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

fn collect_effects(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::EffectDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let name = declaration.name;
        insert_description(
            spec,
            DescriptionKind::Effect,
            None,
            &name,
            declaration.description,
        );
        let effect = Effect {
            name: name.clone(),
            origin: EffectOrigin::ProviderAuthored,
            operations: declaration.operations,
            actions: declaration
                .actions
                .into_iter()
                .map(compiled_action)
                .collect(),
            async_operations: declaration.async_operations,
            webhooks: declaration
                .webhooks
                .into_iter()
                .map(|emission| WebhookEmission {
                    webhook: emission.webhook,
                    body: emission.body,
                    target: emission.target,
                    after: emission.after,
                    delay_milliseconds: emission.delay_milliseconds,
                })
                .collect(),
        };
        if spec.effects.insert(name.clone(), effect).is_some() {
            return Err(Error::DuplicateEffect { name });
        }
    }
    Ok(())
}

fn collect_webhooks(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::WebhookDeclaration>,
    api: &ApiContract,
) -> Result<()> {
    for declaration in declarations {
        let name = declaration.name;
        if api.webhooks.contains_key(&name) {
            return Err(Error::ContractWebhookRedefined { name });
        }
        insert_description(
            spec,
            DescriptionKind::Webhook,
            None,
            &name,
            declaration.description.clone(),
        );
        let webhook = WebhookContract {
            name: name.clone(),
            method: declaration.method,
            operation_id: None,
            summary: declaration.description,
            target: None,
            request: Some(RequestBodyContract {
                required: true,
                content: [(
                    declaration.content_type,
                    MediaTypeContract {
                        schema: None,
                        example: None,
                        examples: BTreeMap::new(),
                        request_relation: None,
                    },
                )]
                .into_iter()
                .collect(),
            }),
            origin: WebhookOrigin::ProviderAuthored,
        };
        if spec.webhooks.insert(name.clone(), webhook).is_some() {
            return Err(Error::DuplicateWebhook { name });
        }
    }
    Ok(())
}

fn compiled_action(declaration: crate::ActionDeclaration) -> EffectAction {
    EffectAction {
        resource: declaration.backend,
        kind: declaration.kind,
        arguments: declaration.arguments,
    }
}

fn collect_behaviors(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::BehaviorDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let operation_id = declaration.operation_id;
        insert_description(
            spec,
            DescriptionKind::Behavior,
            None,
            &operation_id,
            declaration.description,
        );
        let parent = format!("behavior:{operation_id}");
        for outcome in &declaration.outcomes {
            insert_description(
                spec,
                DescriptionKind::Outcome,
                Some(&parent),
                &outcome.name,
                outcome.description.clone(),
            );
        }
        let behavior = Behavior {
            operation_id: operation_id.clone(),
            response_status: declaration.response_status,
            error_sets: declaration.error_sets,
            outcomes: declaration
                .outcomes
                .into_iter()
                .map(provider_outcome)
                .collect(),
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
        insert_description(
            spec,
            DescriptionKind::ErrorSet,
            None,
            &name,
            declaration.description,
        );
        let parent = format!("error_set:{name}");
        for outcome in &declaration.outcomes {
            insert_description(
                spec,
                DescriptionKind::Outcome,
                Some(&parent),
                &outcome.name,
                outcome.description.clone(),
            );
        }
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
        insert_description(
            spec,
            DescriptionKind::ProtocolError,
            None,
            &cause,
            declaration.description,
        );
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
        insert_description(
            spec,
            DescriptionKind::Scenario,
            None,
            &name,
            declaration.description,
        );
        let mut selections = BTreeMap::new();
        for selection in declaration.selections {
            if selections
                .insert(selection.operation_id.clone(), selection.outcome)
                .is_some()
            {
                return Err(Error::DuplicateScenarioSelection {
                    scenario: name,
                    operation_id: selection.operation_id,
                });
            }
        }
        let mut faults = BTreeMap::new();
        for fault in declaration.faults {
            if faults
                .insert(fault.operation_id.clone(), fault.fault)
                .is_some()
            {
                return Err(Error::DuplicateScenarioFault {
                    scenario: name,
                    operation_id: fault.operation_id,
                });
            }
        }
        let scenario = Scenario {
            name: name.clone(),
            selections,
            faults,
        };
        if spec.scenarios.insert(name.clone(), scenario).is_some() {
            return Err(Error::DuplicateScenario { name });
        }
    }
    Ok(())
}

fn collect_faults(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::FaultDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let name = declaration.name;
        insert_description(
            spec,
            DescriptionKind::Fault,
            None,
            &name,
            declaration.description,
        );
        let fault = Fault {
            name: name.clone(),
            phase: declaration.phase,
            action: declaration.action,
        };
        insert_fault(spec, fault)?;
    }
    Ok(())
}

fn collect_fault_uses(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::FaultUseDeclaration>,
) -> Result<()> {
    let declarations = declarations.into_iter().collect::<Vec<_>>();
    let mut exclusions = BTreeSet::new();
    for declaration in declarations
        .iter()
        .filter(|declaration| declaration.exclude)
    {
        let selected = select_catalog_faults(&declaration.selector)?;
        exclusions.extend(selected.into_iter().map(|fault| fault.name));
    }

    let mut expanded = BTreeMap::new();
    for declaration in declarations
        .into_iter()
        .filter(|declaration| !declaration.exclude)
    {
        let selected = select_catalog_faults(&declaration.selector)?;
        let configured = declaration.alias.is_some() || declaration.latency_milliseconds.is_some();
        if configured && selected.len() != 1 {
            return Err(Error::ConfiguredFaultSet {
                selector: declaration.selector,
            });
        }
        for mut fault in selected {
            if exclusions.contains(&fault.name) {
                continue;
            }
            let catalog_name = fault.name.clone();
            if let Some(alias) = &declaration.alias {
                fault.name.clone_from(alias);
            }
            apply_catalog_overrides(&mut fault, &catalog_name, declaration.latency_milliseconds)?;
            insert_description(
                spec,
                DescriptionKind::Fault,
                None,
                &fault.name,
                simile_spec::fault_catalog::description(&catalog_name).map(str::to_owned),
            );
            match expanded.get(&fault.name) {
                Some(existing) if existing == &fault => continue,
                Some(_) => {
                    return Err(Error::DuplicateFault {
                        name: fault.name.clone(),
                    });
                }
                None => {
                    expanded.insert(fault.name.clone(), fault);
                }
            }
        }
    }
    for fault in expanded.into_values() {
        insert_fault(spec, fault)?;
    }
    Ok(())
}

fn select_catalog_faults(selector: &str) -> Result<Vec<Fault>> {
    let selected = simile_spec::fault_catalog::select(selector);
    if selected.is_empty() {
        return Err(if selector.contains('*') {
            Error::UnmatchedFaultSelector {
                selector: selector.to_owned(),
            }
        } else {
            Error::UnknownCatalogFault {
                name: selector.to_owned(),
                available: simile_spec::fault_catalog::names().join(", "),
            }
        });
    }
    Ok(selected)
}

fn apply_catalog_overrides(
    fault: &mut Fault,
    catalog_name: &str,
    latency_milliseconds: Option<u64>,
) -> Result<()> {
    if let Some(milliseconds) = latency_milliseconds {
        match &mut fault.action {
            FaultAction::Delay {
                milliseconds: configured,
            } => *configured = milliseconds,
            FaultAction::Disconnect => {
                return Err(Error::UnsupportedCatalogFaultOverride {
                    fault: catalog_name.to_owned(),
                    attribute: "latency",
                });
            }
        }
    }
    Ok(())
}

fn insert_fault(spec: &mut BehaviorSpec, fault: Fault) -> Result<()> {
    let name = fault.name.clone();
    if spec.faults.insert(name.clone(), fault).is_some() {
        return Err(Error::DuplicateFault { name });
    }
    Ok(())
}

fn collect_fault_profiles(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::FaultProfileDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let name = declaration.name;
        insert_description(
            spec,
            DescriptionKind::FaultProfile,
            None,
            &name,
            declaration.description,
        );
        let profile = FaultProfile {
            name: name.clone(),
            probability_ppm: declaration.probability_ppm,
            faults: declaration.faults,
            operations: declaration.operations,
            max_consecutive: declaration.max_consecutive,
            cooldown_requests: declaration.cooldown_requests,
            seed: declaration.seed,
        };
        if spec.fault_profiles.insert(name.clone(), profile).is_some() {
            return Err(Error::DuplicateFaultProfile { name });
        }
    }
    Ok(())
}

fn insert_description(
    spec: &mut BehaviorSpec,
    kind: DescriptionKind,
    parent: Option<&str>,
    name: &str,
    text: Option<String>,
) {
    if let Some(text) = text {
        spec.descriptions.insert(DescriptionEntry {
            kind,
            parent: parent.map(str::to_owned),
            name: name.to_owned(),
            text,
        });
    }
}

fn validate_faults(spec: &BehaviorSpec, api: &ApiContract) -> Result<()> {
    for fault in spec.faults.values() {
        let compatible = matches!(
            (&fault.phase, &fault.action),
            (FaultPhase::BeforeResponse, FaultAction::Delay { .. })
                | (FaultPhase::AfterCommit, FaultAction::Disconnect)
        );
        if !compatible {
            let (phase, action) = match (&fault.phase, &fault.action) {
                (FaultPhase::BeforeResponse, FaultAction::Disconnect) => {
                    ("before_response", "disconnect")
                }
                (FaultPhase::AfterCommit, FaultAction::Delay { .. }) => ("after_commit", "delay"),
                _ => unreachable!("all current fault phase/action pairs are covered"),
            };
            return Err(Error::IncompatibleFaultPhase {
                fault: fault.name.clone(),
                action,
                phase,
            });
        }
    }
    for profile in spec.fault_profiles.values() {
        let mut fault_names = BTreeSet::new();
        for fault in &profile.faults {
            if !fault_names.insert(fault) {
                return Err(Error::DuplicateFaultProfileFault {
                    profile: profile.name.clone(),
                    fault: fault.clone(),
                });
            }
            if !spec.faults.contains_key(fault) {
                return Err(Error::UnknownFault {
                    owner: format!("fault profile `{}`", profile.name),
                    fault: fault.clone(),
                });
            }
        }
        let mut operation_ids = BTreeSet::new();
        for operation_id in &profile.operations {
            if !operation_ids.insert(operation_id) {
                return Err(Error::DuplicateFaultProfileOperation {
                    profile: profile.name.clone(),
                    operation_id: operation_id.clone(),
                });
            }
            if api.operation_by_id(operation_id).is_none() {
                return Err(Error::UnknownFaultProfileOperation {
                    profile: profile.name.clone(),
                    operation_id: operation_id.clone(),
                });
            }
        }
    }
    Ok(())
}

fn validate_resources(spec: &BehaviorSpec) -> Result<()> {
    for operation in spec.async_operations.values() {
        for resource in &operation.uses {
            if !spec.resources.contains_key(resource) {
                return Err(Error::UnknownBackend {
                    operation: operation.name.clone(),
                    backend: resource.to_string(),
                });
            }
        }
        for action in &operation.actions {
            if !spec.resources.contains_key(&action.resource) {
                return Err(Error::UnknownEffectBackend {
                    owner: format!("async operation `{}`", operation.name),
                    backend: action.resource.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn validate_effects(spec: &BehaviorSpec, api: &ApiContract) -> Result<()> {
    for effect in spec.effects.values() {
        let mut operations = BTreeSet::new();
        for operation_id in &effect.operations {
            if !operations.insert(operation_id) || api.operation_by_id(operation_id).is_none() {
                return Err(Error::UnknownEffectOperation {
                    effect: effect.name.clone(),
                    operation_id: operation_id.clone(),
                });
            }
        }
        let mut asynchronous = BTreeSet::new();
        for operation in &effect.async_operations {
            if !asynchronous.insert(operation) || !spec.async_operations.contains_key(operation) {
                return Err(Error::UnknownEffectAsync {
                    effect: effect.name.clone(),
                    operation: operation.clone(),
                });
            }
        }
        for action in &effect.actions {
            if !spec.resources.contains_key(&action.resource) {
                return Err(Error::UnknownEffectBackend {
                    owner: format!("effect `{}`", effect.name),
                    backend: action.resource.to_string(),
                });
            }
        }
        let mut emitted = BTreeSet::new();
        for emission in &effect.webhooks {
            if !emitted.insert(emission.webhook.clone())
                || (!api.webhooks.contains_key(&emission.webhook)
                    && !spec.webhooks.contains_key(&emission.webhook))
            {
                return Err(Error::UnknownEffectWebhook {
                    effect: effect.name.clone(),
                    webhook: emission.webhook.clone(),
                });
            }
            for dependency in &emission.after {
                if !effect
                    .webhooks
                    .iter()
                    .any(|candidate| &candidate.webhook == dependency)
                {
                    return Err(Error::InvalidWebhookDependency {
                        file: "compiled behavior".to_owned(),
                        webhook: emission.webhook.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }
        if webhook_dependency_cycle(&effect.webhooks) {
            return Err(Error::WebhookDependencyCycle {
                effect: effect.name.clone(),
            });
        }
    }
    Ok(())
}

fn webhook_dependency_cycle(emissions: &[WebhookEmission]) -> bool {
    fn visit(
        name: &str,
        emissions: &[WebhookEmission],
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> bool {
        if visited.contains(name) {
            return false;
        }
        if !visiting.insert(name.to_owned()) {
            return true;
        }
        let cyclic = emissions
            .iter()
            .find(|emission| emission.webhook == name)
            .is_some_and(|emission| {
                emission
                    .after
                    .iter()
                    .any(|dependency| visit(dependency, emissions, visiting, visited))
            });
        visiting.remove(name);
        visited.insert(name.to_owned());
        cyclic
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    emissions
        .iter()
        .any(|emission| visit(&emission.webhook, emissions, &mut visiting, &mut visited))
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
        for (operation_id, fault) in &scenario.faults {
            if api.operation_by_id(operation_id).is_none() {
                return Err(Error::UnknownScenarioFaultOperation {
                    scenario: scenario.name.clone(),
                    operation_id: operation_id.clone(),
                });
            }
            if !spec.faults.contains_key(fault) {
                return Err(Error::UnknownFault {
                    owner: format!("scenario `{}`", scenario.name),
                    fault: fault.clone(),
                });
            }
        }
    }
    Ok(())
}
