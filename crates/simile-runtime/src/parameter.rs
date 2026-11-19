use std::collections::BTreeMap;

use serde::Serialize;
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
#[derive(Clone, Debug, Default, Eq, PartialEq)]
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
}

/// Parameters proven against one operation's compiled contract.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValidatedParameters {
    values: BTreeMap<ParameterLocation, BTreeMap<String, Value>>,
}

impl ValidatedParameters {
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
    let mut validated = ValidatedParameters::default();
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
    values
        .get(&identity)
        .into_iter()
        .flatten()
        .cloned()
        .collect()
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
