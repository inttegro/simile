use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use simile_heuristics::{Plan as HeuristicPlan, Policy as HeuristicPolicy};
use simile_spec::{OpenApiFeature, ParameterLocation, RequestBodyContract, ResponseBodyContract};

use crate::{
    AdditionalProperties, CompileError, JsonType, MediaTypePlan, NumberBound, RequestBodyPlan,
    ResponseBodyPlan, Result, SchemaCatalog, SchemaId, SchemaNode, SchemaRef, ValueSchemaPlan,
    soundness,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SchemaDirection {
    Request,
    Response,
}

/// Compiles request and response bodies into one shared runtime schema catalog.
pub struct BodySchemaCompiler<'a> {
    components: &'a BTreeMap<String, Value>,
    feature: OpenApiFeature,
    nodes: Vec<Option<SchemaNode>>,
    references: BTreeMap<(SchemaDirection, String), SchemaId>,
    pending_constraints: BTreeMap<SchemaId, Vec<soundness::PendingConstraint>>,
    heuristic_policy: HeuristicPolicy,
}

/// One compiled operation parameter available while request relations are resolved.
#[derive(Clone, Copy)]
pub struct RelationParameter<'a> {
    pub location: ParameterLocation,
    pub name: &'a str,
    pub schema: Option<&'a ValueSchemaPlan>,
}

impl<'a> BodySchemaCompiler<'a> {
    #[must_use]
    pub fn new(components: &'a BTreeMap<String, Value>, feature: OpenApiFeature) -> Self {
        Self::with_heuristics(components, feature, HeuristicPolicy::default())
    }

    #[must_use]
    pub fn with_heuristics(
        components: &'a BTreeMap<String, Value>,
        feature: OpenApiFeature,
        heuristic_policy: HeuristicPolicy,
    ) -> Self {
        Self {
            components,
            feature,
            nodes: Vec::new(),
            references: BTreeMap::new(),
            pending_constraints: BTreeMap::new(),
            heuristic_policy,
        }
    }

    pub fn compile_request(&mut self, body: &RequestBodyContract) -> Result<RequestBodyPlan> {
        let content =
            self.compile_content(&body.content, "requestBody", SchemaDirection::Request)?;
        Ok(RequestBodyPlan {
            required: body.required,
            content,
        })
    }

    pub fn compile_response(&mut self, body: &ResponseBodyContract) -> Result<ResponseBodyPlan> {
        self.compile_response_for(body, None)
    }

    /// Compile one request parameter into the same schema graph as bodies.
    pub fn compile_parameter(&mut self, schema: &Value, field: &str) -> Result<ValueSchemaPlan> {
        self.compile_schema(schema, field, SchemaDirection::Request)
            .map(|schema| ValueSchemaPlan {
                schema: SchemaRef(schema),
            })
    }

    /// Compile one response header into the shared response schema graph.
    pub fn compile_response_header(
        &mut self,
        schema: &Value,
        field: &str,
    ) -> Result<ValueSchemaPlan> {
        self.compile_schema(schema, field, SchemaDirection::Response)
            .map(|schema| ValueSchemaPlan {
                schema: SchemaRef(schema),
            })
    }

    /// Compile a response body together with any public request-to-response relation.
    pub fn compile_response_for(
        &mut self,
        body: &ResponseBodyContract,
        request: Option<&RequestBodyPlan>,
    ) -> Result<ResponseBodyPlan> {
        self.compile_response_for_operation(body, request, &[])
    }

    /// Compile a response with the complete validated request surface for its operation.
    pub fn compile_response_for_operation(
        &mut self,
        body: &ResponseBodyContract,
        request: Option<&RequestBodyPlan>,
        parameters: &[RelationParameter<'_>],
    ) -> Result<ResponseBodyPlan> {
        let content = self.compile_response_content(&body.content, request, parameters)?;
        Ok(ResponseBodyPlan { content })
    }

    fn compile_response_content(
        &mut self,
        content: &BTreeMap<String, simile_spec::MediaTypeContract>,
        request: Option<&RequestBodyPlan>,
        parameters: &[RelationParameter<'_>],
    ) -> Result<BTreeMap<String, MediaTypePlan>> {
        content
            .iter()
            .map(|(media_type, contract)| {
                let field = format!("responseBody.{media_type}");
                let schema = contract
                    .schema
                    .as_ref()
                    .map(|schema| {
                        self.compile_schema(
                            schema,
                            &format!("{field}.schema"),
                            SchemaDirection::Response,
                        )
                        .map(SchemaRef)
                    })
                    .transpose()?;
                let relation = contract
                    .request_relation
                    .as_ref()
                    .map(|definition| {
                        let response_schema =
                            schema.ok_or_else(|| CompileError::InvalidConstraint {
                                field: format!("{field}.x-simile-from-request"),
                                reason: "a request relation requires a response schema".to_owned(),
                            })?;
                        let request_schemas = request
                            .into_iter()
                            .flat_map(|request| request.content.values())
                            .filter_map(|media| media.schema)
                            .collect();
                        crate::relation::Plan::compile(
                            definition,
                            request_schemas,
                            parameters,
                            response_schema,
                            &self.structural_catalog()?,
                            &format!("{field}.x-simile-from-request"),
                        )
                    })
                    .transpose()?;
                Ok((
                    media_type.to_ascii_lowercase(),
                    MediaTypePlan {
                        schema,
                        example: contract.example.clone(),
                        relation,
                    },
                ))
            })
            .collect()
    }

    fn compile_content(
        &mut self,
        content: &BTreeMap<String, simile_spec::MediaTypeContract>,
        field: &str,
        direction: SchemaDirection,
    ) -> Result<BTreeMap<String, MediaTypePlan>> {
        content
            .iter()
            .map(|(media_type, contract)| {
                let schema = contract
                    .schema
                    .as_ref()
                    .map(|schema| {
                        self.compile_schema(schema, &format!("{field}.schema"), direction)
                            .map(SchemaRef)
                    })
                    .transpose()?;
                Ok((
                    media_type.to_ascii_lowercase(),
                    MediaTypePlan {
                        schema,
                        example: contract.example.clone(),
                        relation: None,
                    },
                ))
            })
            .collect()
    }

    pub fn finish(self) -> Result<SchemaCatalog> {
        let nodes = self
            .nodes
            .into_iter()
            .map(|node| node.ok_or(CompileError::IncompleteNode))
            .collect::<Result<Vec<_>>>()?;
        let mut catalog = SchemaCatalog { nodes };
        for (id, constraints) in self.pending_constraints {
            let compiled = constraints
                .into_iter()
                .map(|constraint| soundness::compile(&catalog, id, constraint))
                .collect::<Result<Vec<_>>>()?;
            let node = catalog
                .nodes
                .get_mut(id as usize)
                .ok_or(CompileError::IncompleteNode)?;
            node.constraints = compiled;
        }
        Ok(catalog)
    }

    fn structural_catalog(&self) -> Result<SchemaCatalog> {
        self.nodes
            .iter()
            .cloned()
            .map(|node| node.ok_or(CompileError::IncompleteNode))
            .collect::<Result<Vec<_>>>()
            .map(|nodes| SchemaCatalog { nodes })
    }

    fn compile_schema(
        &mut self,
        schema: &Value,
        field: &str,
        direction: SchemaDirection,
    ) -> Result<SchemaId> {
        match schema {
            Value::Bool(allowed) => self.push(SchemaNode {
                allowed: Some(*allowed),
                ..SchemaNode::default()
            }),
            Value::Object(object) => self.compile_object(object, field, direction),
            _ => Err(CompileError::InvalidField {
                field: field.to_owned(),
                expected: "an object or boolean",
            }),
        }
    }

    fn compile_object(
        &mut self,
        schema: &Map<String, Value>,
        field: &str,
        direction: SchemaDirection,
    ) -> Result<SchemaId> {
        if let Some(reference) = schema.get("$ref") {
            let reference = reference
                .as_str()
                .ok_or_else(|| CompileError::InvalidField {
                    field: format!("{field}.$ref"),
                    expected: "a string",
                })?;
            let target = self.compile_reference(reference, field, direction)?;
            if self.feature == OpenApiFeature::V3_0 || schema.len() == 1 {
                return Ok(target);
            }
            let id = self.reserve()?;
            let mut node = self.node(schema, field, direction)?;
            let constraints = soundness::parse(schema, field)?;
            node.all_of.insert(0, target);
            self.fill(id, node);
            self.pending_constraints.insert(id, constraints);
            return Ok(id);
        }

        let id = self.reserve()?;
        let node = self.node(schema, field, direction)?;
        let constraints = soundness::parse(schema, field)?;
        self.fill(id, node);
        self.pending_constraints.insert(id, constraints);
        Ok(id)
    }

    fn compile_reference(
        &mut self,
        reference: &str,
        field: &str,
        direction: SchemaDirection,
    ) -> Result<SchemaId> {
        let key = (direction, reference.to_owned());
        if let Some(id) = self.references.get(&key) {
            return Ok(*id);
        }
        let name = component_name(reference).ok_or_else(|| CompileError::UnsupportedReference {
            field: field.to_owned(),
            reference: reference.to_owned(),
        })?;
        let schema = self
            .components
            .get(&name)
            .ok_or_else(|| CompileError::UnknownReference {
                field: field.to_owned(),
                reference: reference.to_owned(),
            })?;
        let id = self.reserve()?;
        self.references.insert(key, id);
        let mut constraints = Vec::new();
        let node = match schema {
            Value::Bool(allowed) => SchemaNode {
                allowed: Some(*allowed),
                ..SchemaNode::default()
            },
            Value::Object(object) => {
                if let Some(nested) = object.get("$ref").and_then(Value::as_str) {
                    let target = self.compile_reference(
                        nested,
                        &format!("components.schemas.{name}"),
                        direction,
                    )?;
                    let mut node = if self.feature == OpenApiFeature::V3_0 || object.len() == 1 {
                        SchemaNode::default()
                    } else {
                        constraints =
                            soundness::parse(object, &format!("components.schemas.{name}"))?;
                        self.node(object, &format!("components.schemas.{name}"), direction)?
                    };
                    node.all_of.insert(0, target);
                    node
                } else {
                    constraints = soundness::parse(object, &format!("components.schemas.{name}"))?;
                    self.node(object, &format!("components.schemas.{name}"), direction)?
                }
            }
            _ => {
                return Err(CompileError::InvalidField {
                    field: format!("components.schemas.{name}"),
                    expected: "an object or boolean",
                });
            }
        };
        self.fill(id, node);
        self.pending_constraints.insert(id, constraints);
        Ok(id)
    }

    fn node(
        &mut self,
        schema: &Map<String, Value>,
        field: &str,
        direction: SchemaDirection,
    ) -> Result<SchemaNode> {
        let mut types = self.types(schema, field)?;
        if self.feature == OpenApiFeature::V3_0
            && !types.is_empty()
            && schema.get("nullable").and_then(Value::as_bool) == Some(true)
        {
            types.insert(JsonType::Null);
        }
        Ok(SchemaNode {
            allowed: None,
            types,
            enum_values: optional_array(schema, "enum", field)?.cloned(),
            const_value: schema.get("const").cloned(),
            hint: schema_hint(schema),
            format: optional_string(schema, "format", field)?.map(str::to_owned),
            pattern: compile_pattern(schema, field)?,
            properties: self.properties(schema, field, direction)?,
            required: self.direction_required(schema, field, direction)?,
            additional: self.additional_properties(schema, field, direction)?,
            items: schema
                .get("items")
                .map(|value| self.compile_schema(value, &format!("{field}.items"), direction))
                .transpose()?,
            all_of: self.schema_array(schema, "allOf", field, direction)?,
            any_of: self.schema_array(schema, "anyOf", field, direction)?,
            one_of: self.schema_array(schema, "oneOf", field, direction)?,
            not: schema
                .get("not")
                .map(|value| self.compile_schema(value, &format!("{field}.not"), direction))
                .transpose()?,
            minimum: lower_bound(schema, self.feature, field)?,
            maximum: upper_bound(schema, self.feature, field)?,
            multiple_of: positive_number(schema, "multipleOf", field)?,
            min_length: nonnegative_integer(schema, "minLength", field)?,
            max_length: nonnegative_integer(schema, "maxLength", field)?,
            min_items: nonnegative_integer(schema, "minItems", field)?,
            max_items: nonnegative_integer(schema, "maxItems", field)?,
            unique_items: optional_bool(schema, "uniqueItems", field)?.unwrap_or(false),
            min_properties: nonnegative_integer(schema, "minProperties", field)?,
            max_properties: nonnegative_integer(schema, "maxProperties", field)?,
            constraints: Vec::new(),
            heuristics: HeuristicPlan::compile(schema, self.heuristic_policy),
        })
    }

    fn types(&self, schema: &Map<String, Value>, field: &str) -> Result<BTreeSet<JsonType>> {
        let Some(value) = schema.get("type") else {
            return Ok(BTreeSet::new());
        };
        let values = match value {
            Value::String(value) => vec![value.as_str()],
            Value::Array(values) if self.feature != OpenApiFeature::V3_0 && !values.is_empty() => {
                values
                    .iter()
                    .map(|value| {
                        value.as_str().ok_or_else(|| CompileError::InvalidField {
                            field: format!("{field}.type"),
                            expected: "a string or an array of strings",
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
            }
            _ => {
                return Err(CompileError::InvalidField {
                    field: format!("{field}.type"),
                    expected: "a string or an array of strings",
                });
            }
        };
        values
            .into_iter()
            .map(|value| json_type(value, field))
            .collect()
    }

    fn properties(
        &mut self,
        schema: &Map<String, Value>,
        field: &str,
        direction: SchemaDirection,
    ) -> Result<BTreeMap<String, SchemaId>> {
        let Some(value) = schema.get("properties") else {
            return Ok(BTreeMap::new());
        };
        let properties = value
            .as_object()
            .ok_or_else(|| CompileError::InvalidField {
                field: format!("{field}.properties"),
                expected: "an object",
            })?;
        let mut compiled = BTreeMap::new();
        for (name, value) in properties {
            if !self.property_available(value, direction) {
                continue;
            }
            let schema =
                self.compile_schema(value, &format!("{field}.properties.{name}"), direction)?;
            compiled.insert(name.clone(), schema);
        }
        Ok(compiled)
    }

    fn additional_properties(
        &mut self,
        schema: &Map<String, Value>,
        field: &str,
        direction: SchemaDirection,
    ) -> Result<AdditionalProperties> {
        match schema.get("additionalProperties") {
            None | Some(Value::Bool(true)) => Ok(AdditionalProperties::Allow),
            Some(Value::Bool(false)) => Ok(AdditionalProperties::Deny),
            Some(value @ Value::Object(_)) => Ok(AdditionalProperties::Schema(
                self.compile_schema(value, &format!("{field}.additionalProperties"), direction)?,
            )),
            Some(_) => Err(CompileError::InvalidField {
                field: format!("{field}.additionalProperties"),
                expected: "a boolean or schema",
            }),
        }
    }

    fn schema_array(
        &mut self,
        schema: &Map<String, Value>,
        name: &str,
        field: &str,
        direction: SchemaDirection,
    ) -> Result<Vec<SchemaId>> {
        let Some(value) = schema.get(name) else {
            return Ok(Vec::new());
        };
        let values = value
            .as_array()
            .filter(|values| !values.is_empty())
            .ok_or_else(|| CompileError::InvalidField {
                field: format!("{field}.{name}"),
                expected: "a non-empty array of schemas",
            })?;
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                self.compile_schema(value, &format!("{field}.{name}.{index}"), direction)
            })
            .collect()
    }

    fn property_available(&self, value: &Value, direction: SchemaDirection) -> bool {
        !self.directionally_excluded(value, direction, 0, &mut BTreeSet::new())
    }

    fn direction_required(
        &self,
        object: &Map<String, Value>,
        field: &str,
        direction: SchemaDirection,
    ) -> Result<BTreeSet<String>> {
        let mut required = string_set(object, "required", field)?;
        if let Some(properties) = object.get("properties").and_then(Value::as_object) {
            required.retain(|name| {
                properties
                    .get(name)
                    .is_none_or(|value| self.property_available(value, direction))
            });
        }
        Ok(required)
    }

    fn directionally_excluded(
        &self,
        value: &Value,
        direction: SchemaDirection,
        depth: usize,
        visited: &mut BTreeSet<String>,
    ) -> bool {
        let Some(schema) = value.as_object() else {
            return false;
        };
        let annotation = match direction {
            SchemaDirection::Request => "readOnly",
            SchemaDirection::Response => "writeOnly",
        };
        if schema.get(annotation).and_then(Value::as_bool) == Some(true) {
            return true;
        }
        if depth >= 64 {
            return false;
        }
        if let Some(name) = schema
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(component_name)
        {
            if visited.insert(name.clone())
                && self.components.get(&name).is_some_and(|component| {
                    self.directionally_excluded(component, direction, depth + 1, visited)
                })
            {
                return true;
            }
        }
        ["allOf", "anyOf", "oneOf"].iter().any(|keyword| {
            schema
                .get(*keyword)
                .and_then(Value::as_array)
                .is_some_and(|schemas| {
                    schemas.iter().any(|schema| {
                        self.directionally_excluded(schema, direction, depth + 1, visited)
                    })
                })
        })
    }

    fn reserve(&mut self) -> Result<SchemaId> {
        let id = u32::try_from(self.nodes.len()).map_err(|_| CompileError::TooManyNodes)?;
        self.nodes.push(None);
        Ok(id)
    }

    fn push(&mut self, node: SchemaNode) -> Result<SchemaId> {
        let id = self.reserve()?;
        self.fill(id, node);
        Ok(id)
    }

    fn fill(&mut self, id: SchemaId, node: SchemaNode) {
        self.nodes[id as usize] = Some(node);
    }
}

fn compile_pattern(schema: &Map<String, Value>, field: &str) -> Result<Option<String>> {
    let Some(pattern) = optional_string(schema, "pattern", field)? else {
        return Ok(None);
    };
    regress::Regex::new(pattern).map_err(|error| CompileError::InvalidFieldValue {
        field: format!("{field}.pattern"),
        reason: error.to_string(),
    })?;
    Ok(Some(pattern.to_owned()))
}

fn component_name(reference: &str) -> Option<String> {
    let encoded = reference.strip_prefix("#/components/schemas/")?;
    if encoded.is_empty() || encoded.contains('/') {
        return None;
    }
    Some(encoded.replace("~1", "/").replace("~0", "~"))
}

fn json_type(value: &str, field: &str) -> Result<JsonType> {
    match value {
        "null" => Ok(JsonType::Null),
        "boolean" => Ok(JsonType::Boolean),
        "object" => Ok(JsonType::Object),
        "array" => Ok(JsonType::Array),
        "number" => Ok(JsonType::Number),
        "integer" => Ok(JsonType::Integer),
        "string" => Ok(JsonType::String),
        _ => Err(CompileError::InvalidField {
            field: format!("{field}.type"),
            expected: "a JSON Schema type",
        }),
    }
}

fn optional_array<'a>(
    object: &'a Map<String, Value>,
    name: &str,
    field: &str,
) -> Result<Option<&'a Vec<Value>>> {
    object
        .get(name)
        .map(|value| {
            value.as_array().ok_or_else(|| CompileError::InvalidField {
                field: format!("{field}.{name}"),
                expected: "an array",
            })
        })
        .transpose()
}

fn schema_hint(schema: &Map<String, Value>) -> Option<Value> {
    schema
        .get("example")
        .cloned()
        .or_else(|| {
            schema
                .get("examples")
                .and_then(Value::as_array)
                .and_then(|examples| examples.first().cloned())
        })
        .or_else(|| schema.get("default").cloned())
}

fn string_set(object: &Map<String, Value>, name: &str, field: &str) -> Result<BTreeSet<String>> {
    let Some(values) = optional_array(object, name, field)? else {
        return Ok(BTreeSet::new());
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| CompileError::InvalidField {
                    field: format!("{field}.{name}"),
                    expected: "an array of strings",
                })
        })
        .collect()
}

fn optional_bool(object: &Map<String, Value>, name: &str, field: &str) -> Result<Option<bool>> {
    object
        .get(name)
        .map(|value| {
            value.as_bool().ok_or_else(|| CompileError::InvalidField {
                field: format!("{field}.{name}"),
                expected: "a boolean",
            })
        })
        .transpose()
}

fn optional_string<'a>(
    object: &'a Map<String, Value>,
    name: &str,
    field: &str,
) -> Result<Option<&'a str>> {
    object
        .get(name)
        .map(|value| {
            value.as_str().ok_or_else(|| CompileError::InvalidField {
                field: format!("{field}.{name}"),
                expected: "a string",
            })
        })
        .transpose()
}

fn nonnegative_integer(
    object: &Map<String, Value>,
    name: &str,
    field: &str,
) -> Result<Option<usize>> {
    object
        .get(name)
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| CompileError::InvalidField {
                    field: format!("{field}.{name}"),
                    expected: "a non-negative integer",
                })
        })
        .transpose()
}

fn positive_number(object: &Map<String, Value>, name: &str, field: &str) -> Result<Option<f64>> {
    object
        .get(name)
        .map(|value| {
            value
                .as_f64()
                .filter(|value| value.is_finite() && *value > 0.0)
                .ok_or_else(|| CompileError::InvalidField {
                    field: format!("{field}.{name}"),
                    expected: "a positive number",
                })
        })
        .transpose()
}

fn lower_bound(
    object: &Map<String, Value>,
    feature: OpenApiFeature,
    field: &str,
) -> Result<Option<NumberBound>> {
    number_bound(object, "minimum", "exclusiveMinimum", feature, field)
}

fn upper_bound(
    object: &Map<String, Value>,
    feature: OpenApiFeature,
    field: &str,
) -> Result<Option<NumberBound>> {
    number_bound(object, "maximum", "exclusiveMaximum", feature, field)
}

fn number_bound(
    object: &Map<String, Value>,
    inclusive_name: &str,
    exclusive_name: &str,
    feature: OpenApiFeature,
    field: &str,
) -> Result<Option<NumberBound>> {
    if feature != OpenApiFeature::V3_0 {
        if let Some(value) = object.get(exclusive_name) {
            return finite_number(value, &format!("{field}.{exclusive_name}")).map(|value| {
                Some(NumberBound {
                    value,
                    exclusive: true,
                })
            });
        }
    }
    let Some(value) = object.get(inclusive_name) else {
        return Ok(None);
    };
    let value = finite_number(value, &format!("{field}.{inclusive_name}"))?;
    let exclusive = if feature == OpenApiFeature::V3_0 {
        optional_bool(object, exclusive_name, field)?.unwrap_or(false)
    } else {
        false
    };
    Ok(Some(NumberBound { value, exclusive }))
}

fn finite_number(value: &Value, field: &str) -> Result<f64> {
    value
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| CompileError::InvalidField {
            field: field.to_owned(),
            expected: "a finite number",
        })
}
