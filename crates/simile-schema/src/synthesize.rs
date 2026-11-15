use serde_json::{Map, Number, Value};
use thiserror::Error;

use crate::{
    AdditionalProperties, JsonType, SchemaCatalog, SchemaId, SchemaNode, SchemaRef,
    ValidationIssue, validate,
};

const MAX_DEPTH: usize = 64;
const MAX_VARIANTS: usize = 32;

/// A deterministic request value could not be produced from the compiled schema.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SynthesisError {
    #[error("compiled schema contains an invalid node")]
    InvalidSchema,

    #[error("could not synthesize a valid finite value within the deterministic search limits")]
    Exhausted { issues: Vec<ValidationIssue> },
}

impl SchemaCatalog {
    pub(crate) fn synthesize(&self, root: SchemaRef) -> std::result::Result<Value, SynthesisError> {
        if !self.contains(root) {
            return Err(SynthesisError::InvalidSchema);
        }
        let mut closest = Vec::new();
        for variant in 0..MAX_VARIANTS {
            let mut active = Vec::new();
            if let Some(value) =
                (Synthesizer { catalog: self }).node(root.0, variant, 0, &mut active)
            {
                let issues = validate::value(self, root, &value);
                if issues.is_empty() {
                    return Ok(value);
                }
                if closest.is_empty() || issues.len() < closest.len() {
                    closest = issues;
                }
            }
        }
        Err(SynthesisError::Exhausted { issues: closest })
    }
}

struct Synthesizer<'a> {
    catalog: &'a SchemaCatalog,
}

impl Synthesizer<'_> {
    fn node(
        &self,
        id: SchemaId,
        variant: usize,
        depth: usize,
        active: &mut Vec<SchemaId>,
    ) -> Option<Value> {
        if depth >= MAX_DEPTH || active.contains(&id) {
            return None;
        }
        let node = self.catalog.nodes.get(id as usize)?;
        if node.allowed == Some(false) {
            return None;
        }

        active.push(id);
        let candidates = self.candidates(node, variant, depth, active);
        active.pop();

        candidates
            .into_iter()
            .find(|candidate| validate::value(self.catalog, SchemaRef(id), candidate).is_empty())
    }

    fn candidates(
        &self,
        node: &SchemaNode,
        variant: usize,
        depth: usize,
        active: &mut Vec<SchemaId>,
    ) -> Vec<Value> {
        let mut candidates = Vec::new();
        if let Some(hint) = &node.hint {
            push_unique(&mut candidates, hint.clone());
        }
        if let Some(constant) = &node.const_value {
            push_unique(&mut candidates, constant.clone());
        }
        if let Some(values) = &node.enum_values {
            if !values.is_empty() {
                for offset in 0..values.len() {
                    push_unique(
                        &mut candidates,
                        values[(variant + offset) % values.len()].clone(),
                    );
                }
            }
        }

        let mut base = Vec::new();
        for kind in candidate_types(node) {
            if let Some(value) = self.typed(node, kind, variant, depth, active) {
                push_unique(&mut base, value);
            }
        }
        for value in &base {
            push_unique(&mut candidates, value.clone());
        }

        let all = node
            .all_of
            .iter()
            .filter_map(|child| self.node(*child, variant, depth + 1, active))
            .collect::<Vec<_>>();
        for value in &all {
            push_unique(&mut candidates, value.clone());
        }
        if all.len() == node.all_of.len() && !all.is_empty() {
            let seed = base
                .iter()
                .find(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            if let Some(merged) = merge_objects(seed, &all) {
                push_unique(&mut candidates, merged);
            }
        }

        for child in node.any_of.iter().chain(&node.one_of) {
            if let Some(value) = self.node(*child, variant, depth + 1, active) {
                push_unique(&mut candidates, value.clone());
                for base_value in &base {
                    if let Some(merged) = merge_objects(base_value.clone(), &[value.clone()]) {
                        push_unique(&mut candidates, merged);
                    }
                }
            }
        }
        candidates
    }

    fn typed(
        &self,
        node: &SchemaNode,
        kind: JsonType,
        variant: usize,
        depth: usize,
        active: &mut Vec<SchemaId>,
    ) -> Option<Value> {
        match kind {
            JsonType::Null => Some(Value::Null),
            JsonType::Boolean => Some(Value::Bool(variant % 2 == 0)),
            JsonType::Object => self.object(node, variant, depth, active),
            JsonType::Array => self.array(node, variant, depth, active),
            JsonType::Number => number(node, variant, false),
            JsonType::Integer => number(node, variant, true),
            JsonType::String => string(node, variant).map(Value::String),
        }
    }

    fn object(
        &self,
        node: &SchemaNode,
        variant: usize,
        depth: usize,
        active: &mut Vec<SchemaId>,
    ) -> Option<Value> {
        let maximum = node.max_properties.unwrap_or(usize::MAX);
        if node.required.len() > maximum {
            return None;
        }
        let target = node.min_properties.unwrap_or(0).max(node.required.len());
        if target > maximum {
            return None;
        }

        let mut object = Map::new();
        for name in &node.required {
            let schema = node.properties.get(name)?;
            let value = self.node(*schema, variant, depth + 1, active)?;
            object.insert(name.clone(), value);
        }
        for (name, schema) in &node.properties {
            if object.len() >= target {
                break;
            }
            if object.contains_key(name) {
                continue;
            }
            if let Some(value) = self.node(*schema, variant, depth + 1, active) {
                object.insert(name.clone(), value);
            }
        }
        let mut extra = 0_usize;
        while object.len() < target {
            let value = match node.additional {
                AdditionalProperties::Allow => Value::Null,
                AdditionalProperties::Deny => return None,
                AdditionalProperties::Schema(schema) => {
                    self.node(schema, variant + extra, depth + 1, active)?
                }
            };
            let name = format!("additional{extra}");
            object.insert(name, value);
            extra += 1;
        }
        Some(Value::Object(object))
    }

    fn array(
        &self,
        node: &SchemaNode,
        variant: usize,
        depth: usize,
        active: &mut Vec<SchemaId>,
    ) -> Option<Value> {
        let mut length = node.min_items.unwrap_or(0);
        if length == 0 && node.items.is_some() && node.max_items != Some(0) {
            length = 1;
        }
        if node.max_items.is_some_and(|maximum| length > maximum) {
            return None;
        }
        let mut values = Vec::with_capacity(length);
        for index in 0..length {
            let value = node.items.map_or(Some(Value::Null), |schema| {
                self.node(schema, variant + index, depth + 1, active)
            })?;
            if node.unique_items && values.contains(&value) {
                return None;
            }
            values.push(value);
        }
        Some(Value::Array(values))
    }
}

fn candidate_types(node: &SchemaNode) -> Vec<JsonType> {
    let preferred = [
        JsonType::Object,
        JsonType::Array,
        JsonType::String,
        JsonType::Integer,
        JsonType::Number,
        JsonType::Boolean,
        JsonType::Null,
    ];
    if !node.types.is_empty() {
        return preferred
            .into_iter()
            .filter(|kind| node.types.contains(kind))
            .collect();
    }
    if !node.properties.is_empty()
        || !node.required.is_empty()
        || node.min_properties.is_some()
        || node.max_properties.is_some()
    {
        return vec![JsonType::Object];
    }
    if node.items.is_some() || node.min_items.is_some() || node.max_items.is_some() {
        return vec![JsonType::Array];
    }
    if node.min_length.is_some() || node.max_length.is_some() {
        return vec![JsonType::String];
    }
    if node.minimum.is_some() || node.maximum.is_some() || node.multiple_of.is_some() {
        return vec![JsonType::Number, JsonType::Integer];
    }
    preferred.to_vec()
}

fn string(node: &SchemaNode, variant: usize) -> Option<String> {
    let minimum = node.min_length.unwrap_or(0);
    let maximum = node.max_length.unwrap_or(usize::MAX);
    if minimum > maximum {
        return None;
    }
    let seeds = ["example", "sample", "value", "test", "item", "request"];
    let seed = seeds[variant % seeds.len()];
    let length = minimum.max(seed.chars().count()).min(maximum);
    let mut value = seed.chars().take(length).collect::<String>();
    value.extend(std::iter::repeat_n('x', length - value.chars().count()));
    Some(value)
}

fn number(node: &SchemaNode, variant: usize, integer: bool) -> Option<Value> {
    let step = node.multiple_of.unwrap_or(1.0);
    let mut value = 0.0_f64;
    if let Some(lower) = node.minimum {
        value = lower.value;
        if lower.exclusive {
            value += step;
        }
    } else if let Some(upper) = node.maximum {
        if value > upper.value || (upper.exclusive && value >= upper.value) {
            value = upper.value - if upper.exclusive { step } else { 0.0 };
        }
    }
    if node.multiple_of.is_some() {
        value = (value / step).ceil() * step;
        if node
            .minimum
            .is_some_and(|lower| value < lower.value || (lower.exclusive && value <= lower.value))
        {
            value += step;
        }
    }
    if integer {
        value = value.ceil();
    }
    value += step * variant as f64;
    if integer && value.fract() != 0.0 {
        value = value.ceil();
    }
    if !value.is_finite()
        || node
            .minimum
            .is_some_and(|lower| value < lower.value || (lower.exclusive && value <= lower.value))
        || node
            .maximum
            .is_some_and(|upper| value > upper.value || (upper.exclusive && value >= upper.value))
    {
        return None;
    }
    Number::from_f64(value).map(Value::Number)
}

fn merge_objects(mut seed: Value, values: &[Value]) -> Option<Value> {
    let target = seed.as_object_mut()?;
    for value in values {
        for (name, value) in value.as_object()? {
            target.insert(name.clone(), value.clone());
        }
    }
    Some(seed)
}

fn push_unique(values: &mut Vec<Value>, value: Value) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn candidate_type_order_is_stable() {
        let node = SchemaNode {
            types: BTreeSet::from([JsonType::String, JsonType::Object]),
            ..SchemaNode::default()
        };
        assert_eq!(
            candidate_types(&node),
            vec![JsonType::Object, JsonType::String]
        );
    }
}
