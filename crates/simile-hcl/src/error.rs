use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not parse HCL source `{name}`: {source}")]
    Parse {
        name: String,
        #[source]
        source: hcl::Error,
    },

    #[error("{file}: top-level attribute `{attribute}` is not allowed")]
    TopLevelAttribute { file: String, attribute: String },

    #[error("{file}: unknown top-level block `{block}`")]
    UnknownBlock { file: String, block: String },

    #[error("{file}: {block} block requires {expected} label(s), found {actual}")]
    Labels {
        file: String,
        block: String,
        expected: usize,
        actual: usize,
    },

    #[error("{file}: `{block}` does not accept attribute `{attribute}`")]
    UnknownAttribute {
        file: String,
        block: String,
        attribute: String,
    },

    #[error("{file}: `{block}` does not accept nested block `{nested}`")]
    UnknownNestedBlock {
        file: String,
        block: String,
        nested: String,
    },

    #[error("{file}: `{block}` requires attribute `{attribute}`")]
    MissingAttribute {
        file: String,
        block: String,
        attribute: &'static str,
    },

    #[error("{file}: `{block}.{attribute}` must be {expected}")]
    InvalidAttribute {
        file: String,
        block: String,
        attribute: String,
        expected: &'static str,
    },

    #[error("{file}: resource kind `{kind}` is not a Simile abstraction")]
    UnknownResourceKind { file: String, kind: String },

    #[error("resource `{resource}` is declared more than once")]
    DuplicateResource { resource: String },

    #[error("behavior for operationId `{operation_id}` is declared more than once")]
    DuplicateBehavior { operation_id: String },

    #[error("async operation `{name}` is declared more than once")]
    DuplicateAsyncOperation { name: String },

    #[error("behavior references unknown OpenAPI operationId `{operation_id}`")]
    UnknownOperation { operation_id: String },

    #[error("operationId `{operation_id}` does not declare response status {status} in OpenAPI")]
    UndeclaredResponse { operation_id: String, status: u16 },

    #[error("behavior for `{operation_id}` triggers unknown async operation `{target}`")]
    UnknownAsyncTarget {
        operation_id: String,
        target: String,
    },

    #[error("async operation `{operation}` uses undeclared resource `{resource}`")]
    UnknownResource { operation: String, resource: String },
}
