//! Evaluates field predicates shared by implication consequences and aggregate guards.
//!
//! Examples:
//!
//! ```yaml
//! # Exact value
//! { path: /status, equals: paid }
//!
//! # Presence state; missing and JSON null are intentionally different
//! { path: /paid_at, state: non-null }
//!
//! # Array membership
//! { path: /payment_method_types, contains: card }
//! ```
//!
//! Compilation records a schema-valid witness for each predicate. `matches` classifies a field as
//! missing or present-with-value and applies `equals`, `state`, or `contains` literally.
//! `establish_all` searches for one witness satisfying every predicate on the same path; for
//! multiple `contains` predicates it can merge required items into one schema-valid array. The
//! candidate is applied to a clone first, so an incompatible conjunction never partially mutates
//! the payload.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{invalid, path};
use crate::{Result, SchemaCatalog, SchemaId, SchemaRef, validate};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct Predicate {
    #[serde(rename = "p")]
    pub path: String,
    #[serde(rename = "t")]
    test: Test,
    #[serde(rename = "w")]
    witness: Witness,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Test {
    #[serde(rename = "s")]
    State(State),
    #[serde(rename = "e")]
    Equals(Value),
    #[serde(rename = "c")]
    Contains(Value),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum State {
    #[serde(rename = "p")]
    Present,
    #[serde(rename = "a")]
    Absent,
    #[serde(rename = "z")]
    Null,
    #[serde(rename = "n")]
    NonNull,
    #[serde(rename = "o")]
    NullOrAbsent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Witness {
    #[serde(rename = "m")]
    Missing,
    #[serde(rename = "v")]
    Value(Value),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Pending {
    field: String,
    path: String,
    test: Test,
}

pub(super) fn parse(value: &Value, field: &str) -> Result<Pending> {
    let predicate = value
        .as_object()
        .ok_or_else(|| invalid(field, "predicate must be an object"))?;
    let pointer = predicate
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid(
                &format!("{field}.path"),
                "path must be a JSON Pointer string",
            )
        })?;
    path::segments(pointer).map_err(|reason| invalid(&format!("{field}.path"), &reason))?;

    let operators = predicate
        .keys()
        .filter(|key| key.as_str() != "path")
        .collect::<Vec<_>>();
    if operators.len() != 1 {
        return Err(invalid(
            field,
            "predicate requires `path` and exactly one of `state`, `equals`, or `contains`",
        ));
    }
    let test = match operators[0].as_str() {
        "state" => Test::State(parse_state(
            predicate
                .get("state")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid(&format!("{field}.state"), "state must be a string"))?,
            &format!("{field}.state"),
        )?),
        "equals" => Test::Equals(
            predicate
                .get("equals")
                .cloned()
                .ok_or_else(|| invalid(&format!("{field}.equals"), "equals is required"))?,
        ),
        "contains" => Test::Contains(
            predicate
                .get("contains")
                .cloned()
                .ok_or_else(|| invalid(&format!("{field}.contains"), "contains is required"))?,
        ),
        operator => {
            return Err(invalid(
                field,
                &format!("unsupported predicate operator `{operator}`"),
            ));
        }
    };
    Ok(Pending {
        field: field.to_owned(),
        path: pointer.to_owned(),
        test,
    })
}

pub(super) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pending: Pending,
) -> Result<Predicate> {
    let resolved = path::resolve_property(catalog, root, &pending.path).ok_or_else(|| {
        invalid(
            &pending.field,
            &format!("path `{}` does not resolve to a property", pending.path),
        )
    })?;
    let witness =
        witness(catalog, resolved.schema, resolved.required, &pending.test).ok_or_else(|| {
            invalid(
                &pending.field,
                &format!(
                    "predicate on `{}` cannot be satisfied by its declared schema",
                    pending.path
                ),
            )
        })?;
    Ok(Predicate {
        path: pending.path,
        test: pending.test,
        witness,
    })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    predicate: &Predicate,
) -> bool {
    path::resolve_property(catalog, root, &predicate.path).is_some_and(|resolved| {
        witness_is_valid(
            catalog,
            resolved.schema,
            resolved.required,
            &predicate.test,
            &predicate.witness,
        )
    })
}

pub(super) fn compatible(
    catalog: &SchemaCatalog,
    root: SchemaId,
    predicates: &[Predicate],
) -> bool {
    let mut by_path = std::collections::BTreeMap::<&str, Vec<&Predicate>>::new();
    for predicate in predicates {
        by_path.entry(&predicate.path).or_default().push(predicate);
    }
    by_path.into_iter().all(|(pointer, predicates)| {
        let Some(resolved) = path::resolve_property(catalog, root, pointer) else {
            return false;
        };
        witness_candidates(&predicates).into_iter().any(|witness| {
            predicates
                .iter()
                .all(|predicate| predicate.test.matches_witness(&witness))
                && witness_is_valid(
                    catalog,
                    resolved.schema,
                    resolved.required,
                    &predicates[0].test,
                    &witness,
                )
        })
    })
}

pub(super) fn establish_all(predicates: &[&Predicate], value: &mut Value) -> bool {
    let Some(path) = predicates.first().map(|predicate| predicate.path.as_str()) else {
        return false;
    };
    if predicates.iter().any(|predicate| predicate.path != path) {
        return false;
    }
    for witness in witness_candidates(predicates) {
        let mut trial = value.clone();
        apply_witness(&mut trial, path, &witness);
        if predicates.iter().all(|predicate| predicate.matches(&trial)) {
            if trial == *value {
                return false;
            }
            *value = trial;
            return true;
        }
    }
    false
}

impl Predicate {
    /// Evaluates this predicate while preserving the distinction between missing and JSON null.
    pub(super) fn matches(&self, value: &Value) -> bool {
        let witness = value
            .pointer(&self.path)
            .map_or(Witness::Missing, |value| Witness::Value(value.clone()));
        self.test.matches_witness(&witness)
    }

    pub(super) fn requirement(&self) -> String {
        match &self.test {
            Test::Equals(value) => format!("equal {value}"),
            Test::Contains(value) => format!("contain {value}"),
            Test::State(state) => state.requirement().to_owned(),
        }
    }

    pub(super) fn condition(&self) -> String {
        match &self.test {
            Test::Equals(value) => format!("equals {value}"),
            Test::Contains(value) => format!("contains {value}"),
            Test::State(state) => state.condition().to_owned(),
        }
    }
}

fn parse_state(value: &str, field: &str) -> Result<State> {
    match value {
        "present" => Ok(State::Present),
        "absent" => Ok(State::Absent),
        "null" => Ok(State::Null),
        "non-null" => Ok(State::NonNull),
        "null-or-absent" => Ok(State::NullOrAbsent),
        _ => Err(invalid(
            field,
            "state must be `present`, `absent`, `null`, `non-null`, or `null-or-absent`",
        )),
    }
}

fn witness(
    catalog: &SchemaCatalog,
    schema: SchemaId,
    required: bool,
    test: &Test,
) -> Option<Witness> {
    let candidate = |predicate: &dyn Fn(&Value) -> bool| {
        catalog
            .synthesize_representative_matching(SchemaRef(schema), predicate)
            .ok()
            .map(Witness::Value)
    };
    let witness = match test {
        Test::Equals(value) => Witness::Value(value.clone()),
        Test::Contains(expected) => Witness::Value(contains_witness(catalog, schema, expected)?),
        Test::State(State::Present) => candidate(&|_| true)?,
        Test::State(State::Absent) if !required => Witness::Missing,
        Test::State(State::Null) => Witness::Value(Value::Null),
        Test::State(State::NonNull) => candidate(&|value| !value.is_null())?,
        Test::State(State::NullOrAbsent) if !required => Witness::Missing,
        Test::State(State::NullOrAbsent) => Witness::Value(Value::Null),
        Test::State(State::Absent) => return None,
    };
    witness_is_valid(catalog, schema, required, test, &witness).then_some(witness)
}

fn contains_witness(catalog: &SchemaCatalog, schema: SchemaId, expected: &Value) -> Option<Value> {
    let mut candidates = Vec::new();
    if let Ok(Value::Array(mut values)) = catalog.synthesize_representative(SchemaRef(schema)) {
        if !values.contains(expected) {
            values.push(expected.clone());
        }
        candidates.push(Value::Array(values));
    }
    candidates.push(Value::Array(vec![expected.clone()]));
    candidates
        .into_iter()
        .find(|candidate| validate::value(catalog, SchemaRef(schema), candidate).is_empty())
}

fn witness_is_valid(
    catalog: &SchemaCatalog,
    schema: SchemaId,
    required: bool,
    test: &Test,
    witness: &Witness,
) -> bool {
    if !test.matches_witness(witness) {
        return false;
    }
    match witness {
        Witness::Missing => !required,
        Witness::Value(value) => validate::value(catalog, SchemaRef(schema), value).is_empty(),
    }
}

fn witness_candidates(predicates: &[&Predicate]) -> Vec<Witness> {
    let mut witnesses = predicates
        .iter()
        .map(|predicate| predicate.witness.clone())
        .collect::<Vec<_>>();
    let mut combined = Vec::new();
    for witness in &witnesses {
        if let Witness::Value(Value::Array(values)) = witness {
            for value in values {
                if !combined.contains(value) {
                    combined.push(value.clone());
                }
            }
        }
    }
    if !combined.is_empty() {
        witnesses.push(Witness::Value(Value::Array(combined)));
    }
    witnesses.push(Witness::Value(Value::Null));
    witnesses.push(Witness::Missing);
    witnesses.dedup();
    witnesses
}

fn apply_witness(value: &mut Value, pointer: &str, witness: &Witness) -> bool {
    match witness {
        Witness::Missing => path::remove(value, pointer),
        Witness::Value(replacement) => path::apply(value, pointer, replacement.clone()),
    }
}

impl Test {
    fn matches_witness(&self, witness: &Witness) -> bool {
        match (self, witness) {
            (Self::Equals(expected), Witness::Value(actual)) => expected == actual,
            (Self::Contains(expected), Witness::Value(Value::Array(actual))) => {
                actual.contains(expected)
            }
            (Self::Equals(_) | Self::Contains(_), Witness::Missing | Witness::Value(_)) => false,
            (Self::State(state), witness) => state.matches(witness),
        }
    }
}

impl State {
    fn matches(self, witness: &Witness) -> bool {
        match (self, witness) {
            (Self::Present, Witness::Value(_))
            | (Self::Absent, Witness::Missing)
            | (Self::Null, Witness::Value(Value::Null)) => true,
            (Self::NonNull, Witness::Value(value)) => !value.is_null(),
            (Self::NullOrAbsent, Witness::Missing | Witness::Value(Value::Null)) => true,
            _ => false,
        }
    }

    const fn requirement(self) -> &'static str {
        match self {
            Self::Present => "be present",
            Self::Absent => "be absent",
            Self::Null => "be null",
            Self::NonNull => "be present and non-null",
            Self::NullOrAbsent => "be null or absent",
        }
    }

    const fn condition(self) -> &'static str {
        match self {
            Self::Present => "is present",
            Self::Absent => "is absent",
            Self::Null => "is null",
            Self::NonNull => "is present and non-null",
            Self::NullOrAbsent => "is null or absent",
        }
    }
}
