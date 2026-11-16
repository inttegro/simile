use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde_json::{Map, Number, Value};
use thiserror::Error;

use crate::{
    AdditionalProperties, JsonType, SchemaCatalog, SchemaId, SchemaNode, SchemaRef,
    ValidationIssue, soundness, validate,
};

const MAX_DEPTH: usize = 64;
const MAX_VARIANTS: usize = 32;
const REPRESENTATIVE_DEPTH: usize = 2;
const REPRESENTATIVE_PROPERTIES: usize = 24;

/// A deterministic JSON value could not be produced from the compiled schema.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SynthesisError {
    #[error("compiled schema contains an invalid node")]
    InvalidSchema,

    #[error("could not synthesize a valid finite value within the deterministic search limits")]
    Exhausted { issues: Vec<ValidationIssue> },
}

impl SchemaCatalog {
    pub(crate) fn synthesize(&self, root: SchemaRef) -> std::result::Result<Value, SynthesisError> {
        self.synthesize_with(root, SynthesisMode::Minimal)
    }

    pub(crate) fn synthesize_representative(
        &self,
        root: SchemaRef,
    ) -> std::result::Result<Value, SynthesisError> {
        self.synthesize_with(root, SynthesisMode::Representative)
            .or_else(|_| self.synthesize(root))
    }

    pub(crate) fn synthesize_representative_matching(
        &self,
        root: SchemaRef,
        predicate: impl Fn(&Value) -> bool,
    ) -> std::result::Result<Value, SynthesisError> {
        self.synthesize_with_matching(root, SynthesisMode::Representative, &predicate)
            .or_else(|_| self.synthesize_with_matching(root, SynthesisMode::Minimal, &predicate))
    }

    pub(crate) fn synthesize_fuzz(
        &self,
        root: SchemaRef,
        seed: u64,
    ) -> std::result::Result<Value, SynthesisError> {
        if !self.contains(root) {
            return Err(SynthesisError::InvalidSchema);
        }
        let baseline = self.synthesize_representative(root)?;
        for attempt in 0..MAX_VARIANTS {
            let attempt_seed =
                seed.wrapping_add((attempt as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let mut randomizer = Randomizer {
                catalog: self,
                rng: StdRng::seed_from_u64(attempt_seed),
            };
            let mut active = Vec::new();
            let candidate = randomizer.node(root.0, &baseline, 0, &mut active);
            if validate::value(self, root, &candidate).is_empty() {
                return Ok(candidate);
            }
        }
        Ok(baseline)
    }

    fn synthesize_with(
        &self,
        root: SchemaRef,
        mode: SynthesisMode,
    ) -> std::result::Result<Value, SynthesisError> {
        self.synthesize_with_matching(root, mode, &|_| true)
    }

    fn synthesize_with_matching(
        &self,
        root: SchemaRef,
        mode: SynthesisMode,
        predicate: &impl Fn(&Value) -> bool,
    ) -> std::result::Result<Value, SynthesisError> {
        if !self.contains(root) {
            return Err(SynthesisError::InvalidSchema);
        }
        let mut closest = Vec::new();
        for variant in 0..MAX_VARIANTS {
            let mut active = Vec::new();
            if let Some(value) = (Synthesizer {
                catalog: self,
                mode,
            })
            .node(root.0, variant, 0, &mut active)
            {
                let issues = validate::value(self, root, &value);
                if issues.is_empty() && predicate(&value) {
                    return Ok(value);
                }
                if closest.is_empty() || issues.len() < closest.len() {
                    closest = issues;
                }
            }
        }
        Err(SynthesisError::Exhausted { issues: closest })
    }

    pub(crate) fn project(&self, root: SchemaRef, baseline: &Value, request: &Value) -> Value {
        if !self.contains(root) {
            return baseline.clone();
        }
        let mut active = Vec::new();
        let candidate = Projector { catalog: self }.node(root.0, baseline, request, 0, &mut active);
        if validate::value(self, root, &candidate).is_empty() {
            candidate
        } else {
            baseline.clone()
        }
    }
}

struct Synthesizer<'a> {
    catalog: &'a SchemaCatalog,
    mode: SynthesisMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SynthesisMode {
    Minimal,
    Representative,
}

struct Projector<'a> {
    catalog: &'a SchemaCatalog,
}

struct Randomizer<'a> {
    catalog: &'a SchemaCatalog,
    rng: StdRng,
}

impl Randomizer<'_> {
    fn node(
        &mut self,
        id: SchemaId,
        baseline: &Value,
        depth: usize,
        active: &mut Vec<SchemaId>,
    ) -> Value {
        if depth >= MAX_DEPTH || active.contains(&id) {
            return baseline.clone();
        }
        let Some(node) = self.catalog.nodes.get(id as usize).cloned() else {
            return baseline.clone();
        };
        if node.allowed == Some(false) {
            return baseline.clone();
        }
        if let Some(constant) = node.const_value {
            return constant;
        }
        if let Some(values) = &node.enum_values {
            if !values.is_empty() {
                return values[self.rng.random_range(0..values.len())].clone();
            }
        }

        active.push(id);
        let mut candidate = match baseline {
            Value::String(_) => Value::String(random_string(&node, &mut self.rng)),
            Value::Number(number) => {
                let integer = number.is_i64() || number.is_u64();
                random_number(&node, integer, &mut self.rng).unwrap_or_else(|| baseline.clone())
            }
            Value::Bool(_) => Value::Bool(self.rng.random()),
            Value::Array(values) => Value::Array(
                values
                    .iter()
                    .map(|value| {
                        node.items.map_or_else(
                            || value.clone(),
                            |schema| self.node(schema, value, depth + 1, active),
                        )
                    })
                    .collect(),
            ),
            Value::Object(object) => {
                let mut object = object.clone();
                for (name, schema) in &node.properties {
                    if let Some(value) = object.get(name).cloned() {
                        object.insert(name.clone(), self.node(*schema, &value, depth + 1, active));
                    }
                }
                Value::Object(object)
            }
            Value::Null => Value::Null,
        };

        for child in &node.all_of {
            candidate = self.node(*child, &candidate, depth + 1, active);
        }
        for child in node.any_of.iter().chain(&node.one_of) {
            if validate::value(self.catalog, SchemaRef(*child), baseline).is_empty() {
                candidate = self.node(*child, &candidate, depth + 1, active);
                break;
            }
        }
        soundness::establish(&node, &mut candidate);
        active.pop();
        candidate
    }
}

impl Projector<'_> {
    fn node(
        &self,
        id: SchemaId,
        baseline: &Value,
        request: &Value,
        depth: usize,
        active: &mut Vec<SchemaId>,
    ) -> Value {
        if depth >= MAX_DEPTH || active.contains(&id) {
            return baseline.clone();
        }
        let Some(node) = self.catalog.nodes.get(id as usize) else {
            return baseline.clone();
        };

        if !baseline.is_object() || !request.is_object() {
            return if validate::value(self.catalog, SchemaRef(id), request).is_empty() {
                request.clone()
            } else {
                baseline.clone()
            };
        }

        active.push(id);
        let mut projected = baseline.clone();
        for (name, schema) in &node.properties {
            let Some(request_value) = request.get(name) else {
                continue;
            };
            let current = projected.get(name);
            let replacement = if current.is_some_and(Value::is_object) && request_value.is_object()
            {
                current.map(|current| self.node(*schema, current, request_value, depth + 1, active))
            } else if validate::value(self.catalog, SchemaRef(*schema), request_value).is_empty() {
                Some(request_value.clone())
            } else {
                None
            };
            let Some(replacement) = replacement else {
                continue;
            };
            let mut candidate = projected.clone();
            if let Some(object) = candidate.as_object_mut() {
                object.insert(name.clone(), replacement);
            }
            if validate::value(self.catalog, SchemaRef(id), &candidate).is_empty() {
                projected = candidate;
            }
        }

        for schema in &node.all_of {
            let candidate = self.node(*schema, &projected, request, depth + 1, active);
            if validate::value(self.catalog, SchemaRef(id), &candidate).is_empty() {
                projected = candidate;
            }
        }
        for schema in node.any_of.iter().chain(&node.one_of) {
            let candidate = self.node(*schema, &projected, request, depth + 1, active);
            if validate::value(self.catalog, SchemaRef(id), &candidate).is_empty() {
                projected = candidate;
                break;
            }
        }
        active.pop();
        projected
    }
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

        candidates.into_iter().find_map(|mut candidate| {
            soundness::establish(node, &mut candidate);
            validate::value(self.catalog, SchemaRef(id), &candidate)
                .is_empty()
                .then_some(candidate)
        })
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
        let minimum = node.min_properties.unwrap_or(0).max(node.required.len());
        let target = match self.mode {
            SynthesisMode::Minimal => minimum,
            SynthesisMode::Representative if depth < REPRESENTATIVE_DEPTH => minimum.max(
                node.properties
                    .len()
                    .min(maximum)
                    .min(REPRESENTATIVE_PROPERTIES),
            ),
            SynthesisMode::Representative => minimum,
        };
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
    if let Some(value) = node.format.as_deref().and_then(formatted_string) {
        return Some(value.to_owned());
    }
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
    if integer {
        integer_number(value).map(Value::Number)
    } else {
        Number::from_f64(value).map(Value::Number)
    }
}

fn random_number(node: &SchemaNode, integer: bool, rng: &mut StdRng) -> Option<Value> {
    for _ in 0..MAX_VARIANTS {
        if let Some(value) = number(node, rng.random_range(0..MAX_VARIANTS), integer) {
            return Some(value);
        }
    }
    number(node, 0, integer)
}

fn random_string(node: &SchemaNode, rng: &mut StdRng) -> String {
    let minimum = node.min_length.unwrap_or(0);
    let maximum = node.max_length.unwrap_or(usize::MAX);
    if let Some(value) = node
        .format
        .as_deref()
        .and_then(|format| random_formatted_string(format, rng))
    {
        if (minimum..=maximum).contains(&value.chars().count()) {
            return value;
        }
    }

    let lower = minimum.max(8).min(maximum);
    let upper = minimum.saturating_add(16).max(lower).min(maximum);
    let length = if lower < upper {
        rng.random_range(lower..=upper)
    } else {
        lower
    };
    const ALPHANUMERIC: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    (0..length)
        .map(|_| ALPHANUMERIC[rng.random_range(0..ALPHANUMERIC.len())] as char)
        .collect()
}

fn random_formatted_string(format: &str, rng: &mut StdRng) -> Option<String> {
    match format {
        "date" => Some(format!(
            "{:04}-{:02}-{:02}",
            rng.random_range(2000..=2038),
            rng.random_range(1..=12),
            rng.random_range(1..=28)
        )),
        "date-time" => Some(format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            rng.random_range(2000..=2038),
            rng.random_range(1..=12),
            rng.random_range(1..=28),
            rng.random_range(0..=23),
            rng.random_range(0..=59),
            rng.random_range(0..=59)
        )),
        "time" => Some(format!(
            "{:02}:{:02}:{:02}Z",
            rng.random_range(0..=23),
            rng.random_range(0..=59),
            rng.random_range(0..=59)
        )),
        "duration" => Some(format!("PT{}S", rng.random_range(1..=86_400))),
        "email" | "idn-email" => Some(format!("{}@example.test", random_token(rng, 10))),
        "hostname" | "idn-hostname" => Some(format!("{}.example.test", random_token(rng, 10))),
        "ipv4" => Some(format!("192.0.2.{}", rng.random_range(1..=254))),
        "ipv6" => Some(format!(
            "2001:db8::{:x}",
            rng.random_range(1_u16..=u16::MAX)
        )),
        "uuid" => {
            let mut bytes = [0_u8; 16];
            rng.fill(&mut bytes);
            bytes[6] = (bytes[6] & 0x0f) | 0x40;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            Some(format!(
                "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                bytes[0],
                bytes[1],
                bytes[2],
                bytes[3],
                bytes[4],
                bytes[5],
                bytes[6],
                bytes[7],
                bytes[8],
                bytes[9],
                bytes[10],
                bytes[11],
                bytes[12],
                bytes[13],
                bytes[14],
                bytes[15]
            ))
        }
        "uri" | "uri-reference" | "iri" | "iri-reference" => {
            let first = random_token(rng, 10);
            let second = random_token(rng, 10);
            Some(format!("https://example.test/{first}/{second}"))
        }
        "uri-template" => Some(format!(
            "https://example.test/{}/{{id}}",
            random_token(rng, 10)
        )),
        "json-pointer" => Some(format!("/{}", random_token(rng, 10))),
        "relative-json-pointer" => Some(format!("0/{}", random_token(rng, 10))),
        "regex" => Some(format!("^{}$", random_token(rng, 10))),
        "byte" => Some(random_base64(rng)),
        "binary" | "password" => Some(random_token(rng, 16)),
        "currency" => {
            const CURRENCIES: [&str; 6] = ["usd", "eur", "gbp", "ghs", "kes", "ngn"];
            Some(CURRENCIES[rng.random_range(0..CURRENCIES.len())].to_owned())
        }
        "decimal" => Some(format!(
            "{}.{}",
            rng.random_range(1..=9_999),
            rng.random_range(0..=99)
        )),
        _ => None,
    }
}

fn random_token(rng: &mut StdRng, length: usize) -> String {
    const ALPHANUMERIC: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    (0..length)
        .map(|_| ALPHANUMERIC[rng.random_range(0..ALPHANUMERIC.len())] as char)
        .collect()
}

fn random_base64(rng: &mut StdRng) -> String {
    const BASE64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    (0..8)
        .map(|_| BASE64[rng.random_range(0..BASE64.len())] as char)
        .collect()
}

fn integer_number(value: f64) -> Option<Number> {
    if value >= 0.0 {
        let integer = value as u64;
        ((integer as f64) == value).then(|| Number::from(integer))
    } else {
        let integer = value as i64;
        ((integer as f64) == value).then(|| Number::from(integer))
    }
}

fn formatted_string(format: &str) -> Option<&'static str> {
    match format {
        "date" => Some("1970-01-01"),
        "date-time" => Some("1970-01-01T00:00:00Z"),
        "time" => Some("00:00:00Z"),
        "duration" => Some("PT0S"),
        "email" | "idn-email" => Some("user@example.com"),
        "hostname" | "idn-hostname" => Some("example.com"),
        "ipv4" => Some("192.0.2.1"),
        "ipv6" => Some("2001:db8::1"),
        "uuid" => Some("00000000-0000-0000-0000-000000000000"),
        "uri" | "uri-reference" | "iri" | "iri-reference" => Some("https://example.com/"),
        "uri-template" => Some("https://example.com/{id}"),
        "json-pointer" => Some("/example"),
        "relative-json-pointer" => Some("0"),
        "regex" => Some(".*"),
        "byte" => Some("AA=="),
        "binary" | "password" => Some("example"),
        "currency" => Some("usd"),
        "decimal" => Some("0"),
        _ => None,
    }
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
