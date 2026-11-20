use std::collections::BTreeMap;

use serde::Serialize;

use crate::ParameterLocation;

/// One authentication mechanism named by an API contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SecurityScheme {
    ApiKey {
        location: ParameterLocation,
        name: String,
    },
    Http {
        scheme: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        bearer_format: Option<String>,
    },
    OAuth2,
    OpenIdConnect,
    MutualTls,
}

/// One OpenAPI security-requirement alternative.
///
/// Entries inside `schemes` are conjunctive. Multiple requirements on an
/// operation are alternatives, preserving OpenAPI's `OR` of `AND` model.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SecurityRequirement {
    pub schemes: BTreeMap<String, Vec<String>>,
}
