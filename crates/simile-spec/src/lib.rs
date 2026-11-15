#![forbid(unsafe_code)]

mod behavior;
mod contract;

pub use behavior::{
    AsyncOperation, Behavior, BehaviorOrigin, BehaviorSpec, CompiledOperation, CompiledService,
    Resource, ResourceKind, ResourceRef,
};
pub use contract::{
    ApiContract, HttpMethod, MediaTypeContract, OpenApiFeature, OpenApiVersion, Operation,
    OperationKey, RequestBodyContract, ResponseBodyContract, ResponseContract, ResponseStatus,
    ServiceInfo,
};
