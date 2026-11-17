#![forbid(unsafe_code)]

mod ast;
mod compile;
mod error;
mod parse;
mod service;

pub use ast::{
    AsyncDeclaration, BehaviorDeclaration, ErrorSetApplicationDeclaration, ErrorSetDeclaration,
    ListenDeclaration, Module, OpenApiDeclaration, OutcomeDeclaration, ProtocolErrorDeclaration,
    ResourceDeclaration, ScenarioDeclaration, ScenarioSelectionDeclaration, ServiceDeclaration,
    ServiceDocument, Source,
};
pub use compile::compile;
pub use error::{Error, Result};
pub use parse::parse;
pub use service::parse_service;
