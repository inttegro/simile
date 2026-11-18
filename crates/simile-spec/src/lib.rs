#![forbid(unsafe_code)]

mod behavior;
mod contract;
pub mod fault_catalog;

pub use behavior::{
    AsyncOperation, Behavior, BehaviorOrigin, BehaviorSpec, CompiledOperation, CompiledService,
    ErrorSet, ErrorSetApplication, Fault, FaultAction, FaultPhase, FaultProfile, Outcome,
    OutcomeCondition, OutcomeProvenance, ProtocolError, Resource, ResourceKind, ResourceRef,
    Scenario,
};
pub use contract::{
    ApiContract, HttpMethod, MediaTypeContract, OpenApiFeature, OpenApiVersion, Operation,
    OperationKey, RequestBodyContract, ResponseBodyContract, ResponseContract, ResponseStatus,
    ServiceInfo,
};
