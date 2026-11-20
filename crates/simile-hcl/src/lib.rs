#![forbid(unsafe_code)]

mod ast;
mod auth;
mod compile;
mod error;
mod parse;
mod service;
mod webhook;

pub use ast::{
    ActionDeclaration, AsyncDeclaration, BackendDeclaration, BehaviorDeclaration,
    ContractDeclaration, EffectDeclaration, ErrorSetApplicationDeclaration, ErrorSetDeclaration,
    FaultDeclaration, FaultProfileDeclaration, FaultUseDeclaration, ListenDeclaration, Module,
    OutcomeDeclaration, PrincipalCredentialDeclaration, PrincipalDeclaration,
    ProtocolErrorDeclaration, ScenarioDeclaration, ScenarioFaultDeclaration,
    ScenarioSelectionDeclaration, ServiceDeclaration, ServiceDocument, Source, SourceDeclaration,
    WebhookDeclaration, WebhookEmissionDeclaration, WebhookPolicyDeclaration,
};
pub use compile::compile;
pub use error::{Error, Result};
pub use parse::parse;
pub use service::parse_service;
