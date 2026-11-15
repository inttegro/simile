use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum OpenApiFeature {
    #[serde(rename = "3.0")]
    V3_0,
    #[serde(rename = "3.1")]
    V3_1,
    #[serde(rename = "3.2")]
    V3_2,
}

impl fmt::Display for OpenApiFeature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::V3_0 => "3.0",
            Self::V3_1 => "3.1",
            Self::V3_2 => "3.2",
        };
        formatter.write_str(value)
    }
}

/// An exact OpenAPI release and the feature line that determines its semantics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiVersion {
    pub exact: String,
    pub feature: OpenApiFeature,
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body: Option<Value>,
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
    pub openapi: OpenApiVersion,
    pub info: ServiceInfo,
    pub operations: BTreeMap<OperationKey, Operation>,
}

impl ApiContract {
    #[must_use]
    pub fn operation_by_id(&self, operation_id: &str) -> Option<&Operation> {
        self.operations
            .values()
            .find(|operation| operation.operation_id.as_deref() == Some(operation_id))
    }
}
