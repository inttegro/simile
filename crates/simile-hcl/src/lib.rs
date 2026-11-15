#![forbid(unsafe_code)]

mod ast;
mod compile;
mod error;
mod parse;

pub use ast::{AsyncDeclaration, BehaviorDeclaration, Module, ResourceDeclaration, Source};
pub use compile::compile;
pub use error::{Error, Result};
pub use parse::parse;
