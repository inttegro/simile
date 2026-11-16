use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AdditionalProperties, JsonType, NumberBound, SchemaCatalog, SchemaId, SchemaNode, SchemaRef,
    soundness,
};

const MAX_ISSUES: usize = 100;
const MAX_DEPTH: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub path: String,
    pub rule: ValidationRule,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ValidationRule {
    Schema,
    Type,
    Required,
    AdditionalProperties,
    Enum,
    Const,
    Minimum,
    Maximum,
    MultipleOf,
    MinLength,
    MaxLength,
    MinItems,
    MaxItems,
    UniqueItems,
    MinProperties,
    MaxProperties,
    Aggregate,
    Bounded,
    Distinct,
    Equal,
    Equation,
    Implies,
    Lifecycle,
    Ordered,
    Set,
    Template,
    AllOf,
    AnyOf,
    OneOf,
    Not,
    Depth,
}

impl fmt::Display for ValidationRule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Schema => "schema",
            Self::Type => "type",
            Self::Required => "required",
            Self::AdditionalProperties => "additionalProperties",
            Self::Enum => "enum",
            Self::Const => "const",
            Self::Minimum => "minimum",
            Self::Maximum => "maximum",
            Self::MultipleOf => "multipleOf",
            Self::MinLength => "minLength",
            Self::MaxLength => "maxLength",
            Self::MinItems => "minItems",
            Self::MaxItems => "maxItems",
            Self::UniqueItems => "uniqueItems",
            Self::MinProperties => "minProperties",
            Self::MaxProperties => "maxProperties",
            Self::Aggregate => "aggregate",
            Self::Bounded => "bounded",
            Self::Distinct => "distinct",
            Self::Equal => "equal",
            Self::Equation => "equation",
            Self::Implies => "implies",
            Self::Lifecycle => "lifecycle",
            Self::Ordered => "ordered",
            Self::Set => "set",
            Self::Template => "template",
            Self::AllOf => "allOf",
            Self::AnyOf => "anyOf",
            Self::OneOf => "oneOf",
            Self::Not => "not",
            Self::Depth => "depth",
        })
    }
}

pub(crate) fn value(plan: &SchemaCatalog, root: SchemaRef, value: &Value) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    Validator { plan }.visit(root.0, value, "", 0, &mut issues);
    issues
}

struct Validator<'a> {
    plan: &'a SchemaCatalog,
}

impl Validator<'_> {
    fn visit(
        &self,
        id: SchemaId,
        value: &Value,
        path: &str,
        depth: usize,
        issues: &mut Vec<ValidationIssue>,
    ) {
        if issues.len() >= MAX_ISSUES {
            return;
        }
        if depth >= MAX_DEPTH {
            push(
                issues,
                path,
                ValidationRule::Depth,
                "request body exceeds the validation depth limit",
            );
            return;
        }
        let Some(node) = self.plan.nodes.get(id as usize) else {
            push(
                issues,
                path,
                ValidationRule::Schema,
                "compiled schema contains an invalid node",
            );
            return;
        };
        if node.allowed == Some(false) {
            push(
                issues,
                path,
                ValidationRule::Schema,
                "value is not allowed by this schema",
            );
            return;
        }
        if !node.types.is_empty() && !node.types.iter().any(|kind| matches_type(*kind, value)) {
            let expected = node
                .types
                .iter()
                .map(type_name)
                .collect::<Vec<_>>()
                .join(" or ");
            push(
                issues,
                path,
                ValidationRule::Type,
                format!("expected {expected}, found {}", value_type(value)),
            );
            return;
        }
        self.scalar_constraints(node, value, path, issues);
        self.object_constraints(node, value, path, depth, issues);
        self.array_constraints(node, value, path, depth, issues);
        self.relationship_constraints(node, value, path, issues);
        self.combinators(node, value, path, depth, issues);
    }

    fn relationship_constraints(
        &self,
        node: &SchemaNode,
        value: &Value,
        path: &str,
        issues: &mut Vec<ValidationIssue>,
    ) {
        for violation in soundness::prove(node, value) {
            let rule = match violation.kind {
                soundness::ViolationKind::Aggregate => ValidationRule::Aggregate,
                soundness::ViolationKind::Bounded => ValidationRule::Bounded,
                soundness::ViolationKind::Distinct => ValidationRule::Distinct,
                soundness::ViolationKind::Equal => ValidationRule::Equal,
                soundness::ViolationKind::Equation => ValidationRule::Equation,
                soundness::ViolationKind::Implies => ValidationRule::Implies,
                soundness::ViolationKind::Lifecycle => ValidationRule::Lifecycle,
                soundness::ViolationKind::Ordered => ValidationRule::Ordered,
                soundness::ViolationKind::Set => ValidationRule::Set,
                soundness::ViolationKind::Template => ValidationRule::Template,
            };
            push(
                issues,
                &relationship_path(path, &violation.path),
                rule,
                violation.message,
            );
        }
    }

    fn scalar_constraints(
        &self,
        node: &SchemaNode,
        value: &Value,
        path: &str,
        issues: &mut Vec<ValidationIssue>,
    ) {
        if let Some(values) = &node.enum_values {
            if !values.contains(value) {
                push(
                    issues,
                    path,
                    ValidationRule::Enum,
                    "value is not one of the allowed values",
                );
            }
        }
        if let Some(expected) = &node.const_value {
            if expected != value {
                push(
                    issues,
                    path,
                    ValidationRule::Const,
                    "value does not equal the required constant",
                );
            }
        }
        if let Some(number) = value.as_f64() {
            check_bound(issues, path, number, node.minimum, true);
            check_bound(issues, path, number, node.maximum, false);
            if let Some(divisor) = node.multiple_of {
                let quotient = number / divisor;
                let tolerance = f64::EPSILON * quotient.abs().max(1.0) * 16.0;
                if (quotient - quotient.round()).abs() > tolerance {
                    push(
                        issues,
                        path,
                        ValidationRule::MultipleOf,
                        format!("number must be a multiple of {divisor}"),
                    );
                }
            }
        }
        if let Some(text) = value.as_str() {
            let length = text.chars().count();
            check_minimum_count(
                issues,
                path,
                length,
                node.min_length,
                ValidationRule::MinLength,
                "characters",
            );
            check_maximum_count(
                issues,
                path,
                length,
                node.max_length,
                ValidationRule::MaxLength,
                "characters",
            );
        }
    }

    fn object_constraints(
        &self,
        node: &SchemaNode,
        value: &Value,
        path: &str,
        depth: usize,
        issues: &mut Vec<ValidationIssue>,
    ) {
        let Some(object) = value.as_object() else {
            return;
        };
        check_minimum_count(
            issues,
            path,
            object.len(),
            node.min_properties,
            ValidationRule::MinProperties,
            "properties",
        );
        check_maximum_count(
            issues,
            path,
            object.len(),
            node.max_properties,
            ValidationRule::MaxProperties,
            "properties",
        );
        for name in &node.required {
            if !object.contains_key(name) {
                push(
                    issues,
                    &child_path(path, name),
                    ValidationRule::Required,
                    format!("required property `{name}` is missing"),
                );
            }
        }
        for (name, child) in object {
            let child_path = child_path(path, name);
            if let Some(schema) = node.properties.get(name) {
                self.visit(*schema, child, &child_path, depth + 1, issues);
                continue;
            }
            match node.additional {
                AdditionalProperties::Allow => {}
                AdditionalProperties::Deny => push(
                    issues,
                    &child_path,
                    ValidationRule::AdditionalProperties,
                    format!("property `{name}` is not allowed"),
                ),
                AdditionalProperties::Schema(schema) => {
                    self.visit(schema, child, &child_path, depth + 1, issues);
                }
            }
        }
    }

    fn array_constraints(
        &self,
        node: &SchemaNode,
        value: &Value,
        path: &str,
        depth: usize,
        issues: &mut Vec<ValidationIssue>,
    ) {
        let Some(array) = value.as_array() else {
            return;
        };
        check_minimum_count(
            issues,
            path,
            array.len(),
            node.min_items,
            ValidationRule::MinItems,
            "items",
        );
        check_maximum_count(
            issues,
            path,
            array.len(),
            node.max_items,
            ValidationRule::MaxItems,
            "items",
        );
        if node.unique_items {
            let mut duplicates = BTreeSet::new();
            for (index, value) in array.iter().enumerate() {
                if array[..index].contains(value) {
                    duplicates.insert(index);
                }
            }
            for index in duplicates {
                push(
                    issues,
                    &child_path(path, &index.to_string()),
                    ValidationRule::UniqueItems,
                    "array item must be unique",
                );
            }
        }
        if let Some(schema) = node.items {
            for (index, child) in array.iter().enumerate() {
                self.visit(
                    schema,
                    child,
                    &child_path(path, &index.to_string()),
                    depth + 1,
                    issues,
                );
            }
        }
    }

    fn combinators(
        &self,
        node: &SchemaNode,
        value: &Value,
        path: &str,
        depth: usize,
        issues: &mut Vec<ValidationIssue>,
    ) {
        for schema in &node.all_of {
            self.visit(*schema, value, path, depth + 1, issues);
        }
        if !node.any_of.is_empty()
            && !node
                .any_of
                .iter()
                .any(|schema| self.branch_matches(*schema, value, path, depth + 1))
        {
            push(
                issues,
                path,
                ValidationRule::AnyOf,
                "value does not match any allowed schema",
            );
        }
        if !node.one_of.is_empty() {
            let matches = node
                .one_of
                .iter()
                .filter(|schema| self.branch_matches(**schema, value, path, depth + 1))
                .count();
            if matches != 1 {
                push(
                    issues,
                    path,
                    ValidationRule::OneOf,
                    format!("value must match exactly one schema; matched {matches}"),
                );
            }
        }
        if node
            .not
            .is_some_and(|schema| self.branch_matches(schema, value, path, depth + 1))
        {
            push(
                issues,
                path,
                ValidationRule::Not,
                "value matches a forbidden schema",
            );
        }
    }

    fn branch_matches(&self, id: SchemaId, value: &Value, path: &str, depth: usize) -> bool {
        let mut issues = Vec::new();
        self.visit(id, value, path, depth, &mut issues);
        issues.is_empty()
    }
}

fn matches_type(kind: JsonType, value: &Value) -> bool {
    match kind {
        JsonType::Null => value.is_null(),
        JsonType::Boolean => value.is_boolean(),
        JsonType::Object => value.is_object(),
        JsonType::Array => value.is_array(),
        JsonType::Number => value.is_number(),
        JsonType::Integer => value
            .as_f64()
            .is_some_and(|number| number.is_finite() && number.fract() == 0.0),
        JsonType::String => value.is_string(),
    }
}

fn type_name(kind: &JsonType) -> &'static str {
    match kind {
        JsonType::Null => "null",
        JsonType::Boolean => "boolean",
        JsonType::Object => "object",
        JsonType::Array => "array",
        JsonType::Number => "number",
        JsonType::Integer => "integer",
        JsonType::String => "string",
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn check_bound(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    number: f64,
    bound: Option<NumberBound>,
    lower: bool,
) {
    let Some(bound) = bound else {
        return;
    };
    let invalid = if lower {
        number < bound.value || (bound.exclusive && number <= bound.value)
    } else {
        number > bound.value || (bound.exclusive && number >= bound.value)
    };
    if invalid {
        let relation = match (lower, bound.exclusive) {
            (true, true) => "greater than",
            (true, false) => "greater than or equal to",
            (false, true) => "less than",
            (false, false) => "less than or equal to",
        };
        push(
            issues,
            path,
            if lower {
                ValidationRule::Minimum
            } else {
                ValidationRule::Maximum
            },
            format!("number must be {relation} {}", bound.value),
        );
    }
}

fn check_minimum_count(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    actual: usize,
    expected: Option<usize>,
    rule: ValidationRule,
    noun: &str,
) {
    if expected.is_some_and(|expected| actual < expected) {
        push(
            issues,
            path,
            rule,
            format!(
                "must contain at least {} {noun}",
                expected.unwrap_or_default()
            ),
        );
    }
}

fn check_maximum_count(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    actual: usize,
    expected: Option<usize>,
    rule: ValidationRule,
    noun: &str,
) {
    if expected.is_some_and(|expected| actual > expected) {
        push(
            issues,
            path,
            rule,
            format!(
                "must contain no more than {} {noun}",
                expected.unwrap_or_default()
            ),
        );
    }
}

fn child_path(path: &str, child: &str) -> String {
    format!("{path}/{}", child.replace('~', "~0").replace('/', "~1"))
}

fn relationship_path(path: &str, relative: &str) -> String {
    format!("{path}{relative}")
}

fn push(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    rule: ValidationRule,
    message: impl Into<String>,
) {
    if issues.len() < MAX_ISSUES {
        issues.push(ValidationIssue {
            path: path.to_owned(),
            rule,
            message: message.into(),
        });
    }
}
