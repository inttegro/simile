#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use simile_heuristics::Policy;
use simile_spec::{BehaviorOrigin, CompiledOperation, CompiledService};
use thiserror::Error;

#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub name: &'a str,
    pub contents: &'a str,
}

impl<'a> Source<'a> {
    #[must_use]
    pub const fn new(name: &'a str, contents: &'a str) -> Self {
        Self { name, contents }
    }
}

#[derive(Debug)]
pub struct CompileRequest<'a> {
    pub openapi: Source<'a>,
    pub behaviors: Vec<Source<'a>>,
    pub heuristic_policy: Policy,
}

#[derive(Debug)]
pub struct ParsedCompileRequest<'a> {
    pub openapi: Source<'a>,
    pub behaviors: Vec<simile_hcl::Module>,
    pub heuristic_policy: Policy,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Compiler;

impl Compiler {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn compile(&self, request: CompileRequest<'_>) -> Result<CompiledService> {
        let modules = request
            .behaviors
            .into_iter()
            .map(|source| simile_hcl::parse(simile_hcl::Source::new(source.name, source.contents)))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.compile_parsed(ParsedCompileRequest {
            openapi: request.openapi,
            behaviors: modules,
            heuristic_policy: request.heuristic_policy,
        })
    }

    pub fn compile_parsed(&self, request: ParsedCompileRequest<'_>) -> Result<CompiledService> {
        let contract =
            simile_openapi::parse(request.openapi.contents).map_err(|source| Error::OpenApi {
                name: request.openapi.name.to_owned(),
                source,
            })?;
        let behaviors = simile_hcl::compile(request.behaviors, &contract)?;

        let operations = contract
            .operations
            .iter()
            .map(|(key, operation)| {
                let behavior = operation
                    .operation_id
                    .as_ref()
                    .and_then(|id| behaviors.behaviors.get(id))
                    .cloned();
                let origin = if behavior.is_some() {
                    BehaviorOrigin::Hcl
                } else {
                    BehaviorOrigin::OpenApi
                };
                (
                    key.clone(),
                    CompiledOperation {
                        contract: operation.clone(),
                        origin,
                        behavior,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        Ok(CompiledService {
            contract,
            heuristic_policy: request.heuristic_policy,
            operations,
            resources: behaviors.resources,
            async_operations: behaviors.async_operations,
        })
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not compile OpenAPI source `{name}`: {source}")]
    OpenApi {
        name: String,
        #[source]
        source: simile_openapi::Error,
    },

    #[error(transparent)]
    Behavior(#[from] simile_hcl::Error),
}
