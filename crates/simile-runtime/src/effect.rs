use std::collections::BTreeMap;

use serde_json::Value;
use simile_artifact::Artifact;
use simile_spec::{EffectAction, EffectActionKind, EffectValue, ResourceRef};

use crate::{Error, OperationOutcome, Result};

/// A rendered resource command ready for the Stack adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedAction {
    resource: ResourceRef,
    kind: EffectActionKind,
    arguments: BTreeMap<String, Value>,
}

impl PlannedAction {
    #[must_use]
    pub const fn resource(&self) -> &ResourceRef {
        &self.resource
    }

    #[must_use]
    pub const fn kind(&self) -> EffectActionKind {
        self.kind
    }

    #[must_use]
    pub const fn arguments(&self) -> &BTreeMap<String, Value> {
        &self.arguments
    }
}

/// One named asynchronous unit and its fully rendered commands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedAsyncOperation {
    name: String,
    actions: Vec<PlannedAction>,
}

impl PlannedAsyncOperation {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn actions(&self) -> &[PlannedAction] {
        &self.actions
    }
}

/// Consequences of one newly committed operation result.
///
/// Synchronous actions run before response delivery. Async operations run
/// outside the response-critical path and therefore never add latency to the
/// immediate response.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EffectPlan {
    synchronous: Vec<PlannedAction>,
    asynchronous: Vec<PlannedAsyncOperation>,
}

impl EffectPlan {
    #[must_use]
    pub fn synchronous(&self) -> &[PlannedAction] {
        &self.synchronous
    }

    #[must_use]
    pub fn asynchronous(&self) -> &[PlannedAsyncOperation] {
        &self.asynchronous
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.synchronous.is_empty() && self.asynchronous.is_empty()
    }
}

pub(crate) fn plan(
    artifact: &Artifact,
    operation: &str,
    outcome: &OperationOutcome,
) -> Result<EffectPlan> {
    let request = outcome.request_body().map(crate::ValidatedBody::value);
    let response = outcome.response().value();
    let mut plan = EffectPlan::default();

    for effect in artifact.effects_for(operation) {
        let rendered = (|| {
            let synchronous = effect
                .actions()
                .iter()
                .map(|action| render_action(action, request, response))
                .collect::<Result<Vec<_>>>()?;
            let asynchronous = effect
                .async_operations()
                .iter()
                .map(|name| {
                    let operation = artifact
                        .async_operations()
                        .find(|operation| operation.name() == name)
                        .ok_or_else(|| Error::EffectAsyncMissing {
                            effect: effect.name().to_owned(),
                            operation: name.clone(),
                        })?;
                    let actions = operation
                        .actions()
                        .iter()
                        .map(|action| render_action(action, request, response))
                        .collect::<Result<Vec<_>>>()?;
                    Ok(PlannedAsyncOperation {
                        name: name.clone(),
                        actions,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((synchronous, asynchronous))
        })();

        match rendered {
            Ok((synchronous, asynchronous)) => {
                plan.synchronous.extend(synchronous);
                plan.asynchronous.extend(asynchronous);
            }
            // Projections are also applicability guards. An effect that writes
            // `response.id` belongs to outcomes that actually return an id; an
            // alternate error or accepted response must not fail merely because
            // it does not contain that entity.
            Err(Error::EffectProjectionMissing { .. }) => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(plan)
}

fn render_action(
    action: &EffectAction,
    request: Option<&Value>,
    response: Option<&Value>,
) -> Result<PlannedAction> {
    Ok(PlannedAction {
        resource: action.resource.clone(),
        kind: action.kind,
        arguments: action
            .arguments
            .iter()
            .map(|(name, value)| Ok((name.clone(), render_value(value, request, response, name)?)))
            .collect::<Result<BTreeMap<_, _>>>()?,
    })
}

fn render_value(
    value: &EffectValue,
    request: Option<&Value>,
    response: Option<&Value>,
    argument: &str,
) -> Result<Value> {
    match value {
        EffectValue::Literal { value } => Ok(value.clone()),
        EffectValue::Request { pointer } => project(request, "request", pointer, argument),
        EffectValue::Response { pointer } => project(response, "response", pointer, argument),
        EffectValue::Array { items } => Ok(Value::Array(
            items
                .iter()
                .map(|item| render_value(item, request, response, argument))
                .collect::<Result<Vec<_>>>()?,
        )),
        EffectValue::Object { fields } => Ok(Value::Object(
            fields
                .iter()
                .map(|(name, value)| {
                    Ok((
                        name.clone(),
                        render_value(value, request, response, argument)?,
                    ))
                })
                .collect::<Result<serde_json::Map<_, _>>>()?,
        )),
    }
}

fn project(
    document: Option<&Value>,
    document_name: &'static str,
    pointer: &str,
    argument: &str,
) -> Result<Value> {
    let document = document.ok_or_else(|| Error::EffectProjectionMissing {
        document: document_name,
        pointer: pointer.to_owned(),
        argument: argument.to_owned(),
    })?;
    if pointer.is_empty() {
        return Ok(document.clone());
    }
    document
        .pointer(pointer)
        .cloned()
        .ok_or_else(|| Error::EffectProjectionMissing {
            document: document_name,
            pointer: pointer.to_owned(),
            argument: argument.to_owned(),
        })
}
