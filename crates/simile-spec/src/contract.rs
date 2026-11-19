use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum SchemaFeature {
    #[serde(rename = "3.0")]
    V3_0,
    #[serde(rename = "3.1")]
    V3_1,
    #[serde(rename = "3.2")]
    V3_2,
}

impl fmt::Display for SchemaFeature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::V3_0 => "3.0",
            Self::V3_1 => "3.1",
            Self::V3_2 => "3.2",
        };
        formatter.write_str(value)
    }
}

/// An exact source-contract release and the schema feature line that determines its semantics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaVersion {
    pub exact: String,
    pub feature: SchemaFeature,
}

// Why: existing OpenAPI consumers retain their public type names while other frontends can use
// format-neutral schema terminology and remain independent of the OpenAPI adapter.
pub type OpenApiFeature = SchemaFeature;
pub type OpenApiVersion = SchemaVersion;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ServiceInfo {
    pub title: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Put,
    Post,
    Delete,
    Options,
    Head,
    Patch,
    Trace,
    Query,
    #[serde(untagged)]
    Additional(String),
}

impl HttpMethod {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Get => "GET",
            Self::Put => "PUT",
            Self::Post => "POST",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
            Self::Head => "HEAD",
            Self::Patch => "PATCH",
            Self::Trace => "TRACE",
            Self::Query => "QUERY",
            Self::Additional(method) => method,
        }
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The stable structural identity of an operation, independent of `operationId`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OperationKey(String);

impl OperationKey {
    #[must_use]
    pub fn new(method: &HttpMethod, path: &str) -> Self {
        Self(format!("{} {path}", method.as_str()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OperationKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ResponseStatus {
    Exact(u16),
    Range(u16),
    Default,
}

impl ResponseStatus {
    #[must_use]
    pub const fn matches(&self, status: u16) -> bool {
        match self {
            Self::Exact(expected) => *expected == status,
            Self::Range(hundreds) => status / 100 == *hundreds,
            Self::Default => true,
        }
    }
}

impl fmt::Display for ResponseStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(status) => write!(formatter, "{status}"),
            Self::Range(hundreds) => write!(formatter, "{hundreds}XX"),
            Self::Default => formatter.write_str("default"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResponseContract {
    pub status: ResponseStatus,
    pub definition: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<ResponseBodyContract>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MediaTypeContract {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub examples: BTreeMap<String, Value>,
    /// Public request-to-response relationships declared by the OpenAPI adapter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_relation: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RequestBodyContract {
    pub required: bool,
    pub content: BTreeMap<String, MediaTypeContract>,
}

/// The HTTP location from which an operation parameter is read.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterLocation {
    Path,
    Query,
    Header,
    Cookie,
}

/// One OpenAPI operation parameter retained by the canonical contract.
///
/// Simile keeps serialization metadata beside the schema because the wire
/// representation is part of the public contract: a valid JSON value is not
/// necessarily a valid path, query, header, or cookie encoding of that value.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterContract {
    pub name: String,
    pub location: ParameterLocation,
    pub required: bool,
    pub style: String,
    pub explode: bool,
    pub allow_reserved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResponseBodyContract {
    pub content: BTreeMap<String, MediaTypeContract>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub key: OperationKey,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub method: HttpMethod,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ParameterContract>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestBodyContract>,
    pub responses: Vec<ResponseContract>,
}

impl Operation {
    #[must_use]
    pub fn declares_status(&self, status: u16) -> bool {
        self.responses
            .iter()
            .any(|response| response.status.matches(status))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiContract {
    pub openapi: SchemaVersion,
    pub info: ServiceInfo,
    pub operations: BTreeMap<OperationKey, Operation>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub schemas: BTreeMap<String, Value>,
}

impl ApiContract {
    #[must_use]
    pub fn from_schema(
        schema: SchemaVersion,
        info: ServiceInfo,
        operations: BTreeMap<OperationKey, Operation>,
        schemas: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            openapi: schema,
            info,
            operations,
            schemas,
        }
    }

    #[must_use]
    pub fn operation_by_id(&self, operation_id: &str) -> Option<&Operation> {
        self.operations
            .values()
            .find(|operation| operation.operation_id.as_deref() == Some(operation_id))
    }
}
