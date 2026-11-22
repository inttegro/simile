#![forbid(unsafe_code)]

mod ast;
mod compile;
mod error;
mod parse;
mod principal;
mod profile;
mod service;
mod setup;
mod webhook;

pub use ast::{
    ActionDeclaration, AsyncDeclaration, BackendDeclaration, BehaviorDeclaration,
    ContractDeclaration, EffectDeclaration, ErrorSetApplicationDeclaration, ErrorSetDeclaration,
    FaultDeclaration, FaultProfileDeclaration, FaultUseDeclaration, ListenDeclaration, Module,
    OutcomeDeclaration, PrincipalCredentialDeclaration, PrincipalDeclaration,
    PrincipalSetupDeclaration, ProfileCredentialDeclaration, ProfileDeclaration,
    ProtocolErrorDeclaration, ReportingDeclaration, ScenarioDeclaration, ScenarioFaultDeclaration,
    ScenarioSelectionDeclaration, ServiceDeclaration, ServiceDocument, SetupCollectDeclaration,
    SetupIssueDeclaration, Source, SourceDeclaration, WebhookDeclaration,
    WebhookEmissionDeclaration, WebhookPolicyDeclaration,
};
pub use compile::{compile, compile_principal_setups};
pub use error::{Error, Result};
pub use parse::parse;
pub use service::parse_service;
