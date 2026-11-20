use serde::Serialize;

use crate::{EffectValue, HttpMethod, RequestBodyContract};

/// Where a webhook contract entered the compiled service model.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookOrigin {
    OpenApiCallback,
    OpenApiWebhook,
    ProviderAuthored,
}

/// One outbound HTTP message the emulated service may emit.
///
/// A contract describes the wire message, not whether a particular operation
/// emits it. [`WebhookEmission`] attaches occurrences to committed effects.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookContract {
    pub name: String,
    pub method: HttpMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// OpenAPI callback runtime expression used when an effect does not
    /// override the destination with `to`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestBodyContract>,
    pub origin: WebhookOrigin,
}

/// One named webhook occurrence caused by a committed effect.
///
/// `after` contains hard causal dependencies within the same effect. Delivery
/// policy may reorder independent occurrences but can never violate these
/// edges.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookEmission {
    pub webhook: String,
    pub body: EffectValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<EffectValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub delay_milliseconds: u64,
}

const fn is_zero(value: &u64) -> bool {
    *value == 0
}
