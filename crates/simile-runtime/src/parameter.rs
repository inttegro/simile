use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use simile_artifact::{Artifact, ArtifactParameter};
use simile_schema::ValidationIssue;
use simile_spec::ParameterLocation;

use crate::{Error, Result};

/// Wire values collected by a transport adapter before contract validation.
///
/// Values remain repeated strings here because OpenAPI style and schema decide
/// whether a repeated or delimited representation means a scalar, array, or
/// object. The runtime performs that interpretation once for every adapter.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingParameters {
    pub path: BTreeMap<String, Vec<String>>,
    pub query: BTreeMap<String, Vec<String>>,
    pub headers: BTreeMap<String, Vec<String>>,
    pub cookies: BTreeMap<String, Vec<String>>,
}

impl IncomingParameters {
    fn values(&self, location: ParameterLocation) -> &BTreeMap<String, Vec<String>> {
        match location {
            ParameterLocation::Path => &self.path,
            ParameterLocation::Query => &self.query,
            ParameterLocation::Header => &self.headers,
            ParameterLocation::Cookie => &self.cookies,
        }
    }

    #[must_use]
    pub fn first(&self, location: ParameterLocation, name: &str) -> Option<&str> {
        let name = if location == ParameterLocation::Header {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        };
        self.values(location)
            .get(&name)
            .and_then(|values| values.first())
            .map(String::as_str)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
            && self.query.is_empty()
            && self.headers.is_empty()
            && self.cookies.is_empty()
    }
}

/// Parameters proven against one operation's compiled contract.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValidatedParameters {
    operation: Option<String>,
    values: BTreeMap<ParameterLocation, BTreeMap<String, Value>>,
}

impl ValidatedParameters {
    #[must_use]
    pub fn operation(&self) -> Option<&str> {
        self.operation.as_deref()
    }

    #[must_use]
    pub fn get(&self, location: ParameterLocation, name: &str) -> Option<&Value> {
        self.values.get(&location).and_then(|values| {
            if location == ParameterLocation::Header {
                values.get(&name.to_ascii_lowercase())
            } else {
                values.get(name)
            }
        })
    }

    /// Iterate over decoded parameters in one OpenAPI location.
    pub fn at(&self, location: ParameterLocation) -> impl Iterator<Item = (&str, &Value)> {
        self.values
            .get(&location)
            .into_iter()
            .flat_map(|values| values.iter())
            .map(|(name, value)| (name.as_str(), value))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterIssue {
    pub name: String,
    pub location: ParameterLocation,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ValidationIssue>,
}

/// Decode and validate every declared path, query, header, and cookie value.
pub fn validate_parameters(
    artifact: &Artifact,
    operation: &str,
    incoming: &IncomingParameters,
) -> Result<ValidatedParameters> {
    let operation = artifact
        .operation(operation)
        .ok_or_else(|| Error::UnknownOperation {
            operation: operation.to_owned(),
        })?;
    let mut validated = ValidatedParameters {
        operation: Some(operation.key().to_owned()),
        ..ValidatedParameters::default()
    };
    let mut problems = Vec::new();
    for parameter in operation.parameters() {
        let identity = parameter_identity(parameter.location(), parameter.name());
        let raw = parameter_values(&parameter, incoming);
        if raw.is_empty() {
            if parameter.is_required() {
                problems.push(ParameterIssue {
                    name: parameter.name().to_owned(),
                    location: parameter.location(),
                    message: "required parameter is missing".to_owned(),
                    issues: Vec::new(),
                });
            }
            continue;
        }
        match decode_parameter(&parameter, raw) {
            Ok(value) => {
                validated
                    .values
                    .entry(parameter.location())
                    .or_default()
                    .insert(identity, value);
            }
            Err(issue) => problems.push(issue),
        }
    }
    if problems.is_empty() {
        Ok(validated)
    } else {
        Err(Error::InvalidParameters { issues: problems })
    }
}

/// Fill only missing required parameters with deterministic schema-valid values.
///
/// The result stays in the transport-neutral wire representation used by HTTP,
/// reproduction bundles, and response bindings. Caller-provided values always
/// win; optional parameters remain absent unless the caller supplies them.
pub fn synthesize_missing_parameters(
    artifact: &Artifact,
    operation: &str,
    incoming: &IncomingParameters,
    seed: u64,
) -> Result<IncomingParameters> {
    let operation = artifact
        .operation(operation)
        .ok_or_else(|| Error::UnknownOperation {
            operation: operation.to_owned(),
        })?;
    let mut completed = incoming.clone();
    for (index, parameter) in operation.parameters().enumerate() {
        if !parameter.is_required() || !parameter_values(&parameter, &completed).is_empty() {
            continue;
        }
        let value = parameter.schema().map_or_else(
            || Ok(Value::String("simile".to_owned())),
            |schema| {
                schema
                    .synthesize_fuzz(parameter_seed(seed, index))
                    .map_err(|source| Error::ParameterSynthesis {
                        name: parameter.name().to_owned(),
                        source,
                    })
            },
        )?;
        insert_parameter(&mut completed, &parameter, &value);
    }
    validate_parameters(artifact, operation.key(), &completed)?;
    Ok(completed)
}

fn parameter_seed(seed: u64, index: usize) -> u64 {
    seed ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

fn insert_parameter(
    incoming: &mut IncomingParameters,
    parameter: &ArtifactParameter<'_>,
    value: &Value,
) {
    let values = incoming_values_mut(incoming, parameter.location());
    let identity = parameter_identity(parameter.location(), parameter.name());
    match (parameter.style(), parameter.explode(), value) {
        ("deepObject", _, Value::Object(fields)) => {
            for (name, value) in fields {
                values.insert(
                    format!("{}[{name}]", parameter.name()),
                    vec![wire_scalar(value)],
                );
            }
        }
        ("form", true, Value::Object(fields)) => {
            for (name, value) in fields {
                values.insert(name.clone(), vec![wire_scalar(value)]);
            }
        }
        ("form", true, Value::Array(items)) => {
            values.insert(identity, items.iter().map(wire_scalar).collect());
        }
        ("matrix", true, Value::Array(items)) => {
            values.insert(identity, vec![matrix_array(parameter.name(), items)]);
        }
        ("matrix", true, Value::Object(fields)) => {
            values.insert(identity, vec![matrix_object(fields)]);
        }
        ("matrix", false, value) => {
            values.insert(
                identity,
                vec![format!(
                    ";{}={}",
                    parameter.name(),
                    delimited_value(value, ',')
                )],
            );
        }
        ("label", true, value) => {
            values.insert(identity, vec![format!(".{}", exploded_value(value, '.'))]);
        }
        ("label", false, value) => {
            values.insert(identity, vec![format!(".{}", delimited_value(value, ','))]);
        }
        ("spaceDelimited", _, value) => {
            values.insert(identity, vec![delimited_value(value, ' ')]);
        }
        ("pipeDelimited", _, value) => {
            values.insert(identity, vec![delimited_value(value, '|')]);
        }
        ("simple", true, value) => {
            values.insert(identity, vec![exploded_value(value, ',')]);
        }
        (_, _, value) => {
            values.insert(identity, vec![delimited_value(value, ',')]);
        }
    }
}

fn matrix_array(name: &str, items: &[Value]) -> String {
    items.iter().fold(String::new(), |mut rendered, item| {
        let _ = write!(rendered, ";{name}={}", wire_scalar(item));
        rendered
    })
}

fn matrix_object(fields: &Map<String, Value>) -> String {
    fields
        .iter()
        .fold(String::new(), |mut rendered, (name, value)| {
            let _ = write!(rendered, ";{name}={}", wire_scalar(value));
            rendered
        })
}

fn incoming_values_mut(
    incoming: &mut IncomingParameters,
    location: ParameterLocation,
) -> &mut BTreeMap<String, Vec<String>> {
    match location {
        ParameterLocation::Path => &mut incoming.path,
        ParameterLocation::Query => &mut incoming.query,
        ParameterLocation::Header => &mut incoming.headers,
        ParameterLocation::Cookie => &mut incoming.cookies,
    }
}

fn delimited_value(value: &Value, delimiter: char) -> String {
    match value {
        Value::Array(items) => items
            .iter()
            .map(wire_scalar)
            .collect::<Vec<_>>()
            .join(&delimiter.to_string()),
        Value::Object(fields) => fields
            .iter()
            .flat_map(|(name, value)| [name.clone(), wire_scalar(value)])
            .collect::<Vec<_>>()
            .join(&delimiter.to_string()),
        value => wire_scalar(value),
    }
}

fn exploded_value(value: &Value, delimiter: char) -> String {
    match value {
        Value::Object(fields) => fields
            .iter()
            .map(|(name, value)| format!("{name}={}", wire_scalar(value)))
            .collect::<Vec<_>>()
            .join(&delimiter.to_string()),
        value => delimited_value(value, delimiter),
    }
}

fn wire_scalar(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn parameter_values(
    parameter: &ArtifactParameter<'_>,
    incoming: &IncomingParameters,
) -> Vec<String> {
    let values = incoming.values(parameter.location());
    if parameter.location() == ParameterLocation::Query && parameter.style() == "deepObject" {
        let prefix = format!("{}[", parameter.name());
        return values
            .iter()
            .filter(|(name, _)| name.starts_with(&prefix) && name.ends_with(']'))
            .flat_map(|(name, values)| {
                let key = &name[prefix.len()..name.len() - 1];
                values.iter().map(move |value| {
                    // Preserve the object key beside the value in a reversible
                    // pair; `deep_object` decodes it below.
                    format!("{key}\0{value}")
                })
            })
            .collect();
    }
    let identity = parameter_identity(parameter.location(), parameter.name());
    let direct = values
        .get(&identity)
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    if !direct.is_empty() || parameter.style() != "form" || !parameter.explode() {
        return direct;
    }

    // Exploded form objects have no outer parameter key: `role=admin&active=true`.
    // Schema property names make the collection precise even when the request
    // contains unrelated query or cookie values.
    parameter.schema().map_or_else(Vec::new, |schema| {
        schema
            .property_names()
            .flat_map(|property| {
                values
                    .get(property)
                    .into_iter()
                    .flatten()
                    .map(move |value| format!("{property}\0{value}"))
            })
            .collect()
    })
}

fn decode_parameter(
    parameter: &ArtifactParameter<'_>,
    raw: Vec<String>,
) -> std::result::Result<Value, ParameterIssue> {
    let Some(schema) = parameter.schema() else {
        return Ok(if raw.len() == 1 {
            Value::String(raw[0].clone())
        } else {
            Value::Array(raw.into_iter().map(Value::String).collect())
        });
    };
    let candidates = candidates(parameter, &raw);
    let mut closest = None;
    for candidate in candidates {
        let issues = schema.validate(&candidate);
        if issues.is_empty() {
            return Ok(candidate);
        }
        if closest
            .as_ref()
            .is_none_or(|(_, previous): &(Value, Vec<ValidationIssue>)| {
                issues.len() < previous.len()
            })
        {
            closest = Some((candidate, issues));
        }
    }
    let (_, issues) = closest.unwrap_or_else(|| (Value::Null, Vec::new()));
    Err(ParameterIssue {
        name: parameter.name().to_owned(),
        location: parameter.location(),
        message: "wire value does not satisfy the declared parameter schema".to_owned(),
        issues,
    })
}

fn candidates(parameter: &ArtifactParameter<'_>, raw: &[String]) -> Vec<Value> {
    if parameter.style() == "deepObject" {
        return vec![deep_object(raw)];
    }
    let mut values = Vec::new();
    if raw.iter().any(|value| value.contains('\0')) {
        values.push(deep_object(raw));
    }
    if raw.len() > 1 {
        values.push(Value::Array(
            raw.iter().map(|value| scalar(value)).collect(),
        ));
        values.push(Value::Array(
            raw.iter()
                .map(|value| Value::String(value.clone()))
                .collect(),
        ));
    }
    let first = raw.first().map(String::as_str).unwrap_or_default();
    match parameter.style() {
        "matrix" => values.extend(matrix_candidates(
            parameter.name(),
            first,
            parameter.explode(),
        )),
        "label" => values.extend(label_candidates(first, parameter.explode())),
        "simple" if parameter.explode() => {
            if let Some(object) = exploded_object(first.split(',')) {
                values.push(object);
            }
        }
        _ => {}
    }
    values.extend(scalar_candidates(first));
    let delimiter = match parameter.style() {
        "spaceDelimited" => ' ',
        "pipeDelimited" => '|',
        _ => ',',
    };
    if first.contains(delimiter) {
        let parts = first.split(delimiter).collect::<Vec<_>>();
        values.push(Value::Array(
            parts.iter().map(|value| scalar(value)).collect(),
        ));
        // Why: modulo preserves compatibility with the repository's pinned Rust 1.85 toolchain.
        if parts.len() % 2 == 0 {
            let object = parts
                .chunks_exact(2)
                .map(|pair| (pair[0].to_owned(), scalar(pair[1])))
                .collect::<Map<_, _>>();
            values.push(Value::Object(object));
        }
    }
    values
}

fn matrix_candidates(name: &str, raw: &str, explode: bool) -> Vec<Value> {
    let entries = raw
        .strip_prefix(';')
        .unwrap_or(raw)
        .split(';')
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Vec::new();
    }
    if explode {
        let named = entries
            .iter()
            .filter_map(|entry| entry.strip_prefix(&format!("{name}=")))
            .collect::<Vec<_>>();
        let mut candidates = Vec::new();
        if !named.is_empty() {
            candidates.push(Value::Array(named.into_iter().map(scalar).collect()));
        }
        if let Some(object) = exploded_object(entries.iter().copied()) {
            candidates.push(object);
        }
        candidates
    } else {
        entries
            .first()
            .and_then(|entry| entry.strip_prefix(&format!("{name}=")))
            .map(delimited_candidates)
            .unwrap_or_default()
    }
}

fn label_candidates(raw: &str, explode: bool) -> Vec<Value> {
    let value = raw.strip_prefix('.').unwrap_or(raw);
    if !explode {
        return delimited_candidates(value);
    }
    let parts = value.split('.').collect::<Vec<_>>();
    let mut candidates = vec![Value::Array(
        parts.iter().map(|part| scalar(part)).collect(),
    )];
    if let Some(object) = exploded_object(parts.iter().copied()) {
        candidates.push(object);
    }
    candidates
}

fn delimited_candidates(value: &str) -> Vec<Value> {
    let parts = value.split(',').collect::<Vec<_>>();
    let mut candidates = vec![Value::Array(
        parts.iter().map(|part| scalar(part)).collect(),
    )];
    if parts.len() % 2 == 0 {
        candidates.push(Value::Object(
            parts
                .chunks_exact(2)
                .map(|pair| (pair[0].to_owned(), scalar(pair[1])))
                .collect(),
        ));
    }
    candidates
}

fn exploded_object<'a>(entries: impl IntoIterator<Item = &'a str>) -> Option<Value> {
    let object = entries
        .into_iter()
        .filter_map(|entry| entry.split_once('='))
        .map(|(key, value)| (key.to_owned(), scalar(value)))
        .collect::<Map<_, _>>();
    (!object.is_empty()).then_some(Value::Object(object))
}

fn scalar_candidates(value: &str) -> Vec<Value> {
    let mut values = vec![Value::String(value.to_owned())];
    let scalar = scalar(value);
    if !values.contains(&scalar) {
        values.push(scalar);
    }
    if matches!(value.as_bytes().first(), Some(b'{' | b'[')) {
        if let Ok(value) = serde_json::from_str(value) {
            values.push(value);
        }
    }
    values
}

fn scalar(value: &str) -> Value {
    match value {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "null" => Value::Null,
        _ => {
            if let Ok(value) = value.parse::<i64>() {
                return Value::from(value);
            }
            if let Ok(value) = value.parse::<u64>() {
                return Value::from(value);
            }
            if let Some(value) = value.parse::<f64>().ok().and_then(Number::from_f64) {
                return Value::Number(value);
            }
            Value::String(value.to_owned())
        }
    }
}

fn deep_object(raw: &[String]) -> Value {
    Value::Object(
        raw.iter()
            .filter_map(|entry| entry.split_once('\0'))
            .map(|(key, value)| (key.to_owned(), scalar(value)))
            .collect(),
    )
}

fn parameter_identity(location: ParameterLocation, name: &str) -> String {
    if location == ParameterLocation::Header {
        name.to_ascii_lowercase()
    } else {
        name.to_owned()
    }
}
