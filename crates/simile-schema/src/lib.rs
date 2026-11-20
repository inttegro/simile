#![forbid(unsafe_code)]

mod compile;
mod error;
mod relation;
mod soundness;
mod synthesize;
mod validate;

use std::collections::{BTreeMap, BTreeSet};

pub use compile::{BodySchemaCompiler, RelationParameter};
pub use error::{CompileError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use simile_heuristics::Plan as HeuristicPlan;
use simile_spec::ParameterLocation;
pub use synthesize::SynthesisError;
pub use validate::{ValidationIssue, ValidationRule};

type SchemaId = u32;

/// Validated values from every public location of one operation request.
#[derive(Clone, Debug, Default)]
pub struct RequestValues<'a> {
    body: Option<&'a Value>,
    parameters: BTreeMap<(ParameterLocation, String), &'a Value>,
}

impl<'a> RequestValues<'a> {
    #[must_use]
    pub const fn new(body: Option<&'a Value>) -> Self {
        Self {
            body,
            parameters: BTreeMap::new(),
        }
    }

    pub fn insert_parameter(
        &mut self,
        location: ParameterLocation,
        name: impl Into<String>,
        value: &'a Value,
    ) {
        let name = if location == ParameterLocation::Header {
            name.into().to_ascii_lowercase()
        } else {
            name.into()
        };
        self.parameters.insert((location, name), value);
    }

    #[must_use]
    pub const fn body(&self) -> Option<&'a Value> {
        self.body
    }

    #[must_use]
    pub fn parameter(&self, location: ParameterLocation, name: &str) -> Option<&'a Value> {
        let name = if location == ParameterLocation::Header {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        };
        self.parameters.get(&(location, name)).copied()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct SchemaRef(SchemaId);

/// A request-body contract reduced to the information needed by the runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestBodyPlan {
    required: bool,
    content: BTreeMap<String, MediaTypePlan>,
}

impl RequestBodyPlan {
    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.required
    }

    pub fn media_types(&self) -> impl Iterator<Item = &str> {
        self.content.keys().map(String::as_str)
    }

    /// Match a Content-Type value, ignoring parameters and honoring OpenAPI wildcards.
    #[must_use]
    pub fn select(&self, content_type: &str) -> Option<SelectedMediaType<'_>> {
        select_media(&self.content, content_type)
    }

    #[doc(hidden)]
    #[must_use]
    fn is_well_formed(&self, schemas: &SchemaCatalog) -> bool {
        content_is_well_formed(&self.content, schemas)
    }
}

/// A response-body contract reduced to the information needed by the runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseBodyPlan {
    content: BTreeMap<String, MediaTypePlan>,
}

/// A non-body JSON value schema compiled into the shared schema graph.
///
/// HTTP adapters use this for path, query, header, and cookie parameters. The
/// plan deliberately shares the same validator and synthesis semantics as
/// request and response bodies instead of maintaining a weaker second engine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValueSchemaPlan {
    schema: SchemaRef,
}

impl ValueSchemaPlan {
    #[doc(hidden)]
    #[must_use]
    fn is_well_formed(&self, schemas: &SchemaCatalog) -> bool {
        schemas.contains(self.schema)
    }
}

impl ResponseBodyPlan {
    pub fn media_types(&self) -> impl Iterator<Item = &str> {
        self.content.keys().map(String::as_str)
    }

    /// Match a Content-Type value, ignoring parameters and honoring OpenAPI wildcards.
    #[must_use]
    pub fn select(&self, content_type: &str) -> Option<SelectedMediaType<'_>> {
        select_media(&self.content, content_type)
    }

    #[doc(hidden)]
    #[must_use]
    fn is_well_formed(&self, schemas: &SchemaCatalog) -> bool {
        content_is_well_formed(&self.content, schemas)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct MediaTypePlan {
    #[serde(rename = "s", skip_serializing_if = "Option::is_none")]
    schema: Option<SchemaRef>,
    #[serde(rename = "e", skip_serializing_if = "Option::is_none")]
    example: Option<Value>,
    #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
    relation: Option<relation::Plan>,
}

fn select_media<'a>(
    content: &'a BTreeMap<String, MediaTypePlan>,
    content_type: &str,
) -> Option<SelectedMediaType<'a>> {
    let essence = content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if essence.is_empty() {
        return None;
    }
    let exact = content.get_key_value(&essence);
    let wildcard = essence
        .split_once('/')
        .and_then(|(kind, _)| content.get_key_value(&format!("{kind}/*")));
    let fallback = content.get_key_value("*/*");
    exact
        .or(wildcard)
        .or(fallback)
        .map(|(declared, plan)| SelectedMediaType {
            requested: essence,
            declared,
            schema: plan.schema,
            example: plan.example.as_ref(),
            relation: plan.relation.as_ref(),
        })
}

fn content_is_well_formed(
    content: &BTreeMap<String, MediaTypePlan>,
    schemas: &SchemaCatalog,
) -> bool {
    !content.is_empty()
        && content.keys().all(|media_type| !media_type.is_empty())
        && content.values().all(|media| {
            media.schema.is_none_or(|schema| schemas.contains(schema))
                && media.relation.as_ref().is_none_or(|relation| {
                    media
                        .schema
                        .is_some_and(|schema| relation.is_well_formed(schemas, schema))
                })
        })
}

/// The declared media type and optional schema selected for an incoming body.
#[derive(Clone, Debug)]
pub struct SelectedMediaType<'a> {
    requested: String,
    declared: &'a str,
    schema: Option<SchemaRef>,
    example: Option<&'a Value>,
    relation: Option<&'a relation::Plan>,
}

impl<'a> SelectedMediaType<'a> {
    #[must_use]
    pub fn requested(&self) -> &str {
        &self.requested
    }

    #[must_use]
    pub const fn declared(&self) -> &'a str {
        self.declared
    }

    #[must_use]
    const fn schema(&self) -> Option<SchemaRef> {
        self.schema
    }

    const fn relation(&self) -> Option<&'a relation::Plan> {
        self.relation
    }
}

/// The shared schema graph embedded once in a compiled artifact.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SchemaCatalog {
    nodes: Vec<SchemaNode>,
}

impl SchemaCatalog {
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    #[must_use]
    pub fn heuristic_rules(&self) -> usize {
        self.nodes.iter().map(|node| node.heuristics.len()).sum()
    }

    #[must_use]
    fn validate(&self, root: SchemaRef, value: &Value) -> Vec<ValidationIssue> {
        validate::value(self, root, value)
    }

    #[doc(hidden)]
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        let length = self.nodes.len();
        self.nodes.iter().enumerate().all(|(index, node)| {
            node.properties.values().all(|id| valid_id(*id, length))
                && node.items.is_none_or(|id| valid_id(id, length))
                && node.all_of.iter().all(|id| valid_id(*id, length))
                && node.any_of.iter().all(|id| valid_id(*id, length))
                && node.one_of.iter().all(|id| valid_id(*id, length))
                && node.not.is_none_or(|id| valid_id(id, length))
                && match node.additional {
                    AdditionalProperties::Schema(id) => valid_id(id, length),
                    AdditionalProperties::Allow | AdditionalProperties::Deny => true,
                }
                && u32::try_from(index)
                    .is_ok_and(|id| soundness::is_well_formed(self, id, &node.constraints))
                && node.heuristics.is_well_formed()
        })
    }

    fn contains(&self, schema: SchemaRef) -> bool {
        valid_id(schema.0, self.nodes.len())
    }
}

/// A selected media type bound to the shared compiled schema catalog.
#[derive(Clone, Debug)]
pub struct BoundMediaType<'a> {
    selected: SelectedMediaType<'a>,
    schemas: &'a SchemaCatalog,
}

/// A parameter schema bound to the artifact's shared schema catalog.
#[derive(Clone, Copy, Debug)]
pub struct BoundValueSchema<'a> {
    plan: &'a ValueSchemaPlan,
    schemas: &'a SchemaCatalog,
}

impl<'a> BoundValueSchema<'a> {
    #[must_use]
    pub const fn new(plan: &'a ValueSchemaPlan, schemas: &'a SchemaCatalog) -> Self {
        Self { plan, schemas }
    }

    #[must_use]
    pub fn validate(&self, value: &Value) -> Vec<ValidationIssue> {
        self.schemas.validate(self.plan.schema, value)
    }

    pub fn synthesize_fuzz(&self, seed: u64) -> std::result::Result<Value, SynthesisError> {
        self.schemas.synthesize_fuzz(self.plan.schema, seed)
    }

    /// Names declared by an object-shaped parameter schema.
    ///
    /// OpenAPI's exploded `form` encoding places object properties beside the
    /// parameter instead of beneath its name. Transport-neutral decoding needs
    /// this small amount of shape information to collect only the fields that
    /// belong to that parameter.
    pub fn property_names(&self) -> impl Iterator<Item = &str> {
        self.schemas
            .nodes
            .get(self.plan.schema.0 as usize)
            .into_iter()
            .flat_map(|node| node.properties.keys().map(String::as_str))
    }

    #[doc(hidden)]
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        self.plan.is_well_formed(self.schemas)
    }
}

impl<'a> BoundMediaType<'a> {
    #[must_use]
    pub fn requested(&self) -> &str {
        self.selected.requested()
    }

    #[must_use]
    pub const fn declared(&self) -> &'a str {
        self.selected.declared()
    }

    #[must_use]
    pub fn validate(&self, value: &Value) -> Vec<ValidationIssue> {
        self.selected
            .schema()
            .map_or_else(Vec::new, |schema| self.schemas.validate(schema, value))
    }

    /// Validate one direct object property against its compiled schema.
    ///
    /// Form codecs receive text for every ordinary field. The transport
    /// adapter uses this narrow shape query to choose a schema-valid scalar
    /// representation while preserving string fields such as postal codes.
    pub fn validate_property(&self, name: &str, value: &Value) -> Option<Vec<ValidationIssue>> {
        let schema = self.selected.schema()?;
        let node = self.schemas.nodes.get(schema.0 as usize)?;
        let property = *node.properties.get(name)?;
        Some(self.schemas.validate(SchemaRef(property), value))
    }

    /// Whether this response media type declares a compiled request-to-response proof plan.
    #[must_use]
    pub const fn is_request_constrained(&self) -> bool {
        self.selected.relation().is_some()
    }

    /// Produce a deterministic JSON value proven sound by this media type's compiled contract.
    pub fn synthesize(&self) -> std::result::Result<Value, SynthesisError> {
        if let Some(example) = self.selected.example {
            if self.validate(example).is_empty() {
                return Ok(example.clone());
            }
        }
        self.selected.schema().map_or_else(
            || Ok(Value::Object(serde_json::Map::new())),
            |schema| self.schemas.synthesize(schema),
        )
    }

    /// Produce a deterministic valid value that includes finite optional fields where possible.
    pub fn synthesize_representative(&self) -> std::result::Result<Value, SynthesisError> {
        if let Some(example) = self.selected.example {
            if self.validate(example).is_empty() {
                return Ok(example.clone());
            }
        }
        self.selected.schema().map_or_else(
            || Ok(Value::Object(serde_json::Map::new())),
            |schema| self.schemas.synthesize_representative(schema),
        )
    }

    /// Produce a contract-sound value whose unconstrained fields vary with `seed`.
    pub fn synthesize_fuzz(&self, seed: u64) -> std::result::Result<Value, SynthesisError> {
        self.selected.schema().map_or_else(
            || Ok(Value::Object(serde_json::Map::new())),
            |schema| self.schemas.synthesize_fuzz(schema, seed),
        )
    }

    /// Produce a varying valid value while omitting unrelated optional properties.
    pub fn synthesize_fuzz_minimal(&self, seed: u64) -> std::result::Result<Value, SynthesisError> {
        self.selected.schema().map_or_else(
            || Ok(Value::Object(serde_json::Map::new())),
            |schema| self.schemas.synthesize_fuzz_minimal(schema, seed),
        )
    }

    /// Synthesize a valid value and project compatible fields from a request body.
    pub fn synthesize_from(&self, request: &Value) -> std::result::Result<Value, SynthesisError> {
        let baseline = self.synthesize()?;
        self.project_from(baseline, &RequestValues::new(Some(request)), true)
    }

    /// Produce a representative valid value and project compatible request fields into it.
    pub fn synthesize_representative_from(
        &self,
        request: &Value,
    ) -> std::result::Result<Value, SynthesisError> {
        let baseline = self.synthesize_representative()?;
        self.project_from(baseline, &RequestValues::new(Some(request)), true)
    }

    /// Produce a varying valid value and project compatible request fields into it.
    pub fn synthesize_fuzz_from(
        &self,
        request: &Value,
        seed: u64,
    ) -> std::result::Result<Value, SynthesisError> {
        let baseline = self.synthesize_fuzz_minimal(seed)?;
        self.project_from(baseline, &RequestValues::new(Some(request)), true)
    }

    /// Produce a varying valid response from every validated request location.
    pub fn synthesize_fuzz_from_request(
        &self,
        request: &RequestValues<'_>,
        seed: u64,
    ) -> std::result::Result<Value, SynthesisError> {
        let baseline = self.synthesize_fuzz_minimal(seed)?;
        self.project_from(baseline, request, true)
    }

    /// Apply only explicitly declared request relations to a provider-authored exact body.
    pub fn project_exact_from(
        &self,
        body: &Value,
        request: &Value,
    ) -> std::result::Result<Value, SynthesisError> {
        self.project_from(body.clone(), &RequestValues::new(Some(request)), false)
    }

    /// Apply explicit request relations to an exact body using every request location.
    pub fn project_exact_from_request(
        &self,
        body: &Value,
        request: &RequestValues<'_>,
    ) -> std::result::Result<Value, SynthesisError> {
        self.project_from(body.clone(), request, false)
    }

    fn project_from(
        &self,
        baseline: Value,
        request: &RequestValues<'_>,
        project_same_paths: bool,
    ) -> std::result::Result<Value, SynthesisError> {
        let Some(schema) = self.selected.schema() else {
            return Ok(baseline);
        };
        let mut candidate = if project_same_paths {
            request.body().map_or_else(
                || baseline.clone(),
                |body| self.schemas.project(schema, &baseline, body),
            )
        } else {
            baseline
        };
        let Some(relation) = self.selected.relation() else {
            return Ok(candidate);
        };
        relation.apply(self.schemas, schema, request, &mut candidate)?;
        // Why: response-local equations and implications may depend on a projected field; repair
        // them before proving the cross-message relation, then reapply the authoritative request.
        self.schemas.establish(schema, &mut candidate);
        relation.apply(self.schemas, schema, request, &mut candidate)?;
        let issues = self.schemas.validate(schema, &candidate);
        if !issues.is_empty() {
            return Err(SynthesisError::Exhausted { issues });
        }
        relation.prove(request, &candidate)?;
        Ok(candidate)
    }
}

/// A request-body plan bound to the shared schema catalog that owns its roots.
#[derive(Clone, Copy, Debug)]
pub struct BoundRequestBody<'a> {
    plan: &'a RequestBodyPlan,
    schemas: &'a SchemaCatalog,
}

/// A response-body plan bound to the shared compiled schema catalog that owns its roots.
#[derive(Clone, Copy, Debug)]
pub struct BoundResponseBody<'a> {
    plan: &'a ResponseBodyPlan,
    schemas: &'a SchemaCatalog,
}

impl<'a> BoundResponseBody<'a> {
    #[doc(hidden)]
    #[must_use]
    pub const fn new(plan: &'a ResponseBodyPlan, schemas: &'a SchemaCatalog) -> Self {
        Self { plan, schemas }
    }

    pub fn media_types(&self) -> impl Iterator<Item = &str> {
        self.plan.media_types()
    }

    #[must_use]
    pub fn select(&self, content_type: &str) -> Option<BoundMediaType<'a>> {
        self.plan
            .select(content_type)
            .map(|selected| BoundMediaType {
                selected,
                schemas: self.schemas,
            })
    }

    #[doc(hidden)]
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        self.plan.is_well_formed(self.schemas)
    }
}

impl<'a> BoundRequestBody<'a> {
    #[doc(hidden)]
    #[must_use]
    pub const fn new(plan: &'a RequestBodyPlan, schemas: &'a SchemaCatalog) -> Self {
        Self { plan, schemas }
    }

    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.plan.is_required()
    }

    pub fn media_types(&self) -> impl Iterator<Item = &str> {
        self.plan.media_types()
    }

    #[must_use]
    pub fn select(&self, content_type: &str) -> Option<BoundMediaType<'a>> {
        self.plan
            .select(content_type)
            .map(|selected| BoundMediaType {
                selected,
                schemas: self.schemas,
            })
    }

    #[doc(hidden)]
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        self.plan.is_well_formed(self.schemas)
    }
}

fn valid_id(id: SchemaId, length: usize) -> bool {
    usize::try_from(id).is_ok_and(|id| id < length)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct SchemaNode {
    #[serde(rename = "a", skip_serializing_if = "Option::is_none")]
    allowed: Option<bool>,
    #[serde(rename = "t", skip_serializing_if = "BTreeSet::is_empty")]
    types: BTreeSet<JsonType>,
    #[serde(rename = "e", skip_serializing_if = "Option::is_none")]
    enum_values: Option<Vec<Value>>,
    #[serde(rename = "c", skip_serializing_if = "Option::is_none")]
    const_value: Option<Value>,
    #[serde(rename = "h", skip_serializing_if = "Option::is_none")]
    hint: Option<Value>,
    #[serde(rename = "f", skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(rename = "P", skip_serializing_if = "Option::is_none")]
    pattern: Option<String>,
    #[serde(rename = "p", skip_serializing_if = "BTreeMap::is_empty")]
    properties: BTreeMap<String, SchemaId>,
    #[serde(rename = "r", skip_serializing_if = "BTreeSet::is_empty")]
    required: BTreeSet<String>,
    #[serde(rename = "x", skip_serializing_if = "additional_is_allow")]
    additional: AdditionalProperties,
    #[serde(rename = "i", skip_serializing_if = "Option::is_none")]
    items: Option<SchemaId>,
    #[serde(rename = "l", skip_serializing_if = "Vec::is_empty")]
    all_of: Vec<SchemaId>,
    #[serde(rename = "y", skip_serializing_if = "Vec::is_empty")]
    any_of: Vec<SchemaId>,
    #[serde(rename = "o", skip_serializing_if = "Vec::is_empty")]
    one_of: Vec<SchemaId>,
    #[serde(rename = "n", skip_serializing_if = "Option::is_none")]
    not: Option<SchemaId>,
    #[serde(rename = "m", skip_serializing_if = "Option::is_none")]
    minimum: Option<NumberBound>,
    #[serde(rename = "M", skip_serializing_if = "Option::is_none")]
    maximum: Option<NumberBound>,
    #[serde(rename = "d", skip_serializing_if = "Option::is_none")]
    multiple_of: Option<f64>,
    #[serde(rename = "s", skip_serializing_if = "Option::is_none")]
    min_length: Option<usize>,
    #[serde(rename = "S", skip_serializing_if = "Option::is_none")]
    max_length: Option<usize>,
    #[serde(rename = "q", skip_serializing_if = "Option::is_none")]
    min_items: Option<usize>,
    #[serde(rename = "Q", skip_serializing_if = "Option::is_none")]
    max_items: Option<usize>,
    #[serde(rename = "u", skip_serializing_if = "is_false")]
    unique_items: bool,
    #[serde(rename = "z", skip_serializing_if = "Option::is_none")]
    min_properties: Option<usize>,
    #[serde(rename = "Z", skip_serializing_if = "Option::is_none")]
    max_properties: Option<usize>,
    #[serde(rename = "C", skip_serializing_if = "Vec::is_empty")]
    constraints: Vec<soundness::Constraint>,
    #[serde(rename = "H", skip_serializing_if = "HeuristicPlan::is_empty")]
    heuristics: HeuristicPlan,
}

impl Default for SchemaNode {
    fn default() -> Self {
        Self {
            allowed: None,
            types: BTreeSet::new(),
            enum_values: None,
            const_value: None,
            hint: None,
            format: None,
            pattern: None,
            properties: BTreeMap::new(),
            required: BTreeSet::new(),
            additional: AdditionalProperties::Allow,
            items: None,
            all_of: Vec::new(),
            any_of: Vec::new(),
            one_of: Vec::new(),
            not: None,
            minimum: None,
            maximum: None,
            multiple_of: None,
            min_length: None,
            max_length: None,
            min_items: None,
            max_items: None,
            unique_items: false,
            min_properties: None,
            max_properties: None,
            constraints: Vec::new(),
            heuristics: HeuristicPlan::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
enum JsonType {
    Null,
    Boolean,
    Object,
    Array,
    Number,
    Integer,
    String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct NumberBound {
    value: f64,
    exclusive: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
enum AdditionalProperties {
    #[default]
    Allow,
    Deny,
    Schema(SchemaId),
}

fn additional_is_allow(value: &AdditionalProperties) -> bool {
    *value == AdditionalProperties::Allow
}

const fn is_false(value: &bool) -> bool {
    !*value
}
