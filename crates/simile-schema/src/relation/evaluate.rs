//! Applies compiled relations to responses and proves the resulting cross-message contract.

use serde_json::{Number, Value};

use super::{Expression, Plan, Rule};
use crate::soundness::path;
use crate::{RequestValues, SchemaCatalog, SchemaRef, SynthesisError};

impl Plan {
    pub(crate) fn apply(
        &self,
        catalog: &SchemaCatalog,
        response_schema: SchemaRef,
        request: &RequestValues<'_>,
        response: &mut Value,
    ) -> std::result::Result<(), SynthesisError> {
        for mapping in &self.mappings {
            let Some(source) =
                selected_source(request, &mapping.source).or_else(|| mapping.default.clone())
            else {
                continue;
            };
            let baseline = response.pointer(&mapping.target).cloned().or_else(|| {
                catalog
                    .synthesize_representative(SchemaRef(mapping.target_schema))
                    .ok()
            });
            let Some(baseline) = baseline else {
                return Err(unreachable(
                    &mapping.target,
                    "has no constructible response value",
                ));
            };
            let projected = catalog.project(SchemaRef(mapping.target_schema), &baseline, &source);
            if !catalog
                .validate(SchemaRef(mapping.target_schema), &projected)
                .is_empty()
                || !path::apply(response, &mapping.target, projected)
            {
                return Err(unreachable(
                    &mapping.target,
                    "cannot preserve its request value",
                ));
            }
        }
        for derivation in &self.derivations {
            let value = derivation
                .value
                .evaluate(request, request.body().unwrap_or(&Value::Null))
                .ok_or_else(|| unreachable(&derivation.target, "cannot be derived"))?;
            let target = path::resolve_property(catalog, response_schema.0, &derivation.target)
                .ok_or_else(|| unreachable(&derivation.target, "is not declared"))?;
            if !catalog
                .validate(SchemaRef(target.schema), &value)
                .is_empty()
                || !path::apply(response, &derivation.target, value)
            {
                return Err(unreachable(&derivation.target, "rejects its derived value"));
            }
        }
        for rule in &self.rules {
            if !rule.matches(request) {
                continue;
            }
            for (target, value) in &rule.set {
                if !path::apply(response, target, value.clone()) {
                    return Err(unreachable(target, "cannot be set"));
                }
            }
            for target in &rule.omit {
                path::remove(response, target);
            }
        }
        Ok(())
    }

    pub(crate) fn prove(
        &self,
        request: &RequestValues<'_>,
        response: &Value,
    ) -> std::result::Result<(), SynthesisError> {
        for mapping in &self.mappings {
            let Some(source) =
                selected_source(request, &mapping.source).or_else(|| mapping.default.clone())
            else {
                continue;
            };
            let target = response
                .pointer(&mapping.target)
                .ok_or_else(|| unreachable(&mapping.target, "is absent"))?;
            if !contains_request(target, &source) {
                return Err(unreachable(
                    &mapping.target,
                    "does not preserve the submitted request value",
                ));
            }
        }
        for derivation in &self.derivations {
            let expected = derivation
                .value
                .evaluate(request, request.body().unwrap_or(&Value::Null))
                .ok_or_else(|| unreachable(&derivation.target, "cannot be derived"))?;
            if response.pointer(&derivation.target) != Some(&expected) {
                return Err(unreachable(
                    &derivation.target,
                    "does not equal the declared derivation",
                ));
            }
        }
        for rule in &self.rules {
            if !rule.matches(request) {
                continue;
            }
            for (target, expected) in &rule.set {
                if response.pointer(target) != Some(expected) {
                    return Err(unreachable(target, "violates a triggered rule"));
                }
            }
            for target in &rule.omit {
                if response.pointer(target).is_some() {
                    return Err(unreachable(target, "must be absent for this request"));
                }
            }
        }
        Ok(())
    }
}

impl Expression {
    fn evaluate(&self, request: &RequestValues<'_>, context: &Value) -> Option<Value> {
        match self {
            Self::Request(source) => selected_source(request, source),
            Self::Relative(path) => selected_value(context, path),
            Self::Literal(value) => Some(value.clone()),
            Self::Add(values) => numeric_fold(values, request, context, Numeric::add),
            Self::Multiply(values) => numeric_fold(values, request, context, Numeric::multiply),
            Self::Concat(values) => values
                .iter()
                .map(|value| {
                    value
                        .evaluate(request, context)?
                        .as_str()
                        .map(str::to_owned)
                })
                .collect::<Option<String>>()
                .map(Value::String),
            Self::Substring { value, start, end } => value
                .evaluate(request, context)?
                .as_str()
                .and_then(|value| substring(value, *start, *end))
                .map(Value::String),
            Self::Mask {
                value,
                reveal,
                character,
            } => value
                .evaluate(request, context)?
                .as_str()
                .map(|value| mask(value, *reveal, *character))
                .map(Value::String),
            Self::Coalesce(values) => values.iter().find_map(|value| {
                value
                    .evaluate(request, context)
                    .filter(|value| !value.is_null())
            }),
            Self::Sum { each, value } => {
                let mut total = Numeric::Integer(0);
                for selected in path::select(request.body()?, each)? {
                    let value = value.evaluate(request, selected.value)?;
                    total = total.add(Numeric::from_value(&value)?)?;
                }
                total.into_value()
            }
        }
    }
}

fn substring(value: &str, start: i64, end: Option<i64>) -> Option<String> {
    let characters = value.chars().collect::<Vec<_>>();
    let length = i64::try_from(characters.len()).ok()?;
    let start = normalize_index(start, length)?;
    let end = normalize_index(end.unwrap_or(length), length)?;
    (start <= end).then(|| characters[start..end].iter().collect())
}

fn normalize_index(index: i64, length: i64) -> Option<usize> {
    let index = if index < 0 {
        length.checked_add(index)?
    } else {
        index
    };
    (0..=length)
        .contains(&index)
        .then(|| usize::try_from(index).ok())
        .flatten()
}

fn mask(value: &str, reveal: usize, character: char) -> String {
    let characters = value.chars().collect::<Vec<_>>();
    let hidden = characters.len().saturating_sub(reveal);
    std::iter::repeat_n(character, hidden)
        .chain(characters.into_iter().skip(hidden))
        .collect()
}

impl Rule {
    fn matches(&self, request: &RequestValues<'_>) -> bool {
        self.when.iter().all(|condition| {
            selected_source(request, &condition.source)
                .or_else(|| condition.default.clone())
                .as_ref()
                == Some(&condition.equals)
        })
    }
}

#[derive(Clone, Copy)]
enum Numeric {
    Integer(i128),
    Float(f64),
}

impl Numeric {
    fn from_value(value: &Value) -> Option<Self> {
        value
            .as_i64()
            .map(|value| Self::Integer(i128::from(value)))
            .or_else(|| value.as_u64().map(|value| Self::Integer(i128::from(value))))
            .or_else(|| {
                value
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .map(Self::Float)
            })
    }

    fn add(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Integer(left), Self::Integer(right)) => {
                left.checked_add(right).map(Self::Integer)
            }
            (left, right) => Some(Self::Float(left.as_f64() + right.as_f64())),
        }
    }

    fn multiply(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Integer(left), Self::Integer(right)) => {
                left.checked_mul(right).map(Self::Integer)
            }
            (left, right) => Some(Self::Float(left.as_f64() * right.as_f64())),
        }
    }

    const fn as_f64(self) -> f64 {
        match self {
            Self::Integer(value) => value as f64,
            Self::Float(value) => value,
        }
    }

    fn into_value(self) -> Option<Value> {
        match self {
            Self::Integer(value) => i64::try_from(value)
                .ok()
                .map(Number::from)
                .map(Value::Number),
            Self::Float(value) => Number::from_f64(value).map(Value::Number),
        }
    }
}

fn selected_value(value: &Value, selector: &str) -> Option<Value> {
    let selected = path::select(value, selector)?;
    let first = selected.first()?.value;
    selected
        .iter()
        .all(|selected| selected.value == first)
        .then(|| first.clone())
}

fn numeric_fold(
    values: &[Expression],
    request: &RequestValues<'_>,
    context: &Value,
    operation: fn(Numeric, Numeric) -> Option<Numeric>,
) -> Option<Value> {
    let mut values = values.iter();
    let first = Numeric::from_value(&values.next()?.evaluate(request, context)?)?;
    values
        .try_fold(first, |result, value| {
            operation(
                result,
                Numeric::from_value(&value.evaluate(request, context)?)?,
            )
        })?
        .into_value()
}

fn selected_source(request: &RequestValues<'_>, source: &super::Source) -> Option<Value> {
    match source {
        super::Source::Body(selector) => selected_value(request.body()?, selector),
        super::Source::Parameter(source) => {
            request.parameter(source.location, &source.name).cloned()
        }
    }
}

fn contains_request(response: &Value, request: &Value) -> bool {
    match (response, request) {
        (Value::Object(response), Value::Object(request)) => request.iter().all(|(key, value)| {
            response
                .get(key)
                .is_some_and(|response| contains_request(response, value))
        }),
        (Value::Array(response), Value::Array(request)) => {
            response.len() == request.len()
                && response
                    .iter()
                    .zip(request)
                    .all(|(response, request)| contains_request(response, request))
        }
        _ => response == request,
    }
}

fn unreachable(target: &str, reason: &str) -> SynthesisError {
    SynthesisError::Unreachable {
        reason: format!("response target `{target}` {reason}"),
    }
}
