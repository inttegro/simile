use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{IgnoredAny, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use simile_heuristics::Policy;
use simile_schema::{
    BodySchemaCompiler, BoundRequestBody, BoundResponseBody, BoundValueSchema, RelationParameter,
    RequestBodyPlan, ResponseBodyPlan, SchemaCatalog, ValueSchemaPlan,
};
use simile_spec::{
    BehaviorOrigin, CompiledService, DescriptionKind, EffectAction, EffectActionKind, EffectOrigin,
    EffectValue, FaultAction, FaultPhase, HttpMethod, OpenApiFeature, OutcomeProvenance,
    ParameterLocation, PrincipalSetup, ProfileCredential, ProfileStatus, ResourceKind, ResourceRef,
    ResponseStatus, SecurityScheme, SetupBinding, SetupCollect, SetupIssue, SetupIssueSource,
    SetupValueType, WebhookEmission, WebhookOrdering, WebhookOrigin, WebhookPolicy,
};

use crate::{Error, Result};

pub const FORMAT_VERSION: u16 = 21;
pub const EXTENSION: &str = "similec";

const MAGIC: &[u8; 8] = b"SIMILEC\0";
const FLAGS: u16 = 0;
const DIGEST_LENGTH: usize = 32;
const HEADER_LENGTH: usize = MAGIC.len() + 2 + 2 + 8 + DIGEST_LENGTH;

type StringId = u32;
type ValueId = u32;
type ResourceId = u32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSummary {
    pub format_version: u16,
    pub compiler_version: String,
    pub service: String,
    pub service_version: String,
    pub openapi_version: String,
    pub heuristic_policy: Policy,
    pub operations: usize,
    pub parameters: usize,
    pub request_bodies: usize,
    pub response_bodies: usize,
    pub schema_nodes: usize,
    pub heuristic_rules: usize,
    pub responses: usize,
    pub outcomes: usize,
    pub provider_authored_outcomes: usize,
    pub scenario_selections: usize,
    pub scenario_faults: usize,
    pub faults: usize,
    pub fault_profiles: usize,
    pub schema_synthesized_outcomes: usize,
    pub underspecified_operations: usize,
    pub public_export_safe: bool,
    pub operation_outcomes: Vec<OperationOutcomeSummary>,
    pub value_nodes: usize,
    pub resources: usize,
    pub async_operations: usize,
    pub effects: usize,
    pub webhooks: usize,
    pub webhook_emissions: usize,
    pub provider_authored_effects: usize,
    pub convention_inferred_effects: usize,
    pub effect_actions: usize,
    pub encoded_bytes: usize,
    pub payload_sha256: String,
    pub webhook_policy: WebhookPolicy,
    pub security_schemes: usize,
    pub principals: usize,
    pub profiles: usize,
    pub principal_setups: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationOutcomeSummary {
    pub operation: String,
    pub outcomes: Vec<OutcomeSummary>,
    pub underspecified: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeSummary {
    pub name: String,
    pub status: u16,
    pub provenance: OutcomeProvenance,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Artifact {
    payload: Payload,
    encoded_bytes: usize,
    digest: [u8; DIGEST_LENGTH],
}

/// The runtime-facing portion of one compiled operation.
#[derive(Clone, Copy, Debug)]
pub struct ArtifactOperation<'a> {
    key: &'a str,
    method: &'a str,
    path: &'a str,
    operation_id: Option<&'a str>,
    summary: Option<&'a str>,
    request_body: Option<&'a RequestBodyPlan>,
    parameters: &'a [ParameterRecord],
    responses: &'a [ResponseRecord],
    outcomes: &'a [OutcomeRecord],
    behavior_response_status: Option<u16>,
    strings: &'a [String],
    values: &'a [ValueRecord],
    schemas: &'a SchemaCatalog,
    security: &'a [SecurityRequirementRecord],
    origin: BehaviorOrigin,
}

#[derive(Clone, Copy, Debug)]
pub struct ArtifactSecurityRequirement<'a> {
    entries: &'a [(StringId, Vec<StringId>)],
    strings: &'a [String],
}

#[derive(Clone, Copy, Debug)]
pub struct ArtifactSecurityScheme<'a> {
    record: &'a SecuritySchemeRecord,
    strings: &'a [String],
}

#[derive(Clone, Copy, Debug)]
pub struct ArtifactPrincipal<'a> {
    record: &'a PrincipalRecord,
    strings: &'a [String],
}

#[derive(Clone, Copy, Debug)]
pub struct ArtifactProfile<'a> {
    record: &'a ProfileRecord,
    strings: &'a [String],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactCredential<'a> {
    Secret(&'a str),
    Basic {
        username: &'a str,
        password: &'a str,
    },
}

/// One compiled HTTP parameter available to transport adapters.
#[derive(Clone, Copy, Debug)]
pub struct ArtifactParameter<'a> {
    name: &'a str,
    location: ParameterLocation,
    required: bool,
    style: &'a str,
    explode: bool,
    allow_reserved: bool,
    schema: Option<BoundValueSchema<'a>>,
}

/// One compiled response available to the runtime for an operation.
#[derive(Clone, Debug)]
pub struct ArtifactResponse<'a> {
    status: ResponseStatus,
    body: Option<BoundResponseBody<'a>>,
    headers: &'a [ResponseHeaderRecord],
    strings: &'a [String],
    values: &'a [ValueRecord],
    schemas: &'a SchemaCatalog,
}

/// One compiled response header available to an HTTP transport adapter.
#[derive(Clone, Copy, Debug)]
pub struct ArtifactResponseHeader<'a> {
    record: &'a ResponseHeaderRecord,
    strings: &'a [String],
    values: &'a [ValueRecord],
    schemas: &'a SchemaCatalog,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactOutcome {
    name: String,
    status: u16,
    content_type: Option<String>,
    body: Option<Value>,
    provenance: OutcomeProvenance,
    when: Vec<ArtifactOutcomeCondition>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactOutcomeCondition {
    path: String,
    equals: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactProtocolError {
    cause: String,
    statuses: Vec<u16>,
    content_type: Option<String>,
    body: Option<Value>,
    provenance: OutcomeProvenance,
}

/// One opaque public scenario loaded from a compiled artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactScenario {
    name: String,
    selections: Vec<(String, String)>,
    faults: Vec<(String, String)>,
}

/// One environmental fault loaded from a compiled artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactFault {
    name: String,
    phase: FaultPhase,
    action: FaultAction,
}

/// One bounded stochastic fault policy loaded from a compiled artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactFaultProfile {
    name: String,
    probability_ppm: u32,
    faults: Vec<String>,
    operations: Vec<String>,
    max_consecutive: u32,
    cooldown_requests: u32,
    seed: Option<u64>,
}

/// One asynchronous operation triggered by a compiled behavior.
#[derive(Clone, Copy, Debug)]
pub struct ArtifactAsyncOperation<'a> {
    name: &'a str,
    uses: &'a [ResourceId],
    strings: &'a [String],
    resources: &'a [ResourceRecord],
    actions: &'a [ActionRecord],
    values: &'a [ValueRecord],
}

/// One provider-authored effect loaded from a compiled artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactEffect {
    name: String,
    origin: EffectOrigin,
    operations: Vec<String>,
    actions: Vec<EffectAction>,
    async_operations: Vec<String>,
    webhooks: Vec<WebhookEmission>,
}

/// One outbound webhook contract loaded from the artifact.
#[derive(Clone, Copy, Debug)]
pub struct ArtifactWebhook<'a> {
    name: &'a str,
    method: &'a str,
    operation_id: Option<&'a str>,
    summary: Option<&'a str>,
    target: Option<&'a str>,
    request_body: Option<&'a RequestBodyPlan>,
    origin: WebhookOrigin,
    schemas: &'a SchemaCatalog,
}

/// One backend used by an asynchronous operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactResource<'a> {
    kind: ResourceKind,
    name: &'a str,
}

/// One human-readable authoring description retained in the artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactDescription<'a> {
    kind: DescriptionKind,
    parent: Option<&'a str>,
    name: &'a str,
    text: &'a str,
}

impl<'a> ArtifactOperation<'a> {
    #[must_use]
    pub const fn key(&self) -> &'a str {
        self.key
    }

    #[must_use]
    pub const fn method(&self) -> &'a str {
        self.method
    }

    #[must_use]
    pub const fn path(&self) -> &'a str {
        self.path
    }

    #[must_use]
    pub const fn operation_id(&self) -> Option<&'a str> {
        self.operation_id
    }

    #[must_use]
    pub const fn summary(&self) -> Option<&'a str> {
        self.summary
    }

    #[must_use]
    pub fn request_body(&self) -> Option<BoundRequestBody<'a>> {
        self.request_body
            .map(|plan| BoundRequestBody::new(plan, self.schemas))
    }

    pub fn parameters(&self) -> impl Iterator<Item = ArtifactParameter<'a>> + 'a {
        self.parameters.iter().map(|parameter| ArtifactParameter {
            name: &self.strings[parameter.0 as usize],
            location: decode_parameter_location(parameter.1),
            required: parameter.2,
            style: &self.strings[parameter.3 as usize],
            explode: parameter.4,
            allow_reserved: parameter.5,
            schema: parameter
                .6
                .as_ref()
                .map(|schema| BoundValueSchema::new(schema, self.schemas)),
        })
    }

    pub fn responses(&self) -> impl Iterator<Item = ArtifactResponse<'a>> + 'a {
        self.responses.iter().map(|response| ArtifactResponse {
            status: decode_status(response.0, response.1),
            body: response
                .3
                .as_ref()
                .map(|body| BoundResponseBody::new(body, self.schemas)),
            headers: &response.4,
            strings: self.strings,
            values: self.values,
            schemas: self.schemas,
        })
    }

    #[must_use]
    pub const fn behavior_response_status(&self) -> Option<u16> {
        self.behavior_response_status
    }

    /// Source of the operation's behavioral refinement.
    #[must_use]
    pub const fn origin(&self) -> BehaviorOrigin {
        self.origin
    }

    pub fn outcomes(&self) -> impl Iterator<Item = ArtifactOutcome> + '_ {
        self.outcomes.iter().map(|outcome| ArtifactOutcome {
            name: self.strings[outcome.0 as usize].clone(),
            status: outcome.1,
            content_type: outcome.2.map(|id| self.strings[id as usize].clone()),
            body: outcome
                .3
                .map(|id| decode_value(id, self.strings, self.values())),
            provenance: decode_outcome_provenance(outcome.4),
            when: outcome
                .5
                .iter()
                .map(|condition| ArtifactOutcomeCondition {
                    path: self.strings[condition.0 as usize].clone(),
                    equals: decode_value(condition.1, self.strings, self.values()),
                })
                .collect(),
        })
    }

    pub fn security_requirements(
        &self,
    ) -> impl Iterator<Item = ArtifactSecurityRequirement<'a>> + 'a {
        self.security
            .iter()
            .map(|requirement| ArtifactSecurityRequirement {
                entries: &requirement.0,
                strings: self.strings,
            })
    }

    fn values(&self) -> &[ValueRecord] {
        // The value arena is stored on the artifact rather than the operation;
        // this field is injected by `operation_at` through the schema-adjacent slot.
        self.values
    }
}

impl<'a> ArtifactSecurityRequirement<'a> {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn schemes(&self) -> impl Iterator<Item = (&'a str, impl Iterator<Item = &'a str>)> + 'a {
        self.entries.iter().map(|(scheme, scopes)| {
            (
                self.strings[*scheme as usize].as_str(),
                scopes
                    .iter()
                    .map(|scope| self.strings[*scope as usize].as_str()),
            )
        })
    }
}

impl<'a> ArtifactSecurityScheme<'a> {
    #[must_use]
    pub fn name(&self) -> &'a str {
        &self.strings[self.record.0 as usize]
    }

    #[must_use]
    pub fn scheme(&self) -> SecurityScheme {
        decode_security_scheme(self.record, self.strings)
    }
}

impl<'a> ArtifactPrincipal<'a> {
    #[must_use]
    pub fn name(&self) -> &'a str {
        &self.strings[self.record.0 as usize]
    }

    #[must_use]
    pub fn grants(&self, scheme: &str, scope: &str) -> bool {
        self.record
            .1
            .iter()
            .find(|credential| self.strings[credential.0 as usize] == scheme)
            .is_some_and(|credential| {
                credential
                    .1
                    .iter()
                    .any(|grant| self.strings[*grant as usize] == scope)
            })
    }

    pub fn credential_schemes(&self) -> impl Iterator<Item = &'a str> + 'a {
        self.record
            .1
            .iter()
            .map(|credential| self.strings[credential.0 as usize].as_str())
    }

    pub fn granted_scopes(&self) -> impl Iterator<Item = &'a str> + 'a {
        self.record.1.iter().flat_map(|credential| {
            credential
                .1
                .iter()
                .map(|grant| self.strings[*grant as usize].as_str())
        })
    }
}

impl<'a> ArtifactProfile<'a> {
    #[must_use]
    pub fn name(&self) -> &'a str {
        &self.strings[self.record.0 as usize]
    }

    #[must_use]
    pub fn principal(&self) -> &'a str {
        &self.strings[self.record.1 as usize]
    }

    #[must_use]
    pub fn status(&self) -> ProfileStatus {
        decode_profile_status(self.record.2)
    }

    #[must_use]
    pub fn credential(&self, scheme: &str) -> Option<ArtifactCredential<'a>> {
        self.record
            .3
            .iter()
            .find(|credential| self.strings[credential.0 as usize] == scheme)
            .map(|credential| match credential.1 {
                0 => ArtifactCredential::Secret(&self.strings[credential.2 as usize]),
                1 => {
                    let Some(password) = credential.3 else {
                        unreachable!("artifact validation requires a basic password")
                    };
                    ArtifactCredential::Basic {
                        username: &self.strings[credential.2 as usize],
                        password: &self.strings[password as usize],
                    }
                }
                _ => unreachable!("artifact validation rejects unknown credential kinds"),
            })
    }

    pub fn credential_schemes(&self) -> impl Iterator<Item = &'a str> + 'a {
        self.record
            .3
            .iter()
            .map(|credential| self.strings[credential.0 as usize].as_str())
    }
}

impl ArtifactParameter<'_> {
    #[must_use]
    pub const fn name(&self) -> &str {
        self.name
    }

    #[must_use]
    pub const fn location(&self) -> ParameterLocation {
        self.location
    }

    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.required
    }

    #[must_use]
    pub const fn style(&self) -> &str {
        self.style
    }

    #[must_use]
    pub const fn explode(&self) -> bool {
        self.explode
    }

    #[must_use]
    pub const fn allow_reserved(&self) -> bool {
        self.allow_reserved
    }

    #[must_use]
    pub const fn schema(&self) -> Option<BoundValueSchema<'_>> {
        self.schema
    }
}

impl<'a> ArtifactResponse<'a> {
    #[must_use]
    pub const fn status(&self) -> &ResponseStatus {
        &self.status
    }

    #[must_use]
    pub const fn body(&self) -> Option<BoundResponseBody<'a>> {
        self.body
    }

    pub fn headers(&self) -> impl Iterator<Item = ArtifactResponseHeader<'a>> + 'a {
        self.headers.iter().map(|record| ArtifactResponseHeader {
            record,
            strings: self.strings,
            values: self.values,
            schemas: self.schemas,
        })
    }
}

impl ArtifactResponseHeader<'_> {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.strings[self.record.0 as usize]
    }

    #[must_use]
    pub const fn schema(&self) -> Option<BoundValueSchema<'_>> {
        match self.record.1.as_ref() {
            Some(schema) => Some(BoundValueSchema::new(schema, self.schemas)),
            None => None,
        }
    }

    #[must_use]
    pub fn example(&self) -> Option<Value> {
        self.record
            .2
            .map(|value| decode_value(value, self.strings, self.values))
    }
}

impl ArtifactOutcome {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    #[must_use]
    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    #[must_use]
    pub const fn body(&self) -> Option<&Value> {
        self.body.as_ref()
    }

    #[must_use]
    pub const fn provenance(&self) -> OutcomeProvenance {
        self.provenance
    }

    #[must_use]
    pub fn conditions(&self) -> &[ArtifactOutcomeCondition] {
        &self.when
    }
}

impl ArtifactOutcomeCondition {
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub const fn equals(&self) -> &Value {
        &self.equals
    }
}

impl ArtifactProtocolError {
    #[must_use]
    pub fn cause(&self) -> &str {
        &self.cause
    }

    #[must_use]
    pub fn statuses(&self) -> &[u16] {
        &self.statuses
    }

    #[must_use]
    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    #[must_use]
    pub const fn body(&self) -> Option<&Value> {
        self.body.as_ref()
    }

    #[must_use]
    pub const fn provenance(&self) -> OutcomeProvenance {
        self.provenance
    }
}

impl ArtifactScenario {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn selections(&self) -> &[(String, String)] {
        &self.selections
    }

    #[must_use]
    pub fn faults(&self) -> &[(String, String)] {
        &self.faults
    }
}

impl ArtifactFault {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn phase(&self) -> FaultPhase {
        self.phase
    }

    #[must_use]
    pub const fn action(&self) -> &FaultAction {
        &self.action
    }
}

impl ArtifactFaultProfile {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn probability_ppm(&self) -> u32 {
        self.probability_ppm
    }

    #[must_use]
    pub fn faults(&self) -> &[String] {
        &self.faults
    }

    #[must_use]
    pub fn operations(&self) -> &[String] {
        &self.operations
    }

    #[must_use]
    pub const fn max_consecutive(&self) -> u32 {
        self.max_consecutive
    }

    #[must_use]
    pub const fn cooldown_requests(&self) -> u32 {
        self.cooldown_requests
    }

    #[must_use]
    pub const fn seed(&self) -> Option<u64> {
        self.seed
    }
}

impl<'a> ArtifactAsyncOperation<'a> {
    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }

    /// Return the backends declared by this asynchronous operation.
    pub fn resources(&self) -> impl Iterator<Item = ArtifactResource<'a>> + 'a {
        let resources = self.resources;
        let strings = self.strings;
        self.uses.iter().map(move |id| {
            let resource = &resources[*id as usize];
            ArtifactResource {
                kind: decode_resource_kind(resource.0),
                name: &strings[resource.1 as usize],
            }
        })
    }

    #[must_use]
    pub fn actions(&self) -> Vec<EffectAction> {
        self.actions
            .iter()
            .map(|action| decode_effect_action(action, self.strings, self.resources, self.values))
            .collect()
    }
}

impl ArtifactEffect {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn origin(&self) -> EffectOrigin {
        self.origin
    }

    #[must_use]
    pub fn operations(&self) -> &[String] {
        &self.operations
    }

    #[must_use]
    pub fn actions(&self) -> &[EffectAction] {
        &self.actions
    }

    #[must_use]
    pub fn async_operations(&self) -> &[String] {
        &self.async_operations
    }

    #[must_use]
    pub fn webhooks(&self) -> &[WebhookEmission] {
        &self.webhooks
    }
}

impl<'a> ArtifactWebhook<'a> {
    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }

    #[must_use]
    pub const fn method(&self) -> &'a str {
        self.method
    }

    #[must_use]
    pub const fn operation_id(&self) -> Option<&'a str> {
        self.operation_id
    }

    #[must_use]
    pub const fn summary(&self) -> Option<&'a str> {
        self.summary
    }

    #[must_use]
    pub const fn target(&self) -> Option<&'a str> {
        self.target
    }

    #[must_use]
    pub const fn origin(&self) -> WebhookOrigin {
        self.origin
    }

    #[must_use]
    pub fn request_body(&self) -> Option<BoundRequestBody<'a>> {
        self.request_body
            .map(|plan| BoundRequestBody::new(plan, self.schemas))
    }
}

impl<'a> ArtifactResource<'a> {
    #[must_use]
    pub const fn kind(&self) -> ResourceKind {
        self.kind
    }

    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }

    #[must_use]
    pub fn reference(&self) -> ResourceRef {
        ResourceRef::new(self.kind, self.name)
    }
}

impl<'a> ArtifactDescription<'a> {
    #[must_use]
    pub const fn kind(&self) -> DescriptionKind {
        self.kind
    }

    #[must_use]
    pub const fn parent(&self) -> Option<&'a str> {
        self.parent
    }

    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }

    #[must_use]
    pub const fn text(&self) -> &'a str {
        self.text
    }
}

impl Artifact {
    /// Return the OpenAPI service description embedded in the contract, if any.
    #[must_use]
    pub fn service_description(&self) -> Option<&str> {
        self.payload.2.2.map(|id| self.string(id))
    }

    #[must_use]
    pub fn summary(&self) -> ArtifactSummary {
        let responses = self
            .payload
            .3
            .iter()
            .map(|operation| operation.7.len())
            .sum();
        let operation_outcomes = self
            .operations()
            .map(|operation| {
                let outcomes = operation
                    .outcomes()
                    .map(|outcome| OutcomeSummary {
                        name: outcome.name().to_owned(),
                        status: outcome.status(),
                        provenance: outcome.provenance(),
                    })
                    .collect::<Vec<_>>();
                let underspecified = outcomes.is_empty()
                    || outcomes
                        .iter()
                        .all(|outcome| outcome.provenance == OutcomeProvenance::SchemaSynthesized);
                OperationOutcomeSummary {
                    operation: operation.key().to_owned(),
                    outcomes,
                    underspecified,
                }
            })
            .collect::<Vec<_>>();
        let outcomes = operation_outcomes
            .iter()
            .flat_map(|operation| &operation.outcomes)
            .collect::<Vec<_>>();
        ArtifactSummary {
            format_version: FORMAT_VERSION,
            compiler_version: self.string(self.payload.0).to_owned(),
            service: self.string(self.payload.2.0).to_owned(),
            service_version: self.string(self.payload.2.1).to_owned(),
            openapi_version: self.string(self.payload.2.3).to_owned(),
            heuristic_policy: decode_heuristic_policy(self.payload.2.5),
            operations: self.payload.3.len(),
            parameters: self
                .payload
                .3
                .iter()
                .map(|operation| operation.11.len())
                .sum(),
            request_bodies: self
                .payload
                .3
                .iter()
                .filter(|operation| operation.6.is_some())
                .count(),
            response_bodies: self
                .payload
                .3
                .iter()
                .flat_map(|operation| &operation.7)
                .filter(|response| response.3.is_some())
                .count(),
            schema_nodes: self.payload.7.len(),
            heuristic_rules: self.payload.7.heuristic_rules(),
            responses,
            outcomes: outcomes.len(),
            provider_authored_outcomes: outcomes
                .iter()
                .filter(|outcome| outcome.provenance == OutcomeProvenance::ProviderAuthored)
                .count(),
            scenario_selections: self
                .payload
                .2
                .7
                .iter()
                .map(|scenario| scenario.1.len())
                .sum(),
            scenario_faults: self
                .payload
                .2
                .7
                .iter()
                .map(|scenario| scenario.2.len())
                .sum(),
            faults: self.payload.2.8.len(),
            fault_profiles: self.payload.2.9.len(),
            schema_synthesized_outcomes: outcomes
                .iter()
                .filter(|outcome| outcome.provenance == OutcomeProvenance::SchemaSynthesized)
                .count(),
            underspecified_operations: operation_outcomes
                .iter()
                .filter(|operation| operation.underspecified)
                .count(),
            public_export_safe: true,
            operation_outcomes,
            value_nodes: self.payload.4.len(),
            resources: self.payload.5.len(),
            async_operations: self.payload.6.len(),
            effects: self.payload.8.len(),
            webhooks: self.payload.9.len(),
            webhook_emissions: self.payload.8.iter().map(|effect| effect.5.len()).sum(),
            provider_authored_effects: self
                .payload
                .8
                .iter()
                .filter(|effect| decode_effect_origin(effect.4) == EffectOrigin::ProviderAuthored)
                .count(),
            convention_inferred_effects: self
                .payload
                .8
                .iter()
                .filter(|effect| decode_effect_origin(effect.4) == EffectOrigin::ConventionInferred)
                .count(),
            effect_actions: self
                .payload
                .8
                .iter()
                .map(|effect| effect.2.len())
                .sum::<usize>()
                + self
                    .payload
                    .6
                    .iter()
                    .map(|operation| operation.2.len())
                    .sum::<usize>(),
            encoded_bytes: self.encoded_bytes,
            payload_sha256: encode_hex(&self.digest),
            webhook_policy: self.webhook_policy(),
            security_schemes: self.payload.10.len(),
            principals: self.payload.11.len(),
            profiles: self.payload.12.len(),
            principal_setups: self.payload.13.len(),
        }
    }

    /// Return the service-wide webhook delivery infrastructure policy.
    #[must_use]
    pub fn webhook_policy(&self) -> WebhookPolicy {
        let signing = match (self.payload.2.15, self.payload.2.16, self.payload.2.17) {
            (Some(secret), Some(header), Some(timestamp_header)) => {
                Some(simile_spec::WebhookSigning {
                    secret: self.string(secret).to_owned(),
                    header: self.string(header).to_owned(),
                    timestamp_header: self.string(timestamp_header).to_owned(),
                })
            }
            (None, None, None) => None,
            _ => unreachable!("artifact validation requires a complete webhook signing policy"),
        };
        WebhookPolicy {
            ordering: decode_webhook_ordering(self.payload.2.11),
            out_of_order_probability_ppm: self.payload.2.12,
            reorder_window_milliseconds: self.payload.2.13,
            concurrency: self.payload.2.14,
            signing,
        }
    }

    /// Return the stable artifact fingerprint used to seed reproducible runtime decisions.
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Find a compiled operation by its stable `METHOD /path` key.
    #[must_use]
    pub fn operation(&self, key: &str) -> Option<ArtifactOperation<'_>> {
        self.payload
            .3
            .binary_search_by(|operation| self.string(operation.0).cmp(key))
            .ok()
            .map(|index| self.operation_at(index))
    }

    /// Find a compiled operation by its exact, case-sensitive OpenAPI `operationId`.
    #[must_use]
    pub fn operation_by_id(&self, operation_id: &str) -> Option<ArtifactOperation<'_>> {
        self.payload
            .3
            .iter()
            .position(|operation| {
                operation
                    .4
                    .is_some_and(|id| self.string(id) == operation_id)
            })
            .map(|index| self.operation_at(index))
    }

    /// Iterate over every compiled operation in stable structural-key order.
    pub fn operations(&self) -> impl Iterator<Item = ArtifactOperation<'_>> {
        (0..self.payload.3.len()).map(|index| self.operation_at(index))
    }

    pub fn security_schemes(&self) -> impl Iterator<Item = ArtifactSecurityScheme<'_>> {
        self.payload.10.iter().map(|record| ArtifactSecurityScheme {
            record,
            strings: &self.payload.1,
        })
    }

    #[must_use]
    pub fn security_scheme(&self, name: &str) -> Option<ArtifactSecurityScheme<'_>> {
        self.payload
            .10
            .binary_search_by(|record| self.string(record.0).cmp(name))
            .ok()
            .map(|index| ArtifactSecurityScheme {
                record: &self.payload.10[index],
                strings: &self.payload.1,
            })
    }

    pub fn principals(&self) -> impl Iterator<Item = ArtifactPrincipal<'_>> {
        self.payload.11.iter().map(|record| ArtifactPrincipal {
            record,
            strings: &self.payload.1,
        })
    }

    #[must_use]
    pub fn principal(&self, name: &str) -> Option<ArtifactPrincipal<'_>> {
        self.payload
            .11
            .binary_search_by(|record| self.string(record.0).cmp(name))
            .ok()
            .map(|index| ArtifactPrincipal {
                record: &self.payload.11[index],
                strings: &self.payload.1,
            })
    }

    pub fn profiles(&self) -> impl Iterator<Item = ArtifactProfile<'_>> {
        self.payload.12.iter().map(|record| ArtifactProfile {
            record,
            strings: &self.payload.1,
        })
    }

    /// Return every provider-authored principal onboarding workflow.
    pub fn principal_setups(&self) -> impl Iterator<Item = PrincipalSetup> + '_ {
        self.payload
            .13
            .iter()
            .map(|record| self.decode_setup(record))
    }

    /// Find the onboarding workflow for one principal.
    #[must_use]
    pub fn principal_setup(&self, principal: &str) -> Option<PrincipalSetup> {
        self.payload
            .13
            .binary_search_by(|record| self.string(record.0).cmp(principal))
            .ok()
            .map(|index| self.decode_setup(&self.payload.13[index]))
    }

    fn decode_setup(&self, record: &SetupRecord) -> PrincipalSetup {
        PrincipalSetup {
            principal: self.string(record.0).to_owned(),
            description: record.1.map(|id| self.string(id).to_owned()),
            operation_id: self.string(record.2).to_owned(),
            collects: record
                .3
                .iter()
                .map(|collect| {
                    let name = self.string(collect.0).to_owned();
                    (
                        name.clone(),
                        SetupCollect {
                            name,
                            description: collect.1.map(|id| self.string(id).to_owned()),
                            required: collect.2,
                            value_type: decode_setup_value_type(collect.3),
                            request_pointer: self.string(collect.4).to_owned(),
                            binding: collect.5.map(decode_setup_binding),
                        },
                    )
                })
                .collect(),
            issues: record
                .4
                .iter()
                .map(|issue| {
                    let name = self.string(issue.0).to_owned();
                    let payload = self.string(issue.3).to_owned();
                    (
                        name.clone(),
                        SetupIssue {
                            name,
                            description: issue.1.map(|id| self.string(id).to_owned()),
                            source: match issue.2 {
                                0 => SetupIssueSource::Response { pointer: payload },
                                1 => SetupIssueSource::Token { prefix: payload },
                                _ => {
                                    unreachable!("artifact validation rejects setup issue sources")
                                }
                            },
                            credential: issue.4.map(|id| self.string(id).to_owned()),
                            sensitive: issue.5,
                            binding: issue.6.map(decode_setup_binding),
                        },
                    )
                })
                .collect(),
        }
    }

    /// Return the compiled mapping for a transport/runtime failure cause.
    #[must_use]
    pub fn protocol_error(&self, cause: &str) -> Option<ArtifactProtocolError> {
        self.payload
            .2
            .6
            .iter()
            .find(|error| self.string(error.0) == cause)
            .map(|error| ArtifactProtocolError {
                cause: self.string(error.0).to_owned(),
                statuses: error.1.clone(),
                content_type: error.2.map(|id| self.string(id).to_owned()),
                body: error.3.map(|id| self.value(id)),
                provenance: decode_outcome_provenance(error.4),
            })
    }

    pub fn protocol_errors(&self) -> impl Iterator<Item = ArtifactProtocolError> + '_ {
        self.payload.2.6.iter().map(|error| ArtifactProtocolError {
            cause: self.string(error.0).to_owned(),
            statuses: error.1.clone(),
            content_type: error.2.map(|id| self.string(id).to_owned()),
            body: error.3.map(|id| self.value(id)),
            provenance: decode_outcome_provenance(error.4),
        })
    }

    pub fn scenarios(&self) -> impl Iterator<Item = ArtifactScenario> + '_ {
        self.payload.2.7.iter().map(|scenario| ArtifactScenario {
            name: self.string(scenario.0).to_owned(),
            selections: scenario
                .1
                .iter()
                .map(|(operation, outcome)| {
                    (
                        self.string(*operation).to_owned(),
                        self.string(*outcome).to_owned(),
                    )
                })
                .collect(),
            faults: scenario
                .2
                .iter()
                .map(|(operation, fault)| {
                    (
                        self.string(*operation).to_owned(),
                        self.string(*fault).to_owned(),
                    )
                })
                .collect(),
        })
    }

    pub fn resources(&self) -> impl Iterator<Item = ArtifactResource<'_>> {
        self.payload.5.iter().map(|resource| ArtifactResource {
            kind: decode_resource_kind(resource.0),
            name: self.string(resource.1),
        })
    }

    pub fn async_operations(&self) -> impl Iterator<Item = ArtifactAsyncOperation<'_>> {
        self.payload
            .6
            .iter()
            .map(|operation| ArtifactAsyncOperation {
                name: self.string(operation.0),
                uses: &operation.1,
                strings: &self.payload.1,
                resources: &self.payload.5,
                actions: &operation.2,
                values: &self.payload.4,
            })
    }

    /// Iterate over compiled effects in stable name order.
    pub fn effects(&self) -> impl Iterator<Item = ArtifactEffect> + '_ {
        self.payload.8.iter().map(|effect| ArtifactEffect {
            name: self.string(effect.0).to_owned(),
            origin: decode_effect_origin(effect.4),
            operations: effect
                .1
                .iter()
                .map(|operation| self.string(*operation).to_owned())
                .collect(),
            actions: effect
                .2
                .iter()
                .map(|action| {
                    decode_effect_action(action, &self.payload.1, &self.payload.5, &self.payload.4)
                })
                .collect(),
            async_operations: effect
                .3
                .iter()
                .map(|operation| self.string(*operation).to_owned())
                .collect(),
            webhooks: effect
                .5
                .iter()
                .map(|emission| decode_webhook_emission(emission, &self.payload.1, &self.payload.4))
                .collect(),
        })
    }

    /// Iterate over outbound webhook contracts in stable name order.
    pub fn webhooks(&self) -> impl Iterator<Item = ArtifactWebhook<'_>> {
        self.payload
            .9
            .iter()
            .map(|webhook| self.webhook_record(webhook))
    }

    /// Find one outbound webhook contract by its public name.
    #[must_use]
    pub fn webhook(&self, name: &str) -> Option<ArtifactWebhook<'_>> {
        self.payload
            .9
            .binary_search_by(|webhook| self.string(webhook.0).cmp(name))
            .ok()
            .map(|index| self.webhook_record(&self.payload.9[index]))
    }

    /// Return the effects attached to one stable operation key.
    pub fn effects_for<'a>(
        &'a self,
        operation: &'a str,
    ) -> impl Iterator<Item = ArtifactEffect> + 'a {
        self.effects().filter(move |effect| {
            effect
                .operations
                .iter()
                .any(|candidate| candidate == operation)
        })
    }

    /// Resolve an opaque scenario into the named outcome for one operation.
    #[must_use]
    pub fn scenario_outcome(&self, scenario: &str, operation: &str) -> Option<&str> {
        self.payload
            .2
            .7
            .iter()
            .find(|record| self.string(record.0) == scenario)
            .and_then(|record| {
                record
                    .1
                    .iter()
                    .find(|(key, _)| self.string(*key) == operation)
            })
            .map(|(_, outcome)| self.string(*outcome))
    }

    #[must_use]
    pub fn has_scenario(&self, scenario: &str) -> bool {
        self.payload
            .2
            .7
            .iter()
            .any(|record| self.string(record.0) == scenario)
    }

    /// Resolve an opaque scenario into the environmental fault for one operation.
    #[must_use]
    pub fn scenario_fault(&self, scenario: &str, operation: &str) -> Option<&str> {
        self.payload
            .2
            .7
            .iter()
            .find(|record| self.string(record.0) == scenario)
            .and_then(|record| {
                record
                    .2
                    .iter()
                    .find(|(key, _)| self.string(*key) == operation)
            })
            .map(|(_, fault)| self.string(*fault))
    }

    #[must_use]
    pub fn fault(&self, name: &str) -> Option<ArtifactFault> {
        self.payload
            .2
            .8
            .iter()
            .find(|record| self.string(record.0) == name)
            .map(|record| ArtifactFault {
                name: self.string(record.0).to_owned(),
                phase: decode_fault_phase(record.1),
                action: decode_fault_action(record.2, record.3),
            })
    }

    pub fn faults(&self) -> impl Iterator<Item = ArtifactFault> + '_ {
        self.payload.2.8.iter().map(|record| ArtifactFault {
            name: self.string(record.0).to_owned(),
            phase: decode_fault_phase(record.1),
            action: decode_fault_action(record.2, record.3),
        })
    }

    #[must_use]
    pub fn fault_profile(&self, name: &str) -> Option<ArtifactFaultProfile> {
        self.payload
            .2
            .9
            .iter()
            .find(|record| self.string(record.0) == name)
            .map(|record| self.decode_fault_profile(record))
    }

    pub fn fault_profiles(&self) -> impl Iterator<Item = ArtifactFaultProfile> + '_ {
        self.payload
            .2
            .9
            .iter()
            .map(|record| self.decode_fault_profile(record))
    }

    /// Iterate over public descriptions in stable semantic order.
    pub fn descriptions(&self) -> impl Iterator<Item = ArtifactDescription<'_>> {
        self.payload.2.10.iter().map(|record| ArtifactDescription {
            kind: decode_description_kind(record.0),
            parent: record.1.map(|id| self.string(id)),
            name: self.string(record.2),
            text: self.string(record.3),
        })
    }

    /// Find the description for one named semantic object.
    #[must_use]
    pub fn description(
        &self,
        kind: DescriptionKind,
        parent: Option<&str>,
        name: &str,
    ) -> Option<&str> {
        self.descriptions()
            .find(|description| {
                description.kind() == kind
                    && description.parent() == parent
                    && description.name() == name
            })
            .map(|description| description.text())
    }

    #[must_use]
    pub fn has_fault_profile(&self, name: &str) -> bool {
        self.payload
            .2
            .9
            .iter()
            .any(|record| self.string(record.0) == name)
    }

    fn decode_fault_profile(&self, record: &FaultProfileRecord) -> ArtifactFaultProfile {
        ArtifactFaultProfile {
            name: self.string(record.0).to_owned(),
            probability_ppm: record.1,
            faults: record
                .2
                .iter()
                .map(|id| self.string(*id).to_owned())
                .collect(),
            operations: record
                .3
                .iter()
                .map(|id| self.string(*id).to_owned())
                .collect(),
            max_consecutive: record.4,
            cooldown_requests: record.5,
            seed: record.6,
        }
    }

    fn operation_at(&self, index: usize) -> ArtifactOperation<'_> {
        let operation = &self.payload.3[index];
        ArtifactOperation {
            key: self.string(operation.0),
            method: decode_method(operation.1, operation.2.map(|id| self.string(id))),
            path: self.string(operation.3),
            operation_id: operation.4.map(|id| self.string(id)),
            summary: operation.5.map(|id| self.string(id)),
            request_body: operation.6.as_ref(),
            parameters: &operation.11,
            responses: &operation.7,
            outcomes: &operation.10,
            behavior_response_status: operation.9,
            strings: &self.payload.1,
            values: &self.payload.4,
            schemas: &self.payload.7,
            security: &operation.12,
            origin: decode_origin(operation.8),
        }
    }

    fn webhook_record<'a>(&'a self, webhook: &'a WebhookRecord) -> ArtifactWebhook<'a> {
        ArtifactWebhook {
            name: self.string(webhook.0),
            method: decode_method(webhook.1, webhook.2.map(|id| self.string(id))),
            operation_id: webhook.3.map(|id| self.string(id)),
            summary: webhook.4.map(|id| self.string(id)),
            target: webhook.5.map(|id| self.string(id)),
            request_body: webhook.6.as_ref(),
            origin: decode_webhook_origin(webhook.7),
            schemas: &self.payload.7,
        }
    }

    fn string(&self, id: StringId) -> &str {
        // Decoding validates every identifier before constructing Artifact.
        &self.payload.1[id as usize]
    }

    fn value(&self, id: ValueId) -> Value {
        decode_value(id, &self.payload.1, &self.payload.4)
    }
}

fn decode_value(id: ValueId, strings: &[String], values: &[ValueRecord]) -> Value {
    match &values[id as usize] {
        ValueRecord::Null => Value::Null,
        ValueRecord::Bool(value) => Value::Bool(*value),
        ValueRecord::Signed(value) => Value::from(*value),
        ValueRecord::Unsigned(value) => Value::from(*value),
        ValueRecord::Float(value) => Value::from(*value),
        ValueRecord::String(id) => Value::String(strings[*id as usize].clone()),
        ValueRecord::Array(children) => Value::Array(
            children
                .iter()
                .map(|child| decode_value(*child, strings, values))
                .collect(),
        ),
        ValueRecord::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    (
                        strings[*key as usize].clone(),
                        decode_value(*value, strings, values),
                    )
                })
                .collect(),
        ),
    }
}

fn decode_method(kind: u8, additional: Option<&str>) -> &str {
    match kind {
        0 => "GET",
        1 => "PUT",
        2 => "POST",
        3 => "DELETE",
        4 => "OPTIONS",
        5 => "HEAD",
        6 => "PATCH",
        7 => "TRACE",
        8 => "QUERY",
        9 => match additional {
            Some(method) => method,
            None => unreachable!("artifact validation requires an additional HTTP method"),
        },
        _ => unreachable!("artifact validation rejects unknown HTTP methods"),
    }
}

/// Encode a compiled service into the versioned `.similec` representation.
pub fn encode(service: &CompiledService) -> Result<Vec<u8>> {
    let payload = Payload::compile(service)?;
    let mut body = Vec::new();
    ciborium::into_writer(&payload, &mut body).map_err(|error| Error::Encode(error.to_string()))?;
    let body_length = u64::try_from(body.len()).map_err(|_| Error::TooLarge)?;
    let digest: [u8; DIGEST_LENGTH] = Sha256::digest(&body).into();

    let mut encoded = Vec::with_capacity(HEADER_LENGTH + body.len());
    encoded.extend_from_slice(MAGIC);
    encoded.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    encoded.extend_from_slice(&FLAGS.to_be_bytes());
    encoded.extend_from_slice(&body_length.to_be_bytes());
    encoded.extend_from_slice(&digest);
    encoded.extend_from_slice(&body);
    Ok(encoded)
}

/// Decode and validate a versioned `.similec` artifact.
pub fn decode(encoded: &[u8]) -> Result<Artifact> {
    if encoded.len() < HEADER_LENGTH {
        return Err(Error::Truncated);
    }
    if &encoded[..MAGIC.len()] != MAGIC {
        return Err(Error::InvalidMagic);
    }

    let format = u16::from_be_bytes(read_array(encoded, 8)?);
    if format != FORMAT_VERSION {
        return Err(Error::UnsupportedFormat(format));
    }
    let flags = u16::from_be_bytes(read_array(encoded, 10)?);
    if flags != FLAGS {
        return Err(Error::UnsupportedFlags(flags));
    }
    let body_length = u64::from_be_bytes(read_array(encoded, 12)?);
    let body_length = usize::try_from(body_length).map_err(|_| Error::TooLarge)?;
    let expected_length = HEADER_LENGTH
        .checked_add(body_length)
        .ok_or(Error::TooLarge)?;
    match encoded.len().cmp(&expected_length) {
        std::cmp::Ordering::Less => return Err(Error::Truncated),
        std::cmp::Ordering::Greater => return Err(Error::TrailingBytes),
        std::cmp::Ordering::Equal => {}
    }

    let mut expected_digest = [0_u8; DIGEST_LENGTH];
    expected_digest.copy_from_slice(&encoded[20..HEADER_LENGTH]);
    let body = &encoded[HEADER_LENGTH..];
    let digest: [u8; DIGEST_LENGTH] = Sha256::digest(body).into();
    if digest != expected_digest {
        return Err(Error::Integrity);
    }

    let payload: Payload =
        ciborium::from_reader(body).map_err(|error| Error::Decode(error.to_string()))?;
    payload.validate()?;
    Ok(Artifact {
        payload,
        encoded_bytes: encoded.len(),
        digest,
    })
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    let end = offset.checked_add(N).ok_or(Error::Truncated)?;
    let source = bytes.get(offset..end).ok_or(Error::Truncated)?;
    let mut value = [0_u8; N];
    value.copy_from_slice(source);
    Ok(value)
}

fn encode_hex(bytes: &[u8]) -> String {
    use fmt::Write;

    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

// Tuple records are deliberate: the envelope version owns their field order, and CBOR arrays
// avoid repeating JSON-style field names in every operation and schema node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Payload(
    StringId,
    Vec<String>,
    ServiceRecord,
    Vec<OperationRecord>,
    Vec<ValueRecord>,
    Vec<ResourceRecord>,
    Vec<AsyncRecord>,
    SchemaCatalog,
    Vec<EffectRecord>,
    Vec<WebhookRecord>,
    Vec<SecuritySchemeRecord>,
    Vec<PrincipalRecord>,
    Vec<ProfileRecord>,
    Vec<SetupRecord>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SetupRecord(
    StringId,
    Option<StringId>,
    StringId,
    Vec<SetupCollectRecord>,
    Vec<SetupIssueRecord>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SetupCollectRecord(StringId, Option<StringId>, bool, u8, StringId, Option<u8>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SetupIssueRecord(
    StringId,
    Option<StringId>,
    u8,
    StringId,
    Option<StringId>,
    bool,
    Option<u8>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ServiceRecord(
    StringId,
    StringId,
    Option<StringId>,
    StringId,
    u8,
    u8,
    Vec<ProtocolErrorRecord>,
    Vec<ScenarioRecord>,
    Vec<FaultRecord>,
    Vec<FaultProfileRecord>,
    Vec<DescriptionRecord>,
    u8,
    u32,
    u64,
    u16,
    Option<StringId>,
    Option<StringId>,
    Option<StringId>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct DescriptionRecord(u8, Option<StringId>, StringId, StringId);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct OperationRecord(
    StringId,
    u8,
    Option<StringId>,
    StringId,
    Option<StringId>,
    Option<StringId>,
    Option<RequestBodyPlan>,
    Vec<ResponseRecord>,
    u8,
    Option<u16>,
    Vec<OutcomeRecord>,
    Vec<ParameterRecord>,
    Vec<SecurityRequirementRecord>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SecurityRequirementRecord(Vec<(StringId, Vec<StringId>)>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SecuritySchemeRecord(
    StringId,
    u8,
    u8,
    Option<StringId>,
    Option<StringId>,
    Option<StringId>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PrincipalRecord(StringId, Vec<PrincipalCredentialContractRecord>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PrincipalCredentialContractRecord(StringId, Vec<StringId>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ProfileRecord(StringId, StringId, u8, Vec<CredentialRecord>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CredentialRecord(StringId, u8, StringId, Option<StringId>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ParameterRecord(
    StringId,
    u8,
    bool,
    StringId,
    bool,
    bool,
    Option<ValueSchemaPlan>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ResponseRecord(
    u8,
    u16,
    ValueId,
    Option<ResponseBodyPlan>,
    Vec<ResponseHeaderRecord>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ResponseHeaderRecord(StringId, Option<ValueSchemaPlan>, Option<ValueId>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct OutcomeRecord(
    StringId,
    u16,
    Option<StringId>,
    Option<ValueId>,
    u8,
    Vec<ConditionRecord>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ConditionRecord(StringId, ValueId);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ProtocolErrorRecord(StringId, Vec<u16>, Option<StringId>, Option<ValueId>, u8);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ScenarioRecord(
    StringId,
    Vec<(StringId, StringId)>,
    Vec<(StringId, StringId)>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct FaultRecord(StringId, u8, u8, u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct FaultProfileRecord(
    StringId,
    u32,
    Vec<StringId>,
    Vec<StringId>,
    u32,
    u32,
    Option<u64>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ResourceRecord(u8, StringId);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AsyncRecord(StringId, Vec<ResourceId>, Vec<ActionRecord>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct EffectRecord(
    StringId,
    Vec<StringId>,
    Vec<ActionRecord>,
    Vec<StringId>,
    u8,
    Vec<WebhookEmissionRecord>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WebhookEmissionRecord(
    StringId,
    EffectValueRecord,
    Option<EffectValueRecord>,
    Vec<StringId>,
    u64,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WebhookRecord(
    StringId,
    u8,
    Option<StringId>,
    Option<StringId>,
    Option<StringId>,
    Option<StringId>,
    Option<RequestBodyPlan>,
    u8,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ActionRecord(ResourceId, u8, Vec<(StringId, EffectValueRecord)>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
enum EffectValueRecord {
    Literal(ValueId),
    Request(StringId),
    Response(StringId),
    Array(Vec<Self>),
    Object(Vec<(StringId, Self)>),
}

#[derive(Clone, Debug, PartialEq)]
enum ValueRecord {
    Null,
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    String(StringId),
    Array(Vec<ValueId>),
    Object(Vec<(StringId, ValueId)>),
}

impl Serialize for ValueRecord {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple =
            serializer.serialize_tuple(if matches!(self, Self::Null) { 1 } else { 2 })?;
        match self {
            Self::Null => tuple.serialize_element(&0_u8)?,
            Self::Bool(value) => {
                tuple.serialize_element(&1_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::Signed(value) => {
                tuple.serialize_element(&2_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::Unsigned(value) => {
                tuple.serialize_element(&3_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::Float(value) => {
                tuple.serialize_element(&4_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::String(value) => {
                tuple.serialize_element(&5_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::Array(value) => {
                tuple.serialize_element(&6_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::Object(value) => {
                tuple.serialize_element(&7_u8)?;
                tuple.serialize_element(value)?;
            }
        }
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for ValueRecord {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(ValueRecordVisitor)
    }
}

struct ValueRecordVisitor;

impl<'de> Visitor<'de> for ValueRecordVisitor {
    type Value = ValueRecord;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a tagged compiled value node")
    }

    fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        use serde::de::Error as _;

        let tag = sequence
            .next_element::<u8>()?
            .ok_or_else(|| A::Error::custom("value node is missing its tag"))?;
        let value = match tag {
            0 => ValueRecord::Null,
            1 => ValueRecord::Bool(required(&mut sequence, "boolean")?),
            2 => ValueRecord::Signed(required(&mut sequence, "signed integer")?),
            3 => ValueRecord::Unsigned(required(&mut sequence, "unsigned integer")?),
            4 => ValueRecord::Float(required(&mut sequence, "floating point number")?),
            5 => ValueRecord::String(required(&mut sequence, "string identifier")?),
            6 => ValueRecord::Array(required(&mut sequence, "array children")?),
            7 => ValueRecord::Object(required(&mut sequence, "object fields")?),
            _ => return Err(A::Error::custom("value node has an unknown tag")),
        };
        if sequence.next_element::<IgnoredAny>()?.is_some() {
            return Err(A::Error::custom("value node contains trailing fields"));
        }
        Ok(value)
    }
}

fn required<'de, A, T>(sequence: &mut A, name: &str) -> std::result::Result<T, A::Error>
where
    A: SeqAccess<'de>,
    T: Deserialize<'de>,
{
    use serde::de::Error as _;

    sequence
        .next_element()?
        .ok_or_else(|| A::Error::custom(format!("value node is missing its {name}")))
}

impl Payload {
    fn compile(service: &CompiledService) -> Result<Self> {
        audit_public_export(service)?;
        let strings = StringTable::compile(service)?;
        let resource_ids = index_resources(service)?;
        let mut values = ValueArena::default();
        let mut schema_compiler = BodySchemaCompiler::with_heuristics(
            &service.contract.schemas,
            service.contract.openapi.feature,
            service.heuristic_policy,
        );

        let protocol_errors = service
            .protocol_errors
            .values()
            .map(|error| {
                Ok(ProtocolErrorRecord(
                    strings.id(&error.cause)?,
                    error.statuses.clone(),
                    strings.optional(error.content_type.as_deref())?,
                    error
                        .body
                        .as_ref()
                        .map(|body| values.push(body, &strings))
                        .transpose()?,
                    encode_outcome_provenance(error.provenance),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let scenarios = service
            .scenarios
            .values()
            .map(|scenario| {
                let selections = scenario
                    .selections
                    .iter()
                    .map(|(operation_id, outcome)| {
                        let operation = service
                            .operations
                            .iter()
                            .find(|(_, operation)| {
                                operation.contract.operation_id.as_deref()
                                    == Some(operation_id.as_str())
                            })
                            .map(|(key, _)| key)
                            .ok_or_else(|| Error::UnknownReference {
                                kind: "scenario operation",
                                name: operation_id.clone(),
                            })?;
                        Ok((strings.id(operation.as_str())?, strings.id(outcome)?))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let faults = scenario
                    .faults
                    .iter()
                    .map(|(operation_id, fault)| {
                        let operation = operation_key_for_id(service, operation_id)?;
                        Ok((strings.id(operation.as_str())?, strings.id(fault)?))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(ScenarioRecord(
                    strings.id(&scenario.name)?,
                    selections,
                    faults,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let faults = service
            .faults
            .values()
            .map(|fault| {
                let (action, value) = encode_fault_action(&fault.action);
                Ok(FaultRecord(
                    strings.id(&fault.name)?,
                    encode_fault_phase(fault.phase),
                    action,
                    value,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let fault_profiles = service
            .fault_profiles
            .values()
            .map(|profile| {
                let faults = profile
                    .faults
                    .iter()
                    .map(|fault| strings.id(fault))
                    .collect::<Result<Vec<_>>>()?;
                let operations = profile
                    .operations
                    .iter()
                    .map(|operation_id| {
                        operation_key_for_id(service, operation_id)
                            .and_then(|key| strings.id(key.as_str()))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(FaultProfileRecord(
                    strings.id(&profile.name)?,
                    profile.probability_ppm,
                    faults,
                    operations,
                    profile.max_consecutive,
                    profile.cooldown_requests,
                    profile.seed,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let descriptions = service
            .descriptions
            .entries()
            .iter()
            .map(|description| {
                Ok(DescriptionRecord(
                    encode_description_kind(description.kind),
                    strings.optional(description.parent.as_deref())?,
                    strings.id(&description.name)?,
                    strings.id(&description.text)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let service_record = ServiceRecord(
            strings.id(&service.contract.info.title)?,
            strings.id(&service.contract.info.version)?,
            strings.optional(service.contract.info.description.as_deref())?,
            strings.id(&service.contract.openapi.exact)?,
            encode_openapi_feature(service.contract.openapi.feature),
            encode_heuristic_policy(service.heuristic_policy),
            protocol_errors,
            scenarios,
            faults,
            fault_profiles,
            descriptions,
            encode_webhook_ordering(service.webhook_policy.ordering),
            service.webhook_policy.out_of_order_probability_ppm,
            service.webhook_policy.reorder_window_milliseconds,
            service.webhook_policy.concurrency,
            strings.optional(
                service
                    .webhook_policy
                    .signing
                    .as_ref()
                    .map(|signing| signing.secret.as_str()),
            )?,
            strings.optional(
                service
                    .webhook_policy
                    .signing
                    .as_ref()
                    .map(|signing| signing.header.as_str()),
            )?,
            strings.optional(
                service
                    .webhook_policy
                    .signing
                    .as_ref()
                    .map(|signing| signing.timestamp_header.as_str()),
            )?,
        );

        let mut operations = Vec::with_capacity(service.operations.len());
        for (key, compiled) in &service.operations {
            let operation = &compiled.contract;
            let (method, additional_method) = encode_method(&operation.method, &strings)?;
            let request_body = operation
                .request
                .as_ref()
                .map(|body| schema_compiler.compile_request(body))
                .transpose()
                .map_err(|source| Error::OperationSchema {
                    operation: key.to_string(),
                    source,
                })?;
            let parameters = operation
                .parameters
                .iter()
                .map(|parameter| {
                    let schema = parameter
                        .schema
                        .as_ref()
                        .map(|schema| {
                            schema_compiler.compile_parameter(
                                schema,
                                &format!("operation.{}.parameter.{}", key.as_str(), parameter.name),
                            )
                        })
                        .transpose()
                        .map_err(|source| Error::OperationSchema {
                            operation: key.to_string(),
                            source,
                        })?;
                    Ok(ParameterRecord(
                        strings.id(&parameter.name)?,
                        encode_parameter_location(parameter.location),
                        parameter.required,
                        strings.id(&parameter.style)?,
                        parameter.explode,
                        parameter.allow_reserved,
                        schema,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let relation_parameters = operation
                .parameters
                .iter()
                .zip(&parameters)
                .map(|(parameter, compiled)| RelationParameter {
                    location: parameter.location,
                    name: &parameter.name,
                    schema: compiled.6.as_ref(),
                })
                .collect::<Vec<_>>();
            let security = operation
                .security
                .iter()
                .map(|requirement| {
                    let schemes = requirement
                        .schemes
                        .iter()
                        .map(|(scheme, scopes)| {
                            Ok((
                                strings.id(scheme)?,
                                scopes
                                    .iter()
                                    .map(|scope| strings.id(scope))
                                    .collect::<Result<Vec<_>>>()?,
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Ok(SecurityRequirementRecord(schemes))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut ordered_responses = operation.responses.iter().collect::<Vec<_>>();
            ordered_responses.sort_by(|left, right| left.status.cmp(&right.status));
            let responses = ordered_responses
                .into_iter()
                .map(|response| {
                    let (kind, status) = encode_status(&response.status);
                    let mut ordered_headers = response.headers.iter().collect::<Vec<_>>();
                    ordered_headers.sort_by(|left, right| {
                        left.name
                            .to_ascii_lowercase()
                            .cmp(&right.name.to_ascii_lowercase())
                            .then_with(|| left.name.cmp(&right.name))
                    });
                    let headers = ordered_headers
                        .into_iter()
                        .map(|header| {
                            let schema = header
                                .schema
                                .as_ref()
                                .map(|schema| {
                                    schema_compiler.compile_response_header(
                                        schema,
                                        &format!(
                                            "operation.{}.response.{}.header.{}",
                                            key.as_str(),
                                            response.status,
                                            header.name
                                        ),
                                    )
                                })
                                .transpose()
                                .map_err(|source| Error::OperationSchema {
                                    operation: key.to_string(),
                                    source,
                                })?;
                            Ok(ResponseHeaderRecord(
                                strings.id(&header.name)?,
                                schema,
                                header
                                    .example
                                    .as_ref()
                                    .map(|example| values.push(example, &strings))
                                    .transpose()?,
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Ok(ResponseRecord(
                        kind,
                        status,
                        values.push(&response.definition, &strings)?,
                        response
                            .body
                            .as_ref()
                            // Why: a cross-message projection is meaningful only against this
                            // operation's request schema, so compile both sides into one proof plan.
                            .map(|body| {
                                schema_compiler.compile_response_for_operation(
                                    body,
                                    request_body.as_ref(),
                                    &relation_parameters,
                                )
                            })
                            .transpose()
                            .map_err(|source| Error::OperationSchema {
                                operation: key.to_string(),
                                source,
                            })?,
                        headers,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let behavior_response_status = compiled
                .behavior
                .as_ref()
                .and_then(|behavior| behavior.response_status);
            let outcomes = compiled
                .outcomes
                .iter()
                .map(|outcome| {
                    let conditions = outcome
                        .when
                        .iter()
                        .map(|condition| {
                            Ok(ConditionRecord(
                                strings.id(&condition.path)?,
                                values.push(&condition.equals, &strings)?,
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Ok(OutcomeRecord(
                        strings.id(&outcome.name)?,
                        outcome.status,
                        strings.optional(outcome.content_type.as_deref())?,
                        outcome
                            .body
                            .as_ref()
                            .map(|body| values.push(body, &strings))
                            .transpose()?,
                        encode_outcome_provenance(outcome.provenance),
                        conditions,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            operations.push(OperationRecord(
                strings.id(key.as_str())?,
                method,
                additional_method,
                strings.id(&operation.path)?,
                strings.optional(operation.operation_id.as_deref())?,
                strings.optional(operation.summary.as_deref())?,
                request_body,
                responses,
                encode_origin(compiled.origin),
                behavior_response_status,
                outcomes,
                parameters,
                security,
            ));
        }

        let resources = service
            .resources
            .values()
            .map(|resource| {
                Ok(ResourceRecord(
                    encode_resource_kind(resource.kind),
                    strings.id(&resource.name)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let async_operations = service
            .async_operations
            .values()
            .map(|operation| {
                let uses = operation
                    .uses
                    .iter()
                    .map(|reference| {
                        resource_ids.get(reference).copied().ok_or_else(|| {
                            Error::UnknownReference {
                                kind: "resource",
                                name: reference.as_str().to_owned(),
                            }
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let actions = operation
                    .actions
                    .iter()
                    .map(|action| {
                        compile_effect_action(action, &resource_ids, &strings, &mut values)
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(AsyncRecord(strings.id(&operation.name)?, uses, actions))
            })
            .collect::<Result<Vec<_>>>()?;
        let effects = service
            .effects
            .values()
            .map(|effect| {
                let operations = effect
                    .operations
                    .iter()
                    .map(|operation_id| {
                        operation_key_for_id(service, operation_id)
                            .and_then(|key| strings.id(key.as_str()))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let actions = effect
                    .actions
                    .iter()
                    .map(|action| {
                        compile_effect_action(action, &resource_ids, &strings, &mut values)
                    })
                    .collect::<Result<Vec<_>>>()?;
                let asynchronous = effect
                    .async_operations
                    .iter()
                    .map(|operation| strings.id(operation))
                    .collect::<Result<Vec<_>>>()?;
                let webhooks = effect
                    .webhooks
                    .iter()
                    .map(|emission| {
                        Ok(WebhookEmissionRecord(
                            strings.id(&emission.webhook)?,
                            compile_effect_value(&emission.body, &strings, &mut values)?,
                            emission
                                .target
                                .as_ref()
                                .map(|target| compile_effect_value(target, &strings, &mut values))
                                .transpose()?,
                            emission
                                .after
                                .iter()
                                .map(|dependency| strings.id(dependency))
                                .collect::<Result<Vec<_>>>()?,
                            emission.delay_milliseconds,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(EffectRecord(
                    strings.id(&effect.name)?,
                    operations,
                    actions,
                    asynchronous,
                    encode_effect_origin(effect.origin),
                    webhooks,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        let webhooks = service
            .webhooks
            .values()
            .map(|webhook| {
                let (method, additional_method) = encode_method(&webhook.method, &strings)?;
                Ok(WebhookRecord(
                    strings.id(&webhook.name)?,
                    method,
                    additional_method,
                    strings.optional(webhook.operation_id.as_deref())?,
                    strings.optional(webhook.summary.as_deref())?,
                    strings.optional(webhook.target.as_deref())?,
                    webhook
                        .request
                        .as_ref()
                        .map(|request| schema_compiler.compile_request(request))
                        .transpose()
                        .map_err(|source| Error::WebhookSchema {
                            webhook: webhook.name.clone(),
                            source,
                        })?,
                    encode_webhook_origin(webhook.origin),
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        let security_schemes = service
            .contract
            .security_schemes
            .iter()
            .map(|(name, scheme)| encode_security_scheme(name, scheme, &strings))
            .collect::<Result<Vec<_>>>()?;
        let principals = service
            .principals
            .values()
            .map(|principal| {
                let credentials = principal
                    .credentials
                    .iter()
                    .map(|(scheme, contract)| {
                        Ok(PrincipalCredentialContractRecord(
                            strings.id(scheme)?,
                            contract
                                .grants
                                .iter()
                                .map(|grant| strings.id(grant))
                                .collect::<Result<Vec<_>>>()?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(PrincipalRecord(strings.id(&principal.name)?, credentials))
            })
            .collect::<Result<Vec<_>>>()?;

        let profiles = service
            .profiles
            .values()
            .map(|profile| {
                let credentials = profile
                    .credentials
                    .iter()
                    .map(|(scheme, credential)| match credential {
                        ProfileCredential::Secret { value } => Ok(CredentialRecord(
                            strings.id(scheme)?,
                            0,
                            strings.id(value)?,
                            None,
                        )),
                        ProfileCredential::Basic { username, password } => Ok(CredentialRecord(
                            strings.id(scheme)?,
                            1,
                            strings.id(username)?,
                            Some(strings.id(password)?),
                        )),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(ProfileRecord(
                    strings.id(&profile.name)?,
                    strings.id(&profile.principal)?,
                    encode_profile_status(profile.status),
                    credentials,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        let setups = service
            .principal_setups
            .values()
            .map(|setup| {
                let collects = setup
                    .collects
                    .values()
                    .map(|collect| {
                        Ok(SetupCollectRecord(
                            strings.id(&collect.name)?,
                            strings.optional(collect.description.as_deref())?,
                            collect.required,
                            encode_setup_value_type(collect.value_type),
                            strings.id(&collect.request_pointer)?,
                            collect.binding.map(encode_setup_binding),
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let issues = setup
                    .issues
                    .values()
                    .map(|issue| {
                        let (kind, payload) = match &issue.source {
                            SetupIssueSource::Response { pointer } => (0, pointer.as_str()),
                            SetupIssueSource::Token { prefix } => (1, prefix.as_str()),
                        };
                        Ok(SetupIssueRecord(
                            strings.id(&issue.name)?,
                            strings.optional(issue.description.as_deref())?,
                            kind,
                            strings.id(payload)?,
                            strings.optional(issue.credential.as_deref())?,
                            issue.sensitive,
                            issue.binding.map(encode_setup_binding),
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok::<SetupRecord, Error>(SetupRecord(
                    strings.id(&setup.principal)?,
                    strings.optional(setup.description.as_deref())?,
                    strings.id(&setup.operation_id)?,
                    collects,
                    issues,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        let schemas = schema_compiler.finish()?;
        validate_outcome_bodies(&operations, &values.records, &strings.values, &schemas)?;

        Ok(Self(
            strings.id(env!("CARGO_PKG_VERSION"))?,
            strings.values,
            service_record,
            operations,
            values.records,
            resources,
            async_operations,
            schemas,
            effects,
            webhooks,
            security_schemes,
            principals,
            profiles,
            setups,
        ))
    }

    fn validate(&self) -> Result<()> {
        let strings = self.1.len();
        let values = self.4.len();
        let resources = self.5.len();
        let operation_keys = self
            .3
            .iter()
            .map(|operation| operation.0)
            .collect::<BTreeSet<_>>();
        let fault_names = self
            .2
            .8
            .iter()
            .map(|fault| fault.0)
            .collect::<BTreeSet<_>>();
        let security_scheme_names = self
            .10
            .iter()
            .map(|scheme| scheme.0)
            .collect::<BTreeSet<_>>();
        let operation_ids = self
            .3
            .iter()
            .filter_map(|operation| operation.4)
            .collect::<BTreeSet<_>>();
        validate_index(self.0, strings, "compiler version")?;
        validate_index(self.2.0, strings, "service title")?;
        validate_index(self.2.1, strings, "service version")?;
        validate_optional_index(self.2.2, strings, "service description")?;
        validate_index(self.2.3, strings, "OpenAPI version")?;
        if self.2.4 > 2 {
            return Err(Error::Invalid("OpenAPI feature line"));
        }
        if self.2.5 > 3 {
            return Err(Error::Invalid("heuristic policy"));
        }
        for error in &self.2.6 {
            validate_index(error.0, strings, "protocol error cause")?;
            if error.1.is_empty() || error.1.iter().any(|status| !(100..=599).contains(status)) {
                return Err(Error::Invalid("protocol error statuses"));
            }
            validate_optional_index(error.2, strings, "protocol error content type")?;
            validate_optional_index(error.3, values, "protocol error body")?;
            if error.4 > 3 {
                return Err(Error::Invalid("protocol error provenance"));
            }
        }
        for scenario in &self.2.7 {
            validate_index(scenario.0, strings, "scenario name")?;
            for (operation, outcome) in &scenario.1 {
                validate_index(*operation, strings, "scenario operation")?;
                validate_index(*outcome, strings, "scenario outcome")?;
            }
            for (operation, fault) in &scenario.2 {
                validate_index(*operation, strings, "scenario fault operation")?;
                validate_index(*fault, strings, "scenario fault")?;
                if !operation_keys.contains(operation) {
                    return Err(Error::Invalid("scenario fault operation reference"));
                }
                if !fault_names.contains(fault) {
                    return Err(Error::Invalid("scenario fault reference"));
                }
            }
        }
        for fault in &self.2.8 {
            validate_index(fault.0, strings, "fault name")?;
            if fault.1 > 1 {
                return Err(Error::Invalid("fault phase"));
            }
            if fault.2 > 1 {
                return Err(Error::Invalid("fault action"));
            }
            if fault.1 != fault.2 {
                return Err(Error::Invalid("fault phase and action"));
            }
            if (fault.2 == 0 && !(1..=86_400_000).contains(&fault.3))
                || (fault.2 == 1 && fault.3 != 0)
            {
                return Err(Error::Invalid("fault action payload"));
            }
        }
        for profile in &self.2.9 {
            validate_index(profile.0, strings, "fault profile name")?;
            if profile.1 == 0 || profile.1 > 1_000_000 || profile.2.is_empty() {
                return Err(Error::Invalid("fault profile probability or faults"));
            }
            for fault in &profile.2 {
                validate_index(*fault, strings, "fault profile fault")?;
                if !fault_names.contains(fault) {
                    return Err(Error::Invalid("fault profile fault reference"));
                }
            }
            for operation in &profile.3 {
                validate_index(*operation, strings, "fault profile operation")?;
                if !operation_keys.contains(operation) {
                    return Err(Error::Invalid("fault profile operation reference"));
                }
            }
            if profile.4 == 0 {
                return Err(Error::Invalid("fault profile maximum consecutive faults"));
            }
        }
        for description in &self.2.10 {
            if description.0 > 14 {
                return Err(Error::Invalid("description kind"));
            }
            validate_optional_index(description.1, strings, "description parent")?;
            validate_index(description.2, strings, "description name")?;
            validate_index(description.3, strings, "description text")?;
        }

        for setup in &self.13 {
            validate_index(setup.0, strings, "principal setup principal")?;
            validate_optional_index(setup.1, strings, "setup description")?;
            validate_index(setup.2, strings, "setup operation")?;
            if !operation_ids.contains(&setup.2) {
                return Err(Error::Invalid("setup operation reference"));
            }
            let mut collect_names = BTreeSet::new();
            for collect in &setup.3 {
                validate_index(collect.0, strings, "setup collect name")?;
                if !collect_names.insert(collect.0) {
                    return Err(Error::Invalid("setup collect ordering"));
                }
                validate_optional_index(collect.1, strings, "setup collect description")?;
                if collect.3 > 4 || collect.5.is_some_and(|binding| binding > 1) {
                    return Err(Error::Invalid("setup collect type or binding"));
                }
                validate_index(collect.4, strings, "setup collect request pointer")?;
            }
            let mut issue_names = BTreeSet::new();
            for issue in &setup.4 {
                validate_index(issue.0, strings, "setup issue name")?;
                if !issue_names.insert(issue.0) {
                    return Err(Error::Invalid("setup issue ordering"));
                }
                validate_optional_index(issue.1, strings, "setup issue description")?;
                if issue.2 > 1 || issue.6.is_some_and(|binding| binding > 1) {
                    return Err(Error::Invalid("setup issue source or binding"));
                }
                validate_index(issue.3, strings, "setup issue source")?;
                validate_optional_index(issue.4, strings, "setup issue credential")?;
                if issue
                    .4
                    .is_some_and(|credential| !security_scheme_names.contains(&credential))
                {
                    return Err(Error::Invalid("setup issue credential reference"));
                }
            }
        }
        if self.2.11 > 1 {
            return Err(Error::Invalid("webhook ordering policy"));
        }
        if self.2.12 > 1_000_000 || self.2.14 == 0 {
            return Err(Error::Invalid("webhook delivery policy"));
        }
        if self.2.11 == 1 && (self.2.12 != 0 || self.2.13 != 0) {
            return Err(Error::Invalid("strict webhook delivery policy"));
        }
        if self.2.12 > 0 && (self.2.13 == 0 || self.2.14 < 2) {
            return Err(Error::Invalid("webhook reordering policy"));
        }
        validate_optional_index(self.2.15, strings, "webhook signing secret")?;
        validate_optional_index(self.2.16, strings, "webhook signing header")?;
        validate_optional_index(self.2.17, strings, "webhook signing timestamp header")?;
        if self.2.15.is_some() != self.2.16.is_some() || self.2.15.is_some() != self.2.17.is_some()
        {
            return Err(Error::Invalid("webhook signing policy"));
        }

        for operation in &self.3 {
            validate_index(operation.0, strings, "operation key")?;
            if operation.1 > 9 {
                return Err(Error::Invalid("HTTP method"));
            }
            if operation.1 == 9 && operation.2.is_none() {
                return Err(Error::Invalid("additional HTTP method"));
            }
            if operation.1 != 9 && operation.2.is_some() {
                return Err(Error::Invalid("HTTP method payload"));
            }
            validate_optional_index(operation.2, strings, "additional HTTP method")?;
            validate_index(operation.3, strings, "operation path")?;
            validate_optional_index(operation.4, strings, "operation id")?;
            validate_optional_index(operation.5, strings, "operation summary")?;
            if operation
                .6
                .as_ref()
                .is_some_and(|request| !BoundRequestBody::new(request, &self.7).is_well_formed())
            {
                return Err(Error::Invalid("request schema"));
            }
            if operation.8 > 1 {
                return Err(Error::Invalid("behavior origin"));
            }
            for response in &operation.7 {
                if response.0 > 2 {
                    return Err(Error::Invalid("response status"));
                }
                if response.0 == 2 && response.1 != 0 {
                    return Err(Error::Invalid("default response status"));
                }
                validate_index(response.2, values, "response definition")?;
                if response
                    .3
                    .as_ref()
                    .is_some_and(|body| !BoundResponseBody::new(body, &self.7).is_well_formed())
                {
                    return Err(Error::Invalid("response schema"));
                }
                for header in &response.4 {
                    validate_index(header.0, strings, "response header name")?;
                    if header.1.as_ref().is_some_and(|schema| {
                        !BoundValueSchema::new(schema, &self.7).is_well_formed()
                    }) {
                        return Err(Error::Invalid("response header schema"));
                    }
                    validate_optional_index(header.2, values, "response header example")?;
                    if let (Some(schema), Some(example)) = (&header.1, header.2) {
                        let example = decode_value(example, &self.1, &self.4);
                        if !BoundValueSchema::new(schema, &self.7)
                            .validate(&example)
                            .is_empty()
                        {
                            return Err(Error::Invalid("response header example"));
                        }
                    }
                }
            }
            for outcome in &operation.10 {
                validate_index(outcome.0, strings, "outcome name")?;
                if !(100..=599).contains(&outcome.1) {
                    return Err(Error::Invalid("outcome status"));
                }
                validate_optional_index(outcome.2, strings, "outcome content type")?;
                validate_optional_index(outcome.3, values, "outcome body")?;
                if outcome.4 > 3 {
                    return Err(Error::Invalid("outcome provenance"));
                }
                for condition in &outcome.5 {
                    validate_index(condition.0, strings, "outcome condition path")?;
                    validate_index(condition.1, values, "outcome condition value")?;
                }
            }
            for parameter in &operation.11 {
                validate_index(parameter.0, strings, "parameter name")?;
                if parameter.1 > 3 {
                    return Err(Error::Invalid("parameter location"));
                }
                validate_index(parameter.3, strings, "parameter style")?;
                if parameter
                    .6
                    .as_ref()
                    .is_some_and(|schema| !BoundValueSchema::new(schema, &self.7).is_well_formed())
                {
                    return Err(Error::Invalid("parameter schema"));
                }
            }
            for requirement in &operation.12 {
                for (scheme, scopes) in &requirement.0 {
                    validate_index(*scheme, strings, "security requirement scheme")?;
                    if !security_scheme_names.contains(scheme) {
                        return Err(Error::Invalid("security requirement reference"));
                    }
                    for scope in scopes {
                        validate_index(*scope, strings, "security requirement scope")?;
                    }
                }
            }
        }
        for operations in self.3.windows(2) {
            let left = &self.1[operations[0].0 as usize];
            let right = &self.1[operations[1].0 as usize];
            if left >= right {
                return Err(Error::Invalid("operation ordering"));
            }
        }
        for value in &self.4 {
            match value {
                ValueRecord::String(id) => validate_index(*id, strings, "string value")?,
                ValueRecord::Array(children) => {
                    for child in children {
                        validate_index(*child, values, "array child")?;
                    }
                }
                ValueRecord::Object(fields) => {
                    for (key, child) in fields {
                        validate_index(*key, strings, "object key")?;
                        validate_index(*child, values, "object value")?;
                    }
                }
                ValueRecord::Null
                | ValueRecord::Bool(_)
                | ValueRecord::Signed(_)
                | ValueRecord::Unsigned(_)
                | ValueRecord::Float(_) => {}
            }
        }
        for resource in &self.5 {
            if resource.0 > 7 {
                return Err(Error::Invalid("resource kind"));
            }
            validate_index(resource.1, strings, "resource name")?;
        }
        for operation in &self.6 {
            validate_index(operation.0, strings, "async operation name")?;
            for resource in &operation.1 {
                validate_index(*resource, resources, "resource")?;
            }
            for action in &operation.2 {
                validate_effect_action(action, strings, values, resources, &self.4)?;
            }
        }
        if !self.7.is_well_formed() {
            return Err(Error::Invalid("body schema catalog"));
        }
        let async_names = self
            .6
            .iter()
            .map(|operation| operation.0)
            .collect::<BTreeSet<_>>();
        let webhook_names = self
            .9
            .iter()
            .map(|webhook| webhook.0)
            .collect::<BTreeSet<_>>();
        for effect in &self.8 {
            validate_index(effect.0, strings, "effect name")?;
            for operation in &effect.1 {
                validate_index(*operation, strings, "effect operation")?;
                if !operation_keys.contains(operation) {
                    return Err(Error::Invalid("effect operation reference"));
                }
            }
            for action in &effect.2 {
                validate_effect_action(action, strings, values, resources, &self.4)?;
            }
            for operation in &effect.3 {
                validate_index(*operation, strings, "effect async operation")?;
                if !async_names.contains(operation) {
                    return Err(Error::Invalid("effect async operation reference"));
                }
            }
            if effect.4 > 1 {
                return Err(Error::Invalid("effect origin"));
            }
            let emitted = effect
                .5
                .iter()
                .map(|emission| emission.0)
                .collect::<BTreeSet<_>>();
            if emitted.len() != effect.5.len() {
                return Err(Error::Invalid("duplicate effect webhook emission"));
            }
            for emission in &effect.5 {
                validate_index(emission.0, strings, "effect webhook")?;
                if !webhook_names.contains(&emission.0) {
                    return Err(Error::Invalid("effect webhook reference"));
                }
                validate_effect_value(&emission.1, strings, values, &self.4)?;
                if let Some(target) = &emission.2 {
                    validate_effect_value(target, strings, values, &self.4)?;
                }
                for dependency in &emission.3 {
                    validate_index(*dependency, strings, "webhook dependency")?;
                    if !emitted.contains(dependency) || dependency == &emission.0 {
                        return Err(Error::Invalid("effect webhook dependency"));
                    }
                }
            }
        }
        for webhook in &self.9 {
            validate_index(webhook.0, strings, "webhook name")?;
            if webhook.1 > 9 {
                return Err(Error::Invalid("webhook HTTP method"));
            }
            if webhook.1 == 9 && webhook.2.is_none() {
                return Err(Error::Invalid("webhook additional HTTP method"));
            }
            if webhook.1 != 9 && webhook.2.is_some() {
                return Err(Error::Invalid("webhook HTTP method payload"));
            }
            validate_optional_index(webhook.2, strings, "webhook additional HTTP method")?;
            validate_optional_index(webhook.3, strings, "webhook operation id")?;
            validate_optional_index(webhook.4, strings, "webhook summary")?;
            validate_optional_index(webhook.5, strings, "webhook target")?;
            if webhook
                .6
                .as_ref()
                .is_some_and(|request| !BoundRequestBody::new(request, &self.7).is_well_formed())
            {
                return Err(Error::Invalid("webhook request schema"));
            }
            if webhook.7 > 2 {
                return Err(Error::Invalid("webhook origin"));
            }
        }
        for webhooks in self.9.windows(2) {
            let left = &self.1[webhooks[0].0 as usize];
            let right = &self.1[webhooks[1].0 as usize];
            if left >= right {
                return Err(Error::Invalid("webhook ordering"));
            }
        }
        for scheme in &self.10 {
            validate_index(scheme.0, strings, "security scheme name")?;
            if scheme.1 > 4 || scheme.2 > 3 {
                return Err(Error::Invalid("security scheme"));
            }
            validate_optional_index(scheme.3, strings, "API key name")?;
            validate_optional_index(scheme.4, strings, "HTTP auth scheme")?;
            validate_optional_index(scheme.5, strings, "HTTP bearer format")?;
            if (scheme.1 == 0 && (scheme.3.is_none() || scheme.4.is_some() || scheme.5.is_some()))
                || (scheme.1 == 1 && (scheme.3.is_some() || scheme.4.is_none()))
                || (scheme.1 > 1
                    && (scheme.3.is_some() || scheme.4.is_some() || scheme.5.is_some()))
            {
                return Err(Error::Invalid("security scheme payload"));
            }
        }
        for schemes in self.10.windows(2) {
            if self.1[schemes[0].0 as usize] >= self.1[schemes[1].0 as usize] {
                return Err(Error::Invalid("security scheme ordering"));
            }
        }
        let mut principal_names = BTreeSet::new();
        for principal in &self.11 {
            validate_index(principal.0, strings, "principal name")?;
            principal_names.insert(principal.0);
            for credential in &principal.1 {
                validate_index(credential.0, strings, "principal credential scheme")?;
                if !security_scheme_names.contains(&credential.0) {
                    return Err(Error::Invalid("principal credential"));
                }
                for grant in &credential.1 {
                    validate_index(*grant, strings, "principal credential grant")?;
                }
            }
        }
        for principals in self.11.windows(2) {
            if self.1[principals[0].0 as usize] >= self.1[principals[1].0 as usize] {
                return Err(Error::Invalid("principal ordering"));
            }
        }
        for profile in &self.12 {
            validate_index(profile.0, strings, "profile name")?;
            validate_index(profile.1, strings, "profile principal")?;
            if !principal_names.contains(&profile.1) || profile.2 > 2 {
                return Err(Error::Invalid("profile"));
            }
            let principal = self
                .11
                .iter()
                .find(|principal| principal.0 == profile.1)
                .ok_or(Error::Invalid("profile principal"))?;
            for credential in &profile.3 {
                validate_index(credential.0, strings, "profile credential scheme")?;
                if !principal
                    .1
                    .iter()
                    .any(|contract| contract.0 == credential.0)
                    || credential.1 > 1
                {
                    return Err(Error::Invalid("profile credential"));
                }
                validate_index(credential.2, strings, "profile credential value")?;
                validate_optional_index(credential.3, strings, "profile credential password")?;
                if (credential.1 == 0 && credential.3.is_some())
                    || (credential.1 == 1 && credential.3.is_none())
                {
                    return Err(Error::Invalid("profile credential payload"));
                }
            }
        }
        for profiles in self.12.windows(2) {
            if self.1[profiles[0].0 as usize] >= self.1[profiles[1].0 as usize] {
                return Err(Error::Invalid("profile ordering"));
            }
        }
        for setups in self.13.windows(2) {
            if self.1[setups[0].0 as usize] >= self.1[setups[1].0 as usize] {
                return Err(Error::Invalid("principal setup ordering"));
            }
        }
        for setup in &self.13 {
            if !principal_names.contains(&setup.0) {
                return Err(Error::Invalid("principal setup principal"));
            }
            let principal = self
                .11
                .iter()
                .find(|principal| principal.0 == setup.0)
                .ok_or(Error::Invalid("principal setup principal"))?;
            for issue in &setup.4 {
                if issue.4.is_some_and(|credential| {
                    !principal.1.iter().any(|contract| contract.0 == credential)
                }) {
                    return Err(Error::Invalid("principal setup credential"));
                }
            }
        }
        Ok(())
    }
}

fn validate_effect_action(
    action: &ActionRecord,
    strings: usize,
    values: usize,
    resources: usize,
    value_records: &[ValueRecord],
) -> Result<()> {
    validate_index(action.0, resources, "effect action resource")?;
    if action.1 > 14 {
        return Err(Error::Invalid("effect action kind"));
    }
    for (name, value) in &action.2 {
        validate_index(*name, strings, "effect action argument")?;
        validate_effect_value(value, strings, values, value_records)?;
    }
    Ok(())
}

fn validate_effect_value(
    value: &EffectValueRecord,
    strings: usize,
    values: usize,
    value_records: &[ValueRecord],
) -> Result<()> {
    match value {
        EffectValueRecord::Literal(value) => {
            validate_index(*value, values, "effect literal")?;
            if value_records.get(*value as usize).is_none() {
                return Err(Error::Invalid("effect literal"));
            }
        }
        EffectValueRecord::Request(pointer) | EffectValueRecord::Response(pointer) => {
            validate_index(*pointer, strings, "effect projection")?;
        }
        EffectValueRecord::Array(items) => {
            for item in items {
                validate_effect_value(item, strings, values, value_records)?;
            }
        }
        EffectValueRecord::Object(fields) => {
            for (name, value) in fields {
                validate_index(*name, strings, "effect object field")?;
                validate_effect_value(value, strings, values, value_records)?;
            }
        }
    }
    Ok(())
}

fn operation_key_for_id<'a>(
    service: &'a CompiledService,
    operation_id: &str,
) -> Result<&'a simile_spec::OperationKey> {
    service
        .operations
        .iter()
        .find(|(_, operation)| operation.contract.operation_id.as_deref() == Some(operation_id))
        .map(|(key, _)| key)
        .ok_or_else(|| Error::UnknownReference {
            kind: "fault operation",
            name: operation_id.to_owned(),
        })
}

fn validate_outcome_bodies(
    operations: &[OperationRecord],
    values: &[ValueRecord],
    strings: &[String],
    schemas: &SchemaCatalog,
) -> Result<()> {
    for operation in operations {
        for outcome in &operation.10 {
            let Some(body_id) = outcome.3 else {
                continue;
            };
            let response = select_response_record(&operation.7, outcome.1)
                .and_then(|response| response.3.as_ref())
                .ok_or_else(|| Error::InvalidOutcomeBody {
                    operation: strings[operation.0 as usize].clone(),
                    outcome: strings[outcome.0 as usize].clone(),
                    issues: "the selected response declares no body".to_owned(),
                })?;
            let body = BoundResponseBody::new(response, schemas);
            let content_type = outcome
                .2
                .map(|id| strings[id as usize].as_str())
                .or_else(|| body.media_types().next())
                .ok_or_else(|| Error::InvalidOutcomeBody {
                    operation: strings[operation.0 as usize].clone(),
                    outcome: strings[outcome.0 as usize].clone(),
                    issues: "the selected response declares no media type".to_owned(),
                })?;
            let selected = body
                .select(content_type)
                .ok_or_else(|| Error::InvalidOutcomeBody {
                    operation: strings[operation.0 as usize].clone(),
                    outcome: strings[outcome.0 as usize].clone(),
                    issues: format!("content type `{content_type}` is not declared"),
                })?;
            let value = decode_value(body_id, strings, values);
            let issues = selected.validate(&value);
            if !issues.is_empty() {
                return Err(Error::InvalidOutcomeBody {
                    operation: strings[operation.0 as usize].clone(),
                    outcome: strings[outcome.0 as usize].clone(),
                    issues: format!("{issues:?}"),
                });
            }
        }
    }
    Ok(())
}

fn select_response_record(responses: &[ResponseRecord], status: u16) -> Option<&ResponseRecord> {
    responses
        .iter()
        .find(|response| response.0 == 0 && response.1 == status)
        .or_else(|| {
            responses
                .iter()
                .find(|response| response.0 == 1 && status / 100 == response.1)
        })
        .or_else(|| responses.iter().find(|response| response.0 == 2))
}

fn audit_public_export(service: &CompiledService) -> Result<()> {
    for description in service.descriptions.entries() {
        if let Some(parent) = &description.parent {
            audit_public_name(parent)?;
        }
        audit_public_name(&description.name)?;
        audit_public_name(&description.text)?;
    }
    for operation in service.operations.values() {
        for outcome in &operation.outcomes {
            audit_public_name(&outcome.name)?;
            if let Some(body) = &outcome.body {
                audit_public_value(body)?;
            }
        }
    }
    for error in service.protocol_errors.values() {
        audit_public_name(&error.cause)?;
        if let Some(body) = &error.body {
            audit_public_value(body)?;
        }
    }
    for scenario in service.scenarios.values() {
        audit_public_name(&scenario.name)?;
    }
    for fault in service.faults.values() {
        audit_public_name(&fault.name)?;
    }
    for profile in service.fault_profiles.values() {
        audit_public_name(&profile.name)?;
    }
    for webhook in service.webhooks.values() {
        audit_public_name(&webhook.name)?;
        if let Some(target) = &webhook.target {
            audit_public_name(target)?;
        }
    }
    Ok(())
}

fn audit_public_value(value: &Value) -> Result<()> {
    match value {
        Value::Array(values) => {
            for value in values {
                audit_public_value(value)?;
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                audit_public_name(key)?;
                audit_public_value(value)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn audit_public_name(value: &str) -> Result<()> {
    let normalized = value.to_ascii_lowercase();
    if normalized.starts_with("_internal")
        || normalized.starts_with("private_")
        || normalized.starts_with("_private")
        || normalized.contains("implementation_detail")
        || normalized.contains("fraud_threshold")
        || normalized.contains("database_table")
    {
        return Err(Error::UnsafePublicExport(value.to_owned()));
    }
    Ok(())
}

struct StringTable {
    values: Vec<String>,
    ids: BTreeMap<String, StringId>,
}

impl StringTable {
    fn compile(service: &CompiledService) -> Result<Self> {
        let mut values = BTreeSet::new();
        values.insert(env!("CARGO_PKG_VERSION").to_owned());
        values.insert(service.contract.info.title.clone());
        values.insert(service.contract.info.version.clone());
        values.insert(service.contract.openapi.exact.clone());
        if let Some(description) = &service.contract.info.description {
            values.insert(description.clone());
        }
        for (key, compiled) in &service.operations {
            values.insert(key.as_str().to_owned());
            let operation = &compiled.contract;
            values.insert(operation.path.clone());
            if let HttpMethod::Additional(method) = &operation.method {
                values.insert(method.clone());
            }
            if let Some(operation_id) = &operation.operation_id {
                values.insert(operation_id.clone());
            }
            if let Some(summary) = &operation.summary {
                values.insert(summary.clone());
            }
            for parameter in &operation.parameters {
                values.insert(parameter.name.clone());
                values.insert(parameter.style.clone());
            }
            for requirement in &operation.security {
                for (scheme, scopes) in &requirement.schemes {
                    values.insert(scheme.clone());
                    values.extend(scopes.iter().cloned());
                }
            }
            for response in &operation.responses {
                collect_value_strings(&response.definition, &mut values);
                for header in &response.headers {
                    values.insert(header.name.clone());
                    if let Some(example) = &header.example {
                        collect_value_strings(example, &mut values);
                    }
                }
            }
            for outcome in &compiled.outcomes {
                values.insert(outcome.name.clone());
                if let Some(content_type) = &outcome.content_type {
                    values.insert(content_type.clone());
                }
                if let Some(body) = &outcome.body {
                    collect_value_strings(body, &mut values);
                }
                for condition in &outcome.when {
                    values.insert(condition.path.clone());
                    collect_value_strings(&condition.equals, &mut values);
                }
            }
        }
        for error in service.protocol_errors.values() {
            values.insert(error.cause.clone());
            if let Some(content_type) = &error.content_type {
                values.insert(content_type.clone());
            }
            if let Some(body) = &error.body {
                collect_value_strings(body, &mut values);
            }
        }
        for scenario in service.scenarios.values() {
            values.insert(scenario.name.clone());
            for (operation_id, outcome) in &scenario.selections {
                values.insert(operation_id.clone());
                values.insert(outcome.clone());
            }
            for (operation_id, fault) in &scenario.faults {
                values.insert(operation_id.clone());
                values.insert(fault.clone());
            }
        }
        for fault in service.faults.values() {
            values.insert(fault.name.clone());
        }
        for profile in service.fault_profiles.values() {
            values.insert(profile.name.clone());
            values.extend(profile.faults.iter().cloned());
            values.extend(profile.operations.iter().cloned());
        }
        for description in service.descriptions.entries() {
            if let Some(parent) = &description.parent {
                values.insert(parent.clone());
            }
            values.insert(description.name.clone());
            values.insert(description.text.clone());
        }
        for resource in service.resources.values() {
            values.insert(resource.name.clone());
        }
        for operation in service.async_operations.values() {
            values.insert(operation.name.clone());
            for action in &operation.actions {
                collect_effect_action_strings(action, &mut values);
            }
        }
        for effect in service.effects.values() {
            values.insert(effect.name.clone());
            values.extend(effect.async_operations.iter().cloned());
            for action in &effect.actions {
                collect_effect_action_strings(action, &mut values);
            }
            for emission in &effect.webhooks {
                values.insert(emission.webhook.clone());
                collect_effect_value_strings(&emission.body, &mut values);
                if let Some(target) = &emission.target {
                    collect_effect_value_strings(target, &mut values);
                }
                values.extend(emission.after.iter().cloned());
            }
        }
        for webhook in service.webhooks.values() {
            values.insert(webhook.name.clone());
            if let HttpMethod::Additional(method) = &webhook.method {
                values.insert(method.clone());
            }
            if let Some(operation_id) = &webhook.operation_id {
                values.insert(operation_id.clone());
            }
            if let Some(summary) = &webhook.summary {
                values.insert(summary.clone());
            }
            if let Some(target) = &webhook.target {
                values.insert(target.clone());
            }
        }
        for (name, scheme) in &service.contract.security_schemes {
            values.insert(name.clone());
            match scheme {
                SecurityScheme::ApiKey { name, .. } => {
                    values.insert(name.clone());
                }
                SecurityScheme::Http {
                    scheme,
                    bearer_format,
                } => {
                    values.insert(scheme.clone());
                    values.extend(bearer_format.iter().cloned());
                }
                SecurityScheme::OAuth2
                | SecurityScheme::OpenIdConnect
                | SecurityScheme::MutualTls => {}
            }
        }
        for principal in service.principals.values() {
            values.insert(principal.name.clone());
            for (scheme, contract) in &principal.credentials {
                values.insert(scheme.clone());
                values.extend(contract.grants.iter().cloned());
            }
        }
        for profile in service.profiles.values() {
            values.insert(profile.name.clone());
            values.insert(profile.principal.clone());
            for (scheme, credential) in &profile.credentials {
                values.insert(scheme.clone());
                match credential {
                    ProfileCredential::Secret { value } => {
                        values.insert(value.clone());
                    }
                    ProfileCredential::Basic { username, password } => {
                        values.insert(username.clone());
                        values.insert(password.clone());
                    }
                }
            }
        }
        for setup in service.principal_setups.values() {
            values.insert(setup.principal.clone());
            values.insert(setup.operation_id.clone());
            values.extend(setup.description.iter().cloned());
            for collect in setup.collects.values() {
                values.insert(collect.name.clone());
                values.insert(collect.request_pointer.clone());
                values.extend(collect.description.iter().cloned());
            }
            for issue in setup.issues.values() {
                values.insert(issue.name.clone());
                values.extend(issue.description.iter().cloned());
                values.extend(issue.credential.iter().cloned());
                match &issue.source {
                    SetupIssueSource::Response { pointer } => {
                        values.insert(pointer.clone());
                    }
                    SetupIssueSource::Token { prefix } => {
                        values.insert(prefix.clone());
                    }
                }
            }
        }
        if let Some(signing) = &service.webhook_policy.signing {
            values.insert(signing.secret.clone());
            values.insert(signing.header.clone());
            values.insert(signing.timestamp_header.clone());
        }

        let values = values.into_iter().collect::<Vec<_>>();
        let ids = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let id = u32::try_from(index).map_err(|_| Error::Limit("strings"))?;
                Ok((value.clone(), id))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Self { values, ids })
    }

    fn id(&self, value: &str) -> Result<StringId> {
        self.ids
            .get(value)
            .copied()
            .ok_or(Error::Invalid("string table"))
    }

    fn optional(&self, value: Option<&str>) -> Result<Option<StringId>> {
        value.map(|value| self.id(value)).transpose()
    }
}

#[derive(Default)]
struct ValueArena {
    records: Vec<ValueRecord>,
}

impl ValueArena {
    fn push(&mut self, value: &Value, strings: &StringTable) -> Result<ValueId> {
        let record = match value {
            Value::Null => ValueRecord::Null,
            Value::Bool(value) => ValueRecord::Bool(*value),
            Value::Number(value) => {
                if let Some(value) = value.as_i64() {
                    ValueRecord::Signed(value)
                } else if let Some(value) = value.as_u64() {
                    ValueRecord::Unsigned(value)
                } else {
                    ValueRecord::Float(value.as_f64().ok_or(Error::Invalid("number"))?)
                }
            }
            Value::String(value) => ValueRecord::String(strings.id(value)?),
            Value::Array(values) => {
                let children = values
                    .iter()
                    .map(|value| self.push(value, strings))
                    .collect::<Result<Vec<_>>>()?;
                ValueRecord::Array(children)
            }
            Value::Object(values) => {
                let mut ordered = values.iter().collect::<Vec<_>>();
                ordered.sort_by(|(left, _), (right, _)| left.cmp(right));
                let fields = ordered
                    .into_iter()
                    .map(|(key, value)| Ok((strings.id(key)?, self.push(value, strings)?)))
                    .collect::<Result<Vec<_>>>()?;
                ValueRecord::Object(fields)
            }
        };
        let id = u32::try_from(self.records.len()).map_err(|_| Error::Limit("value nodes"))?;
        self.records.push(record);
        Ok(id)
    }
}

fn collect_value_strings(value: &Value, strings: &mut BTreeSet<String>) {
    match value {
        Value::String(value) => {
            strings.insert(value.clone());
        }
        Value::Array(values) => {
            for value in values {
                collect_value_strings(value, strings);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                strings.insert(key.clone());
                collect_value_strings(value, strings);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn collect_effect_action_strings(action: &EffectAction, strings: &mut BTreeSet<String>) {
    for (name, value) in &action.arguments {
        strings.insert(name.clone());
        collect_effect_value_strings(value, strings);
    }
}

fn collect_effect_value_strings(value: &EffectValue, strings: &mut BTreeSet<String>) {
    match value {
        EffectValue::Literal { value } => collect_value_strings(value, strings),
        EffectValue::Request { pointer } | EffectValue::Response { pointer } => {
            strings.insert(pointer.clone());
        }
        EffectValue::Array { items } => {
            for item in items {
                collect_effect_value_strings(item, strings);
            }
        }
        EffectValue::Object { fields } => {
            for (name, value) in fields {
                strings.insert(name.clone());
                collect_effect_value_strings(value, strings);
            }
        }
    }
}

fn compile_effect_action(
    action: &EffectAction,
    resources: &BTreeMap<ResourceRef, ResourceId>,
    strings: &StringTable,
    values: &mut ValueArena,
) -> Result<ActionRecord> {
    let resource =
        resources
            .get(&action.resource)
            .copied()
            .ok_or_else(|| Error::UnknownReference {
                kind: "effect resource",
                name: action.resource.to_string(),
            })?;
    let arguments = action
        .arguments
        .iter()
        .map(|(name, value)| {
            Ok((
                strings.id(name)?,
                compile_effect_value(value, strings, values)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ActionRecord(
        resource,
        encode_effect_action_kind(action.kind),
        arguments,
    ))
}

fn compile_effect_value(
    value: &EffectValue,
    strings: &StringTable,
    values: &mut ValueArena,
) -> Result<EffectValueRecord> {
    Ok(match value {
        EffectValue::Literal { value } => EffectValueRecord::Literal(values.push(value, strings)?),
        EffectValue::Request { pointer } => EffectValueRecord::Request(strings.id(pointer)?),
        EffectValue::Response { pointer } => EffectValueRecord::Response(strings.id(pointer)?),
        EffectValue::Array { items } => EffectValueRecord::Array(
            items
                .iter()
                .map(|item| compile_effect_value(item, strings, values))
                .collect::<Result<Vec<_>>>()?,
        ),
        EffectValue::Object { fields } => EffectValueRecord::Object(
            fields
                .iter()
                .map(|(name, value)| {
                    Ok((
                        strings.id(name)?,
                        compile_effect_value(value, strings, values)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?,
        ),
    })
}

fn decode_effect_action(
    action: &ActionRecord,
    strings: &[String],
    resources: &[ResourceRecord],
    values: &[ValueRecord],
) -> EffectAction {
    let resource = &resources[action.0 as usize];
    EffectAction {
        resource: ResourceRef::new(
            decode_resource_kind(resource.0),
            &strings[resource.1 as usize],
        ),
        kind: decode_effect_action_kind(action.1),
        arguments: action
            .2
            .iter()
            .map(|(name, value)| {
                (
                    strings[*name as usize].clone(),
                    decode_effect_value(value, strings, values),
                )
            })
            .collect(),
    }
}

fn decode_effect_value(
    value: &EffectValueRecord,
    strings: &[String],
    values: &[ValueRecord],
) -> EffectValue {
    match value {
        EffectValueRecord::Literal(value) => EffectValue::Literal {
            value: decode_value(*value, strings, values),
        },
        EffectValueRecord::Request(pointer) => EffectValue::Request {
            pointer: strings[*pointer as usize].clone(),
        },
        EffectValueRecord::Response(pointer) => EffectValue::Response {
            pointer: strings[*pointer as usize].clone(),
        },
        EffectValueRecord::Array(items) => EffectValue::Array {
            items: items
                .iter()
                .map(|item| decode_effect_value(item, strings, values))
                .collect(),
        },
        EffectValueRecord::Object(fields) => EffectValue::Object {
            fields: fields
                .iter()
                .map(|(name, value)| {
                    (
                        strings[*name as usize].clone(),
                        decode_effect_value(value, strings, values),
                    )
                })
                .collect(),
        },
    }
}

fn decode_webhook_emission(
    emission: &WebhookEmissionRecord,
    strings: &[String],
    values: &[ValueRecord],
) -> WebhookEmission {
    WebhookEmission {
        webhook: strings[emission.0 as usize].clone(),
        body: decode_effect_value(&emission.1, strings, values),
        target: emission
            .2
            .as_ref()
            .map(|target| decode_effect_value(target, strings, values)),
        after: emission
            .3
            .iter()
            .map(|dependency| strings[*dependency as usize].clone())
            .collect(),
        delay_milliseconds: emission.4,
    }
}

fn index_resources(service: &CompiledService) -> Result<BTreeMap<ResourceRef, ResourceId>> {
    service
        .resources
        .keys()
        .enumerate()
        .map(|(index, reference)| {
            let id = u32::try_from(index).map_err(|_| Error::Limit("resources"))?;
            Ok((reference.clone(), id))
        })
        .collect()
}

fn encode_method(method: &HttpMethod, strings: &StringTable) -> Result<(u8, Option<StringId>)> {
    Ok(match method {
        HttpMethod::Get => (0, None),
        HttpMethod::Put => (1, None),
        HttpMethod::Post => (2, None),
        HttpMethod::Delete => (3, None),
        HttpMethod::Options => (4, None),
        HttpMethod::Head => (5, None),
        HttpMethod::Patch => (6, None),
        HttpMethod::Trace => (7, None),
        HttpMethod::Query => (8, None),
        HttpMethod::Additional(method) => (9, Some(strings.id(method)?)),
    })
}

const fn encode_openapi_feature(feature: OpenApiFeature) -> u8 {
    match feature {
        OpenApiFeature::V3_0 => 0,
        OpenApiFeature::V3_1 => 1,
        OpenApiFeature::V3_2 => 2,
    }
}

const fn encode_heuristic_policy(policy: Policy) -> u8 {
    match policy {
        Policy::Off => 0,
        Policy::Conservative => 1,
        Policy::Standard => 2,
        Policy::Aggressive => 3,
    }
}

const fn encode_webhook_ordering(ordering: WebhookOrdering) -> u8 {
    match ordering {
        WebhookOrdering::BestEffort => 0,
        WebhookOrdering::Strict => 1,
    }
}

fn decode_webhook_ordering(ordering: u8) -> WebhookOrdering {
    match ordering {
        0 => WebhookOrdering::BestEffort,
        1 => WebhookOrdering::Strict,
        _ => unreachable!("artifact validation rejects unknown webhook ordering policies"),
    }
}

const fn encode_webhook_origin(origin: WebhookOrigin) -> u8 {
    match origin {
        WebhookOrigin::OpenApiCallback => 0,
        WebhookOrigin::OpenApiWebhook => 1,
        WebhookOrigin::ProviderAuthored => 2,
    }
}

fn decode_webhook_origin(origin: u8) -> WebhookOrigin {
    match origin {
        0 => WebhookOrigin::OpenApiCallback,
        1 => WebhookOrigin::OpenApiWebhook,
        2 => WebhookOrigin::ProviderAuthored,
        _ => unreachable!("artifact validation rejects unknown webhook origins"),
    }
}

fn decode_heuristic_policy(policy: u8) -> Policy {
    match policy {
        0 => Policy::Off,
        1 => Policy::Conservative,
        2 => Policy::Standard,
        3 => Policy::Aggressive,
        _ => unreachable!("artifact validation rejects unknown heuristic policies"),
    }
}

const fn encode_status(status: &ResponseStatus) -> (u8, u16) {
    match status {
        ResponseStatus::Exact(status) => (0, *status),
        ResponseStatus::Range(status) => (1, *status),
        ResponseStatus::Default => (2, 0),
    }
}

fn decode_status(kind: u8, status: u16) -> ResponseStatus {
    match kind {
        0 => ResponseStatus::Exact(status),
        1 => ResponseStatus::Range(status),
        2 => ResponseStatus::Default,
        _ => unreachable!("artifact validation rejects unknown response status kinds"),
    }
}

const fn encode_origin(origin: BehaviorOrigin) -> u8 {
    match origin {
        BehaviorOrigin::OpenApi => 0,
        BehaviorOrigin::Hcl => 1,
    }
}

fn decode_origin(origin: u8) -> BehaviorOrigin {
    match origin {
        0 => BehaviorOrigin::OpenApi,
        1 => BehaviorOrigin::Hcl,
        _ => unreachable!("artifact validation rejects unknown behavior origins"),
    }
}

const fn encode_effect_origin(origin: EffectOrigin) -> u8 {
    match origin {
        EffectOrigin::ProviderAuthored => 0,
        EffectOrigin::ConventionInferred => 1,
    }
}

fn decode_effect_origin(origin: u8) -> EffectOrigin {
    match origin {
        0 => EffectOrigin::ProviderAuthored,
        1 => EffectOrigin::ConventionInferred,
        _ => unreachable!("artifact validation rejects unknown effect origins"),
    }
}

const fn encode_outcome_provenance(provenance: OutcomeProvenance) -> u8 {
    match provenance {
        OutcomeProvenance::OpenApiDerived => 0,
        OutcomeProvenance::ProviderAuthored => 1,
        OutcomeProvenance::RecordedReviewed => 2,
        OutcomeProvenance::SchemaSynthesized => 3,
    }
}

fn decode_outcome_provenance(provenance: u8) -> OutcomeProvenance {
    match provenance {
        0 => OutcomeProvenance::OpenApiDerived,
        1 => OutcomeProvenance::ProviderAuthored,
        2 => OutcomeProvenance::RecordedReviewed,
        3 => OutcomeProvenance::SchemaSynthesized,
        _ => unreachable!("artifact validation rejects unknown outcome provenance"),
    }
}

const fn encode_description_kind(kind: DescriptionKind) -> u8 {
    match kind {
        DescriptionKind::Service => 0,
        DescriptionKind::Behavior => 1,
        DescriptionKind::Outcome => 2,
        DescriptionKind::ErrorSet => 3,
        DescriptionKind::ProtocolError => 4,
        DescriptionKind::Scenario => 5,
        DescriptionKind::Backend => 6,
        DescriptionKind::AsyncOperation => 7,
        DescriptionKind::Fault => 8,
        DescriptionKind::FaultProfile => 9,
        DescriptionKind::Effect => 10,
        DescriptionKind::Webhook => 11,
        DescriptionKind::Principal => 12,
        DescriptionKind::Profile => 13,
        DescriptionKind::PrincipalSetup => 14,
    }
}

fn decode_description_kind(kind: u8) -> DescriptionKind {
    match kind {
        0 => DescriptionKind::Service,
        1 => DescriptionKind::Behavior,
        2 => DescriptionKind::Outcome,
        3 => DescriptionKind::ErrorSet,
        4 => DescriptionKind::ProtocolError,
        5 => DescriptionKind::Scenario,
        6 => DescriptionKind::Backend,
        7 => DescriptionKind::AsyncOperation,
        8 => DescriptionKind::Fault,
        9 => DescriptionKind::FaultProfile,
        10 => DescriptionKind::Effect,
        11 => DescriptionKind::Webhook,
        12 => DescriptionKind::Principal,
        13 => DescriptionKind::Profile,
        14 => DescriptionKind::PrincipalSetup,
        _ => unreachable!("artifact validation rejects unknown description kinds"),
    }
}

const fn encode_setup_value_type(value_type: SetupValueType) -> u8 {
    match value_type {
        SetupValueType::String => 0,
        SetupValueType::Url => 1,
        SetupValueType::Boolean => 2,
        SetupValueType::Integer => 3,
        SetupValueType::Json => 4,
    }
}

fn decode_setup_value_type(value_type: u8) -> SetupValueType {
    match value_type {
        0 => SetupValueType::String,
        1 => SetupValueType::Url,
        2 => SetupValueType::Boolean,
        3 => SetupValueType::Integer,
        4 => SetupValueType::Json,
        _ => unreachable!("artifact validation rejects setup value types"),
    }
}

const fn encode_setup_binding(binding: SetupBinding) -> u8 {
    match binding {
        SetupBinding::WebhookTarget => 0,
        SetupBinding::WebhookSigningSecret => 1,
    }
}

fn decode_setup_binding(binding: u8) -> SetupBinding {
    match binding {
        0 => SetupBinding::WebhookTarget,
        1 => SetupBinding::WebhookSigningSecret,
        _ => unreachable!("artifact validation rejects setup bindings"),
    }
}

const fn encode_effect_action_kind(kind: EffectActionKind) -> u8 {
    match kind {
        EffectActionKind::DbPut => 0,
        EffectActionKind::DbDelete => 1,
        EffectActionKind::CacheSet => 2,
        EffectActionKind::CacheDelete => 3,
        EffectActionKind::CacheIncrement => 4,
        EffectActionKind::CacheCompareAndSet => 5,
        EffectActionKind::QueuePublish => 6,
        EffectActionKind::MailSend => 7,
        EffectActionKind::ObjectStorePut => 8,
        EffectActionKind::ObjectStoreDelete => 9,
        EffectActionKind::EventBusPublish => 10,
        EffectActionKind::SchedulerSchedule => 11,
        EffectActionKind::SchedulerCancel => 12,
        EffectActionKind::HttpRegister => 13,
        EffectActionKind::HttpCall => 14,
    }
}

fn decode_effect_action_kind(kind: u8) -> EffectActionKind {
    match kind {
        0 => EffectActionKind::DbPut,
        1 => EffectActionKind::DbDelete,
        2 => EffectActionKind::CacheSet,
        3 => EffectActionKind::CacheDelete,
        4 => EffectActionKind::CacheIncrement,
        5 => EffectActionKind::CacheCompareAndSet,
        6 => EffectActionKind::QueuePublish,
        7 => EffectActionKind::MailSend,
        8 => EffectActionKind::ObjectStorePut,
        9 => EffectActionKind::ObjectStoreDelete,
        10 => EffectActionKind::EventBusPublish,
        11 => EffectActionKind::SchedulerSchedule,
        12 => EffectActionKind::SchedulerCancel,
        13 => EffectActionKind::HttpRegister,
        14 => EffectActionKind::HttpCall,
        _ => unreachable!("artifact validation rejects unknown effect action kinds"),
    }
}

const fn encode_fault_phase(phase: FaultPhase) -> u8 {
    match phase {
        FaultPhase::BeforeResponse => 0,
        FaultPhase::AfterCommit => 1,
    }
}

fn decode_fault_phase(phase: u8) -> FaultPhase {
    match phase {
        0 => FaultPhase::BeforeResponse,
        1 => FaultPhase::AfterCommit,
        _ => unreachable!("artifact validation rejects unknown fault phases"),
    }
}

const fn encode_fault_action(action: &FaultAction) -> (u8, u64) {
    match action {
        FaultAction::Delay { milliseconds } => (0, *milliseconds),
        FaultAction::Disconnect => (1, 0),
    }
}

fn decode_fault_action(action: u8, value: u64) -> FaultAction {
    match action {
        0 => FaultAction::Delay {
            milliseconds: value,
        },
        1 => FaultAction::Disconnect,
        _ => unreachable!("artifact validation rejects unknown fault actions"),
    }
}

const fn encode_resource_kind(kind: ResourceKind) -> u8 {
    match kind {
        ResourceKind::Db => 0,
        ResourceKind::Cache => 1,
        ResourceKind::Queue => 2,
        ResourceKind::Mail => 3,
        ResourceKind::ObjectStore => 4,
        ResourceKind::EventBus => 5,
        ResourceKind::Scheduler => 6,
        ResourceKind::Http => 7,
    }
}

fn decode_resource_kind(kind: u8) -> ResourceKind {
    match kind {
        0 => ResourceKind::Db,
        1 => ResourceKind::Cache,
        2 => ResourceKind::Queue,
        3 => ResourceKind::Mail,
        4 => ResourceKind::ObjectStore,
        5 => ResourceKind::EventBus,
        6 => ResourceKind::Scheduler,
        7 => ResourceKind::Http,
        _ => unreachable!("artifact validation rejects unknown resource kinds"),
    }
}

const fn encode_parameter_location(location: ParameterLocation) -> u8 {
    match location {
        ParameterLocation::Path => 0,
        ParameterLocation::Query => 1,
        ParameterLocation::Header => 2,
        ParameterLocation::Cookie => 3,
    }
}

fn decode_parameter_location(location: u8) -> ParameterLocation {
    match location {
        0 => ParameterLocation::Path,
        1 => ParameterLocation::Query,
        2 => ParameterLocation::Header,
        3 => ParameterLocation::Cookie,
        _ => unreachable!("artifact validation rejects unknown parameter locations"),
    }
}

fn encode_security_scheme(
    name: &str,
    scheme: &SecurityScheme,
    strings: &StringTable,
) -> Result<SecuritySchemeRecord> {
    let record = match scheme {
        SecurityScheme::ApiKey {
            location,
            name: key,
        } => SecuritySchemeRecord(
            strings.id(name)?,
            0,
            encode_parameter_location(*location),
            Some(strings.id(key)?),
            None,
            None,
        ),
        SecurityScheme::Http {
            scheme,
            bearer_format,
        } => SecuritySchemeRecord(
            strings.id(name)?,
            1,
            0,
            None,
            Some(strings.id(scheme)?),
            strings.optional(bearer_format.as_deref())?,
        ),
        SecurityScheme::OAuth2 => SecuritySchemeRecord(strings.id(name)?, 2, 0, None, None, None),
        SecurityScheme::OpenIdConnect => {
            SecuritySchemeRecord(strings.id(name)?, 3, 0, None, None, None)
        }
        SecurityScheme::MutualTls => {
            SecuritySchemeRecord(strings.id(name)?, 4, 0, None, None, None)
        }
    };
    Ok(record)
}

fn decode_security_scheme(record: &SecuritySchemeRecord, strings: &[String]) -> SecurityScheme {
    match record.1 {
        0 => {
            let Some(name) = record.3 else {
                unreachable!("artifact validation requires an API key name")
            };
            SecurityScheme::ApiKey {
                location: decode_parameter_location(record.2),
                name: strings[name as usize].clone(),
            }
        }
        1 => {
            let Some(scheme) = record.4 else {
                unreachable!("artifact validation requires an HTTP scheme")
            };
            SecurityScheme::Http {
                scheme: strings[scheme as usize].clone(),
                bearer_format: record.5.map(|id| strings[id as usize].clone()),
            }
        }
        2 => SecurityScheme::OAuth2,
        3 => SecurityScheme::OpenIdConnect,
        4 => SecurityScheme::MutualTls,
        _ => unreachable!("artifact validation rejects unknown security schemes"),
    }
}

const fn encode_profile_status(status: ProfileStatus) -> u8 {
    match status {
        ProfileStatus::Active => 0,
        ProfileStatus::Revoked => 1,
        ProfileStatus::Expired => 2,
    }
}

fn decode_profile_status(status: u8) -> ProfileStatus {
    match status {
        0 => ProfileStatus::Active,
        1 => ProfileStatus::Revoked,
        2 => ProfileStatus::Expired,
        _ => unreachable!("artifact validation rejects unknown profile statuses"),
    }
}

fn validate_index(id: u32, length: usize, field: &'static str) -> Result<()> {
    if usize::try_from(id).map_or(true, |id| id >= length) {
        return Err(Error::Invalid(field));
    }
    Ok(())
}

fn validate_optional_index(id: Option<u32>, length: usize, field: &'static str) -> Result<()> {
    if let Some(id) = id {
        validate_index(id, length, field)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rejects_dangling_indexes() {
        let payload = Payload(
            0,
            vec![
                "0.1.0".to_owned(),
                "Orders".to_owned(),
                "1.0.0".to_owned(),
                "3.2.1".to_owned(),
                "/orders".to_owned(),
            ],
            ServiceRecord(
                1,
                2,
                None,
                3,
                2,
                2,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                0,
                100_000,
                5_000,
                8,
                None,
                None,
                None,
            ),
            vec![OperationRecord(
                99,
                2,
                None,
                4,
                None,
                None,
                None,
                Vec::new(),
                0,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            SchemaCatalog::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        assert!(matches!(
            payload.validate(),
            Err(Error::Invalid("operation key"))
        ));
    }

    #[test]
    fn validation_rejects_incompatible_fault_records() {
        let payload = Payload(
            0,
            vec![
                "0.1.0".to_owned(),
                "Orders".to_owned(),
                "1.0.0".to_owned(),
                "3.2.1".to_owned(),
                "early_disconnect".to_owned(),
            ],
            ServiceRecord(
                1,
                2,
                None,
                3,
                2,
                2,
                Vec::new(),
                Vec::new(),
                vec![FaultRecord(4, 0, 1, 0)],
                Vec::new(),
                Vec::new(),
                0,
                100_000,
                5_000,
                8,
                None,
                None,
                None,
            ),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            SchemaCatalog::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        assert!(matches!(
            payload.validate(),
            Err(Error::Invalid("fault phase and action"))
        ));
    }
}
