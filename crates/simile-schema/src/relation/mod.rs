//! Compiles and proves public request-to-response relationships.
//!
//! OpenAPI validates request and response documents independently. The media-type extension
//! `x-simile-from-request` supplies the missing cross-document contract: compatible request
//! values can be projected into a response, totals can be derived, and request controls can
//! select reachable lifecycle fields while excluding impossible optional data.

mod compile;
mod evaluate;
mod parse;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{SchemaId, SchemaRef};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Plan {
    #[serde(rename = "q")]
    request_schema: SchemaRef,
    #[serde(rename = "m", default, skip_serializing_if = "Vec::is_empty")]
    mappings: Vec<Mapping>,
    #[serde(rename = "d", default, skip_serializing_if = "Vec::is_empty")]
    derivations: Vec<Derivation>,
    #[serde(rename = "r", default, skip_serializing_if = "Vec::is_empty")]
    rules: Vec<Rule>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Mapping {
    #[serde(rename = "s")]
    source: String,
    #[serde(rename = "t")]
    target: String,
    #[serde(rename = "T")]
    target_schema: SchemaId,
    #[serde(rename = "d", skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Derivation {
    #[serde(rename = "t")]
    target: String,
    #[serde(rename = "v")]
    value: Expression,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum Expression {
    #[serde(rename = "r")]
    Request(String),
    #[serde(rename = "p")]
    Relative(String),
    #[serde(rename = "l")]
    Literal(Value),
    #[serde(rename = "a")]
    Add(Vec<Expression>),
    #[serde(rename = "m")]
    Multiply(Vec<Expression>),
    #[serde(rename = "s")]
    Sum {
        each: String,
        value: Box<Expression>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Rule {
    #[serde(rename = "w")]
    when: Vec<Condition>,
    #[serde(rename = "s", default, skip_serializing_if = "Vec::is_empty")]
    set: Vec<(String, Value)>,
    #[serde(rename = "o", default, skip_serializing_if = "Vec::is_empty")]
    omit: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Condition {
    #[serde(rename = "p")]
    path: String,
    #[serde(rename = "e")]
    equals: Value,
    #[serde(rename = "d", skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
}
