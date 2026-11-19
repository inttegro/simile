use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use simile_heuristics::Policy;

use crate::{ApiContract, Operation, OperationKey};

/// The kind of named authoring object described by public presentation metadata.
///
/// Descriptions never participate in request matching, outcome selection, or
/// fault execution. Keeping the vocabulary explicit lets `simile describe`
/// explain a compiled service without re-reading its source HCL.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DescriptionKind {
    Service,
    Behavior,
    Outcome,
    ErrorSet,
    ProtocolError,
    Scenario,
    Resource,
    AsyncOperation,
    Fault,
    FaultProfile,
}

/// One human-readable description retained in a compiled service artifact.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptionEntry {
    pub kind: DescriptionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub name: String,
    pub text: String,
}

/// Presentation metadata for named service concepts.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DescriptionCatalog {
    entries: Vec<DescriptionEntry>,
}

impl DescriptionCatalog {
    pub fn insert(&mut self, entry: DescriptionEntry) {
        if let Some(existing) = self.entries.iter_mut().find(|existing| {
            existing.kind == entry.kind
                && existing.parent == entry.parent
                && existing.name == entry.name
        }) {
            *existing = entry;
        } else {
            self.entries.push(entry);
            self.entries.sort_by(|left, right| {
                (&left.kind, &left.parent, &left.name).cmp(&(
                    &right.kind,
                    &right.parent,
                    &right.name,
                ))
            });
        }
    }

    #[must_use]
    pub fn entries(&self) -> &[DescriptionEntry] {
        &self.entries
    }

    #[must_use]
    pub fn find(&self, kind: DescriptionKind, parent: Option<&str>, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| {
                entry.kind == kind && entry.parent.as_deref() == parent && entry.name == name
            })
            .map(|entry| entry.text.as_str())
    }
}

/// How much authority the compiled artifact has for an operation outcome.
///
/// This provenance is carried into `.similec` so `simile inspect` can distinguish
/// an exact provider-authored response from a value that is merely valid against
/// an OpenAPI schema.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeProvenance {
    OpenApiDerived,
    ProviderAuthored,
    RecordedReviewed,
    SchemaSynthesized,
}

/// A public request predicate that may select an outcome.
///
/// Predicates deliberately operate only on the request document. Internal
/// implementation state, vendor names, risk rules, and thresholds have no
/// representation in a distributable Simile artifact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeCondition {
    pub path: String,
    pub equals: Value,
}

/// One deterministic, named response an operation can produce.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub name: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    pub provenance: OutcomeProvenance,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<OutcomeCondition>,
}

/// A reusable provider-authored collection of outcomes.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ErrorSet {
    pub name: String,
    pub outcomes: Vec<Outcome>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ErrorSetApplication {
    pub error_set: String,
    pub tags: Vec<String>,
}

/// A transport/runtime failure mapped into the service's public contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolError {
    pub cause: String,
    pub statuses: Vec<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    pub provenance: OutcomeProvenance,
}

/// One opaque public scenario and the named outcomes it activates.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Scenario {
    pub name: String,
    /// `operationId -> outcome name`.
    pub selections: BTreeMap<String, String>,
    /// `operationId -> fault name`.
    ///
    /// Scenario faults are deterministic test controls. They never inspect
    /// private provider state and are distinct from outcomes: an outcome is a
    /// service result, while a fault changes how that result reaches a caller.
    pub faults: BTreeMap<String, String>,
}

/// The point in a completed operation at which an environmental fault applies.
///
/// The initial vocabulary is deliberately small. More phases can be added
/// without teaching outcome selection about transport mechanics.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultPhase {
    /// The operation has produced a response but the adapter has not sent it.
    BeforeResponse,
    /// The operation is committed; only delivery to the caller is disrupted.
    AfterCommit,
}

/// One environmental disruption Simile may apply to an operation attempt.
///
/// Domain errors are intentionally absent. A bad payment method or invalid
/// state remains a deterministic named outcome selected by a public condition
/// or scenario; stochastic activation can reference only these transport-safe
/// actions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum FaultAction {
    Delay { milliseconds: u64 },
    Disconnect,
}

/// A named environmental fault, either provider-authored or expanded from a
/// built-in catalog entry during compilation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Fault {
    pub name: String,
    pub phase: FaultPhase,
    pub action: FaultAction,
}

/// A bounded stochastic activation policy for environmental faults.
///
/// Probability is stored as parts per million to keep compiled artifacts
/// deterministic and avoid floating-point policy comparisons at runtime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaultProfile {
    pub name: String,
    pub probability_ppm: u32,
    pub faults: Vec<String>,
    /// Empty means every operation; otherwise these are OpenAPI operationIds.
    pub operations: Vec<String>,
    pub max_consecutive: u32,
    pub cooldown_requests: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub error_sets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outcomes: Vec<Outcome>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AsyncOperation {
    pub name: String,
    pub uses: Vec<ResourceRef>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorSpec {
    pub descriptions: DescriptionCatalog,
    pub resources: BTreeMap<ResourceRef, Resource>,
    pub behaviors: BTreeMap<String, Behavior>,
    pub error_sets: BTreeMap<String, ErrorSet>,
    pub error_set_applications: Vec<ErrorSetApplication>,
    pub protocol_errors: BTreeMap<String, ProtocolError>,
    pub scenarios: BTreeMap<String, Scenario>,
    pub faults: BTreeMap<String, Fault>,
    pub fault_profiles: BTreeMap<String, FaultProfile>,
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
    pub outcomes: Vec<Outcome>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledService {
    pub descriptions: DescriptionCatalog,
    pub contract: ApiContract,
    pub heuristic_policy: Policy,
    pub operations: BTreeMap<OperationKey, CompiledOperation>,
    pub protocol_errors: BTreeMap<String, ProtocolError>,
    pub scenarios: BTreeMap<String, Scenario>,
    pub faults: BTreeMap<String, Fault>,
    pub fault_profiles: BTreeMap<String, FaultProfile>,
    pub resources: BTreeMap<ResourceRef, Resource>,
    pub async_operations: BTreeMap<String, AsyncOperation>,
}
