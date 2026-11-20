use simile_spec::ResourceKind;
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

    #[error("principal_setup `{principal}` references unknown operationId `{operation_id}`")]
    UnknownSetupOperation {
        principal: String,
        operation_id: String,
    },

    #[error("principal_setup `{principal}` collect value `{name}` is declared more than once")]
    DuplicateSetupCollect { principal: String, name: String },

    #[error("principal_setup `{principal}` issued value `{name}` is declared more than once")]
    DuplicateSetupIssue { principal: String, name: String },

    #[error("principal_setup is declared more than once for principal `{principal}`")]
    DuplicatePrincipalSetup { principal: String },

    #[error("principal_setup references unknown principal `{principal}`")]
    UnknownSetupPrincipal { principal: String },

    #[error(
        "principal_setup `{principal}` issued value `{issue}` references unknown credential `{credential}`"
    )]
    UnknownSetupCredential {
        principal: String,
        issue: String,
        credential: String,
    },

    #[error(
        "principal_setup `{principal}` cannot issue credential `{credential}` from `{issue}` as one string"
    )]
    InvalidSetupCredential {
        principal: String,
        issue: String,
        credential: String,
    },

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

    #[error("{file}: backend kind `{kind}` is not a Simile abstraction")]
    UnknownBackendKind { file: String, kind: String },

    #[error("backend `{backend}` is declared more than once")]
    DuplicateBackend { backend: String },

    #[error("behavior for operationId `{operation_id}` is declared more than once")]
    DuplicateBehavior { operation_id: String },

    #[error("error set `{name}` is declared more than once")]
    DuplicateErrorSet { name: String },

    #[error("scenario `{name}` is declared more than once")]
    DuplicateScenario { name: String },

    #[error("scenario `{scenario}` selects operationId `{operation_id}` more than once")]
    DuplicateScenarioSelection {
        scenario: String,
        operation_id: String,
    },

    #[error(
        "scenario `{scenario}` injects a fault into operationId `{operation_id}` more than once"
    )]
    DuplicateScenarioFault {
        scenario: String,
        operation_id: String,
    },

    #[error("fault `{name}` is declared more than once")]
    DuplicateFault { name: String },

    #[error("catalog fault `{name}` does not exist; available faults: {available}")]
    UnknownCatalogFault { name: String, available: String },

    #[error(
        "{file}: invalid fault selector `{selector}`; use an exact name, `namespace.*`, `*`, or prefix one with `^`"
    )]
    InvalidFaultSelector { file: String, selector: String },

    #[error("{file}: fault exclusion `{selector}` cannot contain overrides or an alias")]
    ConfiguredFaultExclusion { file: String, selector: String },

    #[error("fault selector `{selector}` does not match any catalog fault")]
    UnmatchedFaultSelector { selector: String },

    #[error(
        "fault selector `{selector}` selects multiple faults and cannot contain overrides or an alias"
    )]
    ConfiguredFaultSet { selector: String },

    #[error("catalog fault `{fault}` does not support the `{attribute}` override")]
    UnsupportedCatalogFaultOverride {
        fault: String,
        attribute: &'static str,
    },

    #[error("fault profile `{name}` is declared more than once")]
    DuplicateFaultProfile { name: String },

    #[error("webhook infrastructure policy is declared more than once")]
    DuplicateWebhookPolicy,

    #[error("principal `{name}` is declared more than once")]
    DuplicatePrincipal { name: String },

    #[error("principal `{principal}` declares credential `{scheme}` more than once")]
    DuplicatePrincipalCredential { principal: String, scheme: String },

    #[error("principal `{principal}` references unknown OpenAPI security scheme `{scheme}`")]
    UnknownPrincipalSecurityScheme { principal: String, scheme: String },

    #[error(
        "principal `{principal}` credential `{scheme}` does not match its OpenAPI security scheme"
    )]
    InvalidPrincipalCredential { principal: String, scheme: String },

    #[error("profile `{name}` is declared more than once")]
    DuplicateProfile { name: String },

    #[error("profile `{profile}` references unknown principal `{principal}`")]
    UnknownProfilePrincipal { profile: String, principal: String },

    #[error(
        "profile `{profile}` references credential `{credential}` absent from principal `{principal}`"
    )]
    UnknownProfileCredential {
        profile: String,
        principal: String,
        credential: String,
    },

    #[error("profile `{profile}` declares credential `{credential}` more than once")]
    DuplicateProfileCredential { profile: String, credential: String },

    #[error(
        "profile `{profile}` credential `{credential}` does not match its OpenAPI security scheme"
    )]
    InvalidProfileCredential { profile: String, credential: String },

    #[error("fault profile `{profile}` lists fault `{fault}` more than once")]
    DuplicateFaultProfileFault { profile: String, fault: String },

    #[error("fault profile `{profile}` lists operationId `{operation_id}` more than once")]
    DuplicateFaultProfileOperation {
        profile: String,
        operation_id: String,
    },

    #[error("{file}: fault `{fault}` must contain exactly one action block")]
    FaultActionCount { file: String, fault: String },

    #[error("fault `{fault}` uses action `{action}` at incompatible phase `{phase}`")]
    IncompatibleFaultPhase {
        fault: String,
        action: &'static str,
        phase: &'static str,
    },

    #[error("{owner} references unknown fault `{fault}`")]
    UnknownFault { owner: String, fault: String },

    #[error("fault profile `{profile}` references unknown OpenAPI operationId `{operation_id}`")]
    UnknownFaultProfileOperation {
        profile: String,
        operation_id: String,
    },

    #[error("async operation `{name}` is declared more than once")]
    DuplicateAsyncOperation { name: String },

    #[error("effect `{name}` is declared more than once")]
    DuplicateEffect { name: String },

    #[error("webhook `{name}` is declared more than once")]
    DuplicateWebhook { name: String },

    #[error("webhook `{name}` is already declared by the API contract")]
    ContractWebhookRedefined { name: String },

    #[error("{file}: effect emits webhook `{webhook}` more than once")]
    DuplicateWebhookEmission { file: String, webhook: String },

    #[error("{file}: effect configures unknown emitted webhook `{webhook}`")]
    UnknownWebhookEmissionOverride { file: String, webhook: String },

    #[error(
        "{file}: webhook `{webhook}` has invalid dependency `{dependency}`; dependencies must be distinct emitted webhooks"
    )]
    InvalidWebhookDependency {
        file: String,
        webhook: String,
        dependency: String,
    },

    #[error("effect `{effect}` emits unknown webhook `{webhook}`")]
    UnknownEffectWebhook { effect: String, webhook: String },

    #[error("effect `{effect}` contains a webhook dependency cycle")]
    WebhookDependencyCycle { effect: String },

    #[error("{file}: effect `{effect}` must contain an action or name asynchronous work")]
    EmptyEffect { file: String, effect: String },

    #[error(
        "{file}: action target `{backend}` must be a declared backend address such as `db.orders`"
    )]
    InvalidActionBackend { file: String, backend: String },

    #[error("{file}: `{action}` is not an effect action supported by `{resource_kind}` backends")]
    UnknownEffectAction {
        file: String,
        resource_kind: ResourceKind,
        action: String,
    },

    #[error("{file}: action `{backend} {action}` does not accept argument `{argument}`")]
    UnknownEffectArgument {
        file: String,
        backend: String,
        action: String,
        argument: String,
    },

    #[error("{file}: action `{backend} {action}` requires argument `{argument}`")]
    MissingEffectArgument {
        file: String,
        backend: String,
        action: String,
        argument: &'static str,
    },

    #[error(
        "{file}: effect argument `{attribute}` must be a literal or a request/response projection"
    )]
    InvalidEffectValue { file: String, attribute: String },

    #[error("effect `{effect}` references unknown OpenAPI operationId `{operation_id}`")]
    UnknownEffectOperation {
        effect: String,
        operation_id: String,
    },

    #[error("effect `{effect}` references unknown async operation `{operation}")]
    UnknownEffectAsync { effect: String, operation: String },

    #[error("{owner} action references undeclared backend `{backend}`")]
    UnknownEffectBackend { owner: String, backend: String },

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

    #[error("scenario `{scenario}` injects a fault into unknown operationId `{operation_id}`")]
    UnknownScenarioFaultOperation {
        scenario: String,
        operation_id: String,
    },

    #[error("async operation `{operation}` uses undeclared backend `{backend}`")]
    UnknownBackend { operation: String, backend: String },
}
