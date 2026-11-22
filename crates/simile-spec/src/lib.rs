#![forbid(unsafe_code)]

mod auth;
mod behavior;
mod contract;
mod effect;
pub mod fault_catalog;
mod principal;
mod profile;
mod setup;
mod webhook;

pub use auth::{SecurityRequirement, SecurityScheme};
pub use behavior::{
    AsyncOperation, Behavior, BehaviorOrigin, BehaviorSpec, CompiledOperation, CompiledService,
    DescriptionCatalog, DescriptionEntry, DescriptionKind, ErrorSet, ErrorSetApplication, Fault,
    FaultAction, FaultPhase, FaultProfile, Outcome, OutcomeCondition, OutcomeProvenance,
    ProtocolError, Resource, ResourceKind, ResourceRef, Scenario, StateProjection,
    StateProjectionPredicate, StateProjectionValueState, WebhookOrdering, WebhookPolicy,
    WebhookSigning,
};
pub use contract::{
    ApiContract, HttpMethod, MediaTypeContract, OpenApiFeature, OpenApiVersion, Operation,
    OperationKey, ParameterContract, ParameterLocation, RequestBodyContract, ResponseBodyContract,
    ResponseContract, ResponseHeaderContract, ResponseStatus, SchemaFeature, SchemaVersion,
    ServiceInfo,
};
pub use effect::{Effect, EffectAction, EffectActionKind, EffectOrigin, EffectValue};
pub use principal::{Principal, PrincipalCredential};
pub use profile::{Profile, ProfileCredential, ProfileStatus};
pub use setup::{
    PrincipalSetup, SetupBinding, SetupCollect, SetupIssue, SetupIssueSource, SetupValueType,
};
pub use webhook::{WebhookContract, WebhookEmission, WebhookOrigin};
