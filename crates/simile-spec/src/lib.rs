#![forbid(unsafe_code)]

mod behavior;
mod contract;

pub use behavior::{
    AsyncOperation, Behavior, BehaviorOrigin, BehaviorSpec, CompiledOperation, CompiledService,
    Resource, ResourceKind, ResourceRef,
};
pub use contract::{
    ApiContract, HttpMethod, OpenApiFeature, OpenApiVersion, Operation, OperationKey,
    ResponseContract, ResponseStatus, ServiceInfo,
};
