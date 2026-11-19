#![forbid(unsafe_code)]

mod ast;
mod compile;
mod error;
mod parse;
mod service;

pub use ast::{
    ActionDeclaration, AsyncDeclaration, BehaviorDeclaration, ContractDeclaration,
    EffectDeclaration, ErrorSetApplicationDeclaration, ErrorSetDeclaration, FaultDeclaration,
    FaultProfileDeclaration, FaultUseDeclaration, ListenDeclaration, Module, OutcomeDeclaration,
    ProtocolErrorDeclaration, ResourceDeclaration, ScenarioDeclaration, ScenarioFaultDeclaration,
    ScenarioSelectionDeclaration, ServiceDeclaration, ServiceDocument, Source, SourceDeclaration,
};
pub use compile::compile;
pub use error::{Error, Result};
pub use parse::parse;
pub use service::parse_service;
