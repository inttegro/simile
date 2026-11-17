use serde_json::Value;
use simile_spec::{FaultAction, FaultPhase, OutcomeCondition, ResourceKind, ResourceRef};

#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub name: &'a str,
    pub contents: &'a str,
}

impl<'a> Source<'a> {
    #[must_use]
    pub const fn new(name: &'a str, contents: &'a str) -> Self {
        Self { name, contents }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDeclaration {
    pub kind: ResourceKind,
    pub name: String,
}

impl ResourceDeclaration {
    #[must_use]
    pub fn reference(&self) -> ResourceRef {
        ResourceRef::new(self.kind, &self.name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BehaviorDeclaration {
    pub operation_id: String,
    pub response_status: Option<u16>,
    pub error_sets: Vec<String>,
    pub outcomes: Vec<OutcomeDeclaration>,
    pub triggers: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomeDeclaration {
    pub name: String,
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Option<Value>,
    pub when: Vec<OutcomeCondition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorSetDeclaration {
    pub name: String,
    pub outcomes: Vec<OutcomeDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorSetApplicationDeclaration {
    pub error_set: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolErrorDeclaration {
    pub cause: String,
    pub statuses: Vec<u16>,
    pub content_type: Option<String>,
    pub body: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioDeclaration {
    pub name: String,
    pub selections: Vec<ScenarioSelectionDeclaration>,
    pub faults: Vec<ScenarioFaultDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioSelectionDeclaration {
    pub operation_id: String,
    pub outcome: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioFaultDeclaration {
    pub operation_id: String,
    pub fault: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaultDeclaration {
    pub name: String,
    pub phase: FaultPhase,
    pub action: FaultAction,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaultProfileDeclaration {
    pub name: String,
    pub probability_ppm: u32,
    pub faults: Vec<String>,
    pub operations: Vec<String>,
    pub max_consecutive: u32,
    pub cooldown_requests: u32,
    pub seed: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsyncDeclaration {
    pub name: String,
    pub uses: Vec<ResourceRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Module {
    pub source: String,
    pub resources: Vec<ResourceDeclaration>,
    pub behaviors: Vec<BehaviorDeclaration>,
    pub error_sets: Vec<ErrorSetDeclaration>,
    pub error_set_applications: Vec<ErrorSetApplicationDeclaration>,
    pub protocol_errors: Vec<ProtocolErrorDeclaration>,
    pub scenarios: Vec<ScenarioDeclaration>,
    pub faults: Vec<FaultDeclaration>,
    pub fault_profiles: Vec<FaultProfileDeclaration>,
    pub async_operations: Vec<AsyncDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceDocument {
    pub service: ServiceDeclaration,
    pub inline_behavior: Module,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceDeclaration {
    pub name: String,
    pub listen: ListenDeclaration,
    pub openapi: OpenApiDeclaration,
    pub behaviors: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListenDeclaration {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenApiDeclaration {
    Path(String),
    Url(String),
}
