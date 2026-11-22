use serde_json::Value;
use simile_spec::{
    EffectActionKind, EffectValue, FaultAction, FaultPhase, OutcomeCondition, ResourceKind,
    ResourceRef, SetupBinding, SetupValueType,
};

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
pub struct BackendDeclaration {
    pub kind: ResourceKind,
    pub name: String,
    pub description: Option<String>,
}

impl BackendDeclaration {
    #[must_use]
    pub fn reference(&self) -> ResourceRef {
        ResourceRef::new(self.kind, &self.name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BehaviorDeclaration {
    pub operation_id: String,
    pub description: Option<String>,
    pub response_status: Option<u16>,
    pub error_sets: Vec<String>,
    pub outcomes: Vec<OutcomeDeclaration>,
    pub state_projections: Vec<StateProjectionDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateProjectionDeclaration {
    pub resource: ResourceRef,
    pub collection: String,
    pub key: String,
    pub select: String,
    pub into: String,
    pub missing_outcome: Option<String>,
    pub predicates: Vec<StateProjectionPredicateDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateProjectionPredicateDeclaration {
    Equals {
        stored: String,
        value: Value,
        otherwise: Option<String>,
    },
    EqualsRequest {
        stored: String,
        request: String,
        otherwise: Option<String>,
    },
    State {
        stored: String,
        state: simile_spec::StateProjectionValueState,
        otherwise: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomeDeclaration {
    pub name: String,
    pub description: Option<String>,
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Option<Value>,
    pub when: Vec<OutcomeCondition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorSetDeclaration {
    pub name: String,
    pub description: Option<String>,
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
    pub description: Option<String>,
    pub statuses: Vec<u16>,
    pub content_type: Option<String>,
    pub body: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioDeclaration {
    pub name: String,
    pub description: Option<String>,
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
    pub description: Option<String>,
    pub phase: FaultPhase,
    pub action: FaultAction,
}

/// One catalog fault selected by a `faults { use ... }` declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaultUseDeclaration {
    pub selector: String,
    pub exclude: bool,
    pub alias: Option<String>,
    pub latency_milliseconds: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaultProfileDeclaration {
    pub name: String,
    pub description: Option<String>,
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
    pub description: Option<String>,
    pub uses: Vec<ResourceRef>,
    pub actions: Vec<ActionDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionDeclaration {
    pub backend: ResourceRef,
    pub kind: EffectActionKind,
    pub arguments: std::collections::BTreeMap<String, EffectValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectDeclaration {
    pub name: String,
    pub description: Option<String>,
    pub operations: Vec<String>,
    pub actions: Vec<ActionDeclaration>,
    pub async_operations: Vec<String>,
    pub webhooks: Vec<WebhookEmissionDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookEmissionDeclaration {
    pub webhook: String,
    pub body: EffectValue,
    pub target: Option<EffectValue>,
    pub after: Vec<String>,
    pub delay_milliseconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookDeclaration {
    pub name: String,
    pub description: Option<String>,
    pub method: simile_spec::HttpMethod,
    pub content_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookPolicyDeclaration {
    pub ordering: simile_spec::WebhookOrdering,
    pub out_of_order_probability_ppm: Option<u32>,
    pub reorder_window_milliseconds: Option<u64>,
    pub concurrency: Option<u16>,
    pub signing: Option<simile_spec::WebhookSigning>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalDeclaration {
    pub name: String,
    pub description: Option<String>,
    pub credentials: Vec<PrincipalCredentialDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalCredentialDeclaration {
    pub scheme: String,
    pub grants: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileDeclaration {
    pub name: String,
    pub description: Option<String>,
    pub principal: String,
    pub status: simile_spec::ProfileStatus,
    pub credentials: Vec<ProfileCredentialDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileCredentialDeclaration {
    pub scheme: String,
    pub credential: simile_spec::ProfileCredential,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Module {
    pub source: String,
    pub backends: Vec<BackendDeclaration>,
    pub behaviors: Vec<BehaviorDeclaration>,
    pub error_sets: Vec<ErrorSetDeclaration>,
    pub error_set_applications: Vec<ErrorSetApplicationDeclaration>,
    pub protocol_errors: Vec<ProtocolErrorDeclaration>,
    pub scenarios: Vec<ScenarioDeclaration>,
    pub fault_uses: Vec<FaultUseDeclaration>,
    pub faults: Vec<FaultDeclaration>,
    pub fault_profiles: Vec<FaultProfileDeclaration>,
    pub async_operations: Vec<AsyncDeclaration>,
    pub effects: Vec<EffectDeclaration>,
    pub webhooks: Vec<WebhookDeclaration>,
    pub webhook_policies: Vec<WebhookPolicyDeclaration>,
    pub principals: Vec<PrincipalDeclaration>,
    pub profiles: Vec<ProfileDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceDocument {
    pub service: ServiceDeclaration,
    pub principal_setups: Vec<PrincipalSetupDeclaration>,
    pub inline_behavior: Module,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalSetupDeclaration {
    pub principal: String,
    pub description: Option<String>,
    pub operation_id: String,
    pub collects: Vec<SetupCollectDeclaration>,
    pub issues: Vec<SetupIssueDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetupCollectDeclaration {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
    pub value_type: SetupValueType,
    pub request_pointer: String,
    pub binding: Option<SetupBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetupIssueDeclaration {
    pub name: String,
    pub description: Option<String>,
    pub response_pointer: Option<String>,
    pub token_prefix: Option<String>,
    pub credential: Option<String>,
    pub sensitive: bool,
    pub binding: Option<SetupBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceDeclaration {
    pub name: String,
    pub description: Option<String>,
    pub listen: ListenDeclaration,
    pub contract: ContractDeclaration,
    pub behaviors: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListenDeclaration {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceDeclaration {
    Path(String),
    Url(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContractDeclaration {
    OpenApi(SourceDeclaration),
    Smithy(SourceDeclaration),
    TypeScript(SourceDeclaration),
}
