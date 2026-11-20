use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// One authentication mechanism available to a principal.
///
/// The map key in [`Principal::credentials`] names an API-contract security
/// scheme. Grants belong to the principal contract, so every profile of that
/// principal has the same published authorization shape.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrincipalCredential {
    pub grants: BTreeSet<String>,
}

/// One kind of caller recognized by a service.
///
/// A principal describes identity and authorization shape. It contains no
/// credential values. Concrete values live in profiles created by HCL fixtures
/// or by a `principal_setup` workflow.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Principal {
    pub name: String,
    pub credentials: BTreeMap<String, PrincipalCredential>,
}
