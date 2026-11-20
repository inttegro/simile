use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The lifecycle state of one concrete service profile.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileStatus {
    #[default]
    Active,
    Revoked,
    Expired,
}

impl ProfileStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
        }
    }
}

/// One credential value held by a concrete local profile.
///
/// Values embedded in a service definition are synthetic fixtures. Runtime
/// setup may also issue them into durable profile state. Provider production
/// credentials never belong in either location.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProfileCredential {
    Secret { value: String },
    Basic { username: String, password: String },
}

/// One concrete local instance of a principal.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Profile {
    pub name: String,
    pub principal: String,
    pub status: ProfileStatus,
    pub credentials: BTreeMap<String, ProfileCredential>,
}
