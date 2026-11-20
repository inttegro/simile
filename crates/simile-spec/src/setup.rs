use std::collections::BTreeMap;

use serde::Serialize;

/// Consumer-facing onboarding declared by a service provider.
///
/// A principal setup creates one durable service profile. The selected
/// operation performs any provider-domain work (for example, creating an app),
/// collected values populate that operation, and issued values become the
/// credentials and identifiers the caller uses afterwards.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalSetup {
    /// Principal provisioned by this setup workflow.
    pub principal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub operation_id: String,
    pub collects: BTreeMap<String, SetupCollect>,
    pub issues: BTreeMap<String, SetupIssue>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupCollect {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
    pub value_type: SetupValueType,
    /// JSON Pointer within the setup operation request body.
    pub request_pointer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<SetupBinding>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupValueType {
    #[default]
    String,
    Url,
    Boolean,
    Integer,
    Json,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupIssue {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source: SetupIssueSource,
    /// Credential slot on the principal populated by this issued value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
    pub sensitive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<SetupBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SetupIssueSource {
    /// Project a value from the setup operation response body.
    Response { pointer: String },
    /// Generate a high-entropy local token, optionally with a public prefix.
    Token { prefix: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupBinding {
    WebhookTarget,
    WebhookSigningSecret,
}
