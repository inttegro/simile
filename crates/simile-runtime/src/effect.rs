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

/// One schema-checked outbound webhook occurrence caused by an effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedWebhook {
    name: String,
    method: String,
    content_type: Option<String>,
    target: Option<String>,
    body: Value,
    after: Vec<String>,
    delay_milliseconds: u64,
}

impl PlannedWebhook {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    #[must_use]
    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    #[must_use]
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    #[must_use]
    pub const fn body(&self) -> &Value {
        &self.body
    }

    #[must_use]
    pub fn after(&self) -> &[String] {
        &self.after
    }

    #[must_use]
    pub const fn delay_milliseconds(&self) -> u64 {
        self.delay_milliseconds
    }
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
    webhooks: Vec<PlannedWebhook>,
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
    pub fn webhooks(&self) -> &[PlannedWebhook] {
        &self.webhooks
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.synchronous.is_empty() && self.asynchronous.is_empty() && self.webhooks.is_empty()
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
            let webhooks = effect
                .webhooks()
                .iter()
                .map(|emission| render_webhook(artifact, emission, request, response))
                .collect::<Result<Vec<_>>>()?;
            Ok((synchronous, asynchronous, webhooks))
        })();

        match rendered {
            Ok((synchronous, asynchronous, webhooks)) => {
                plan.synchronous.extend(synchronous);
                plan.asynchronous.extend(asynchronous);
                plan.webhooks.extend(webhooks);
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

fn render_webhook(
    artifact: &Artifact,
    emission: &simile_spec::WebhookEmission,
    request: Option<&Value>,
    response: Option<&Value>,
) -> Result<PlannedWebhook> {
    let webhook =
        artifact
            .webhook(&emission.webhook)
            .ok_or_else(|| Error::EffectWebhookMissing {
                webhook: emission.webhook.clone(),
            })?;
    let body = render_value(&emission.body, request, response, "emit")?;
    let content_type = webhook
        .request_body()
        .and_then(|plan| plan.media_types().next().map(str::to_owned));
    if let (Some(plan), Some(content_type)) = (webhook.request_body(), content_type.as_deref()) {
        let selected =
            plan.select(content_type)
                .ok_or_else(|| Error::EffectWebhookMediaTypeMissing {
                    webhook: emission.webhook.clone(),
                })?;
        let issues = selected.validate(&body);
        if !issues.is_empty() {
            return Err(Error::InvalidWebhookBody {
                webhook: emission.webhook.clone(),
                issues,
            });
        }
    }
    let target = match &emission.target {
        Some(value) => Some(render_webhook_target(value, request, response)?),
        None => webhook
            .target()
            .map(|target| resolve_callback_target(target, request, response))
            .transpose()?,
    };
    Ok(PlannedWebhook {
        name: emission.webhook.clone(),
        method: webhook.method().to_owned(),
        content_type,
        target,
        body,
        after: emission.after.clone(),
        delay_milliseconds: emission.delay_milliseconds,
    })
}

fn render_webhook_target(
    value: &EffectValue,
    request: Option<&Value>,
    response: Option<&Value>,
) -> Result<String> {
    render_value(value, request, response, "to")?
        .as_str()
        .filter(|target| !target.trim().is_empty())
        .map(str::to_owned)
        .ok_or(Error::InvalidWebhookTarget)
}

fn resolve_callback_target(
    expression: &str,
    request: Option<&Value>,
    response: Option<&Value>,
) -> Result<String> {
    let expression = expression
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .unwrap_or(expression);
    let (document, pointer) = expression
        .strip_prefix("$request.body#")
        .map(|pointer| (request, pointer))
        .or_else(|| {
            expression
                .strip_prefix("$response.body#")
                .map(|pointer| (response, pointer))
        })
        .ok_or_else(|| Error::UnsupportedCallbackExpression {
            expression: expression.to_owned(),
        })?;
    document
        .and_then(|document| document.pointer(pointer))
        .and_then(Value::as_str)
        .filter(|target| !target.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| Error::CallbackTargetMissing {
            expression: expression.to_owned(),
        })
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
