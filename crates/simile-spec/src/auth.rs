use std::collections::{BTreeMap, BTreeSet};

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

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalStatus {
    #[default]
    Active,
    Revoked,
    Expired,
}

/// A test identity accepted by the emulator.
///
/// Credentials are local fixtures embedded in the compiled service. They must
/// never contain a provider's production secret.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PrincipalCredential {
    Secret { value: String },
    Basic { username: String, password: String },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Principal {
    pub name: String,
    pub status: PrincipalStatus,
    pub credentials: BTreeMap<String, PrincipalCredential>,
    pub grants: BTreeSet<String>,
}
