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

    #[error("{file}: service configuration requires exactly one `service` block")]
    MissingService { file: String },

    #[error("{file}: service configuration contains more than one `service` block")]
    DuplicateService { file: String },

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

    #[error("{file}: `{block}` requires nested block `{nested}`")]
    MissingNestedBlock {
        file: String,
        block: String,
        nested: &'static str,
    },

    #[error("{file}: `{block}` contains more than one `{nested}` block")]
    DuplicateNestedBlock {
        file: String,
        block: String,
        nested: &'static str,
    },

    #[error("{file}: `{block}` requires exactly one of attributes {attributes}")]
    ExclusiveAttributes {
        file: String,
        block: String,
        attributes: &'static str,
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

    #[error("error set `{name}` is declared more than once")]
    DuplicateErrorSet { name: String },

    #[error("scenario `{name}` is declared more than once")]
    DuplicateScenario { name: String },

    #[error("async operation `{name}` is declared more than once")]
    DuplicateAsyncOperation { name: String },

    #[error("behavior references unknown OpenAPI operationId `{operation_id}`")]
    UnknownOperation { operation_id: String },

    #[error("operationId `{operation_id}` does not declare response status {status} in OpenAPI")]
    UndeclaredResponse { operation_id: String, status: u16 },

    #[error(
        "outcome `{outcome}` for operationId `{operation_id}` uses response status {status}, which is not declared in OpenAPI"
    )]
    UndeclaredOutcomeResponse {
        operation_id: String,
        outcome: String,
        status: u16,
    },

    #[error("behavior for `{operation_id}` references unknown error set `{name}`")]
    UnknownErrorSet { operation_id: String, name: String },

    #[error("apply_error_set references unknown error set `{name}`")]
    UnknownAppliedErrorSet { name: String },

    #[error("apply_error_set references unknown OpenAPI tag `{tag}`")]
    UnknownOpenApiTag { tag: String },

    #[error("scenario `{scenario}` references unknown operationId `{operation_id}`")]
    UnknownScenarioOperation {
        scenario: String,
        operation_id: String,
    },

    #[error("behavior for `{operation_id}` triggers unknown async operation `{target}`")]
    UnknownAsyncTarget {
        operation_id: String,
        target: String,
    },

    #[error("async operation `{operation}` uses undeclared resource `{resource}`")]
    UnknownResource { operation: String, resource: String },
}
