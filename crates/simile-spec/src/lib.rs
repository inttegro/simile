#![forbid(unsafe_code)]

mod auth;
mod behavior;
mod contract;
mod effect;
pub mod fault_catalog;
mod webhook;

pub use auth::{
    Principal, PrincipalCredential, PrincipalStatus, SecurityRequirement, SecurityScheme,
};
pub use behavior::{
    AsyncOperation, Behavior, BehaviorOrigin, BehaviorSpec, CompiledOperation, CompiledService,
    DescriptionCatalog, DescriptionEntry, DescriptionKind, ErrorSet, ErrorSetApplication, Fault,
    FaultAction, FaultPhase, FaultProfile, Outcome, OutcomeCondition, OutcomeProvenance,
    ProtocolError, Resource, ResourceKind, ResourceRef, Scenario, WebhookOrdering, WebhookPolicy,
    WebhookSigning,
};
pub use contract::{
    ApiContract, HttpMethod, MediaTypeContract, OpenApiFeature, OpenApiVersion, Operation,
    OperationKey, ParameterContract, ParameterLocation, RequestBodyContract, ResponseBodyContract,
    ResponseContract, ResponseStatus, SchemaFeature, SchemaVersion, ServiceInfo,
};
pub use effect::{Effect, EffectAction, EffectActionKind, EffectOrigin, EffectValue};
pub use webhook::{WebhookContract, WebhookEmission, WebhookOrigin};
