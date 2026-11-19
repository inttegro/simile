#![forbid(unsafe_code)]

mod behavior;
mod contract;
mod effect;
pub mod fault_catalog;

pub use behavior::{
    AsyncOperation, Behavior, BehaviorOrigin, BehaviorSpec, CompiledOperation, CompiledService,
    DescriptionCatalog, DescriptionEntry, DescriptionKind, ErrorSet, ErrorSetApplication, Fault,
    FaultAction, FaultPhase, FaultProfile, Outcome, OutcomeCondition, OutcomeProvenance,
    ProtocolError, Resource, ResourceKind, ResourceRef, Scenario,
};
pub use contract::{
    ApiContract, HttpMethod, MediaTypeContract, OpenApiFeature, OpenApiVersion, Operation,
    OperationKey, ParameterContract, ParameterLocation, RequestBodyContract, ResponseBodyContract,
    ResponseContract, ResponseStatus, ServiceInfo,
};
pub use effect::{Effect, EffectAction, EffectActionKind, EffectOrigin, EffectValue};
