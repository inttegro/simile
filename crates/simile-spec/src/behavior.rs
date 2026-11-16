use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::Serialize;
use simile_heuristics::Policy;

use crate::{ApiContract, Operation, OperationKey};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Db,
    Cache,
    Queue,
    Mail,
    ObjectStore,
    EventBus,
    Scheduler,
    Http,
}

impl ResourceKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Db => "db",
            Self::Cache => "cache",
            Self::Queue => "queue",
            Self::Mail => "mail",
            Self::ObjectStore => "object_store",
            Self::EventBus => "event_bus",
            Self::Scheduler => "scheduler",
            Self::Http => "http",
        }
    }
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ResourceKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "db" => Ok(Self::Db),
            "cache" => Ok(Self::Cache),
            "queue" => Ok(Self::Queue),
            "mail" => Ok(Self::Mail),
            "object_store" => Ok(Self::ObjectStore),
            "event_bus" => Ok(Self::EventBus),
            "scheduler" => Ok(Self::Scheduler),
            "http" => Ok(Self::Http),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ResourceRef(String);

impl ResourceRef {
    #[must_use]
    pub fn new(kind: ResourceKind, name: &str) -> Self {
        Self(format!("{}.{}", kind.as_str(), name))
    }

    #[must_use]
    pub fn parse(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Resource {
    pub kind: ResourceKind,
    pub name: String,
}

impl Resource {
    #[must_use]
    pub fn reference(&self) -> ResourceRef {
        ResourceRef::new(self.kind, &self.name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Behavior {
    pub operation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_status: Option<u16>,
    pub triggers: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AsyncOperation {
    pub name: String,
    pub uses: Vec<ResourceRef>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorSpec {
    pub resources: BTreeMap<ResourceRef, Resource>,
    pub behaviors: BTreeMap<String, Behavior>,
    pub async_operations: BTreeMap<String, AsyncOperation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorOrigin {
    OpenApi,
    Hcl,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledOperation {
    pub contract: Operation,
    pub origin: BehaviorOrigin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavior: Option<Behavior>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledService {
    pub contract: ApiContract,
    pub heuristic_policy: Policy,
    pub operations: BTreeMap<OperationKey, CompiledOperation>,
    pub resources: BTreeMap<ResourceRef, Resource>,
    pub async_operations: BTreeMap<String, AsyncOperation>,
}
