#![forbid(unsafe_code)]

mod compile;
mod error;
mod synthesize;
mod validate;

use std::collections::{BTreeMap, BTreeSet};

pub use compile::BodySchemaCompiler;
pub use error::{CompileError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use synthesize::SynthesisError;
pub use validate::{ValidationIssue, ValidationRule};

type SchemaId = u32;

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
        })
}

fn content_is_well_formed(
    content: &BTreeMap<String, MediaTypePlan>,
    schemas: &SchemaCatalog,
) -> bool {
    !content.is_empty()
        && content.keys().all(|media_type| !media_type.is_empty())
        && content
            .values()
            .all(|media| media.schema.is_none_or(|schema| schemas.contains(schema)))
}

/// The declared media type and optional schema selected for an incoming body.
#[derive(Clone, Debug)]
pub struct SelectedMediaType<'a> {
    requested: String,
    declared: &'a str,
    schema: Option<SchemaRef>,
    example: Option<&'a Value>,
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
    fn validate(&self, root: SchemaRef, value: &Value) -> Vec<ValidationIssue> {
        validate::value(self, root, value)
    }

    #[doc(hidden)]
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        let length = self.nodes.len();
        self.nodes.iter().all(|node| {
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

    /// Produce a deterministic JSON value accepted by this media type's schema.
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

    /// Produce a schema-valid value whose unconstrained fields vary with `seed`.
    pub fn synthesize_fuzz(&self, seed: u64) -> std::result::Result<Value, SynthesisError> {
        self.selected.schema().map_or_else(
            || Ok(Value::Object(serde_json::Map::new())),
            |schema| self.schemas.synthesize_fuzz(schema, seed),
        )
    }

    /// Synthesize a valid value and project compatible fields from a request body.
    pub fn synthesize_from(&self, request: &Value) -> std::result::Result<Value, SynthesisError> {
        let baseline = self.synthesize()?;
        Ok(match self.selected.schema() {
            Some(schema) => self.schemas.project(schema, &baseline, request),
            None => baseline,
        })
    }

    /// Produce a representative valid value and project compatible request fields into it.
    pub fn synthesize_representative_from(
        &self,
        request: &Value,
    ) -> std::result::Result<Value, SynthesisError> {
        let baseline = self.synthesize_representative()?;
        Ok(match self.selected.schema() {
            Some(schema) => self.schemas.project(schema, &baseline, request),
            None => baseline,
        })
    }

    /// Produce a varying valid value and project compatible request fields into it.
    pub fn synthesize_fuzz_from(
        &self,
        request: &Value,
        seed: u64,
    ) -> std::result::Result<Value, SynthesisError> {
        let baseline = self.synthesize_fuzz(seed)?;
        Ok(match self.selected.schema() {
            Some(schema) => self.schemas.project(schema, &baseline, request),
            None => baseline,
        })
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
