//! Evaluates implications with one antecedent and a conjunctive set of consequences.
//!
//! For example:
//!
//! ```yaml
//! x-simile-constraints:
//!   - implies:
//!       when: { path: /active, equals: false }
//!       then:
//!         all:
//!           - { path: /published_at, state: null-or-absent }
//!           - { path: /status, equals: draft }
//! ```
//!
//! A false antecedent satisfies the implication vacuously. When it is true, proof requires every
//! consequence to match. Establishment groups consequences by path and asks the predicate
//! evaluator for one schema-valid witness that satisfies all predicates on that path; changes are
//! made on a clone and committed only when the complete implication is sound.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Violation, ViolationKind, comparison, invalid, predicate};
use crate::{Result, SchemaCatalog, SchemaId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Constraint {
    #[serde(rename = "w")]
    when: Condition,
    #[serde(rename = "t")]
    then: Vec<predicate::Predicate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Pending {
    field: String,
    when: PendingCondition,
    then: Vec<predicate::Pending>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Condition {
    #[serde(rename = "p")]
    Predicate(predicate::Predicate),
    #[serde(rename = "c")]
    Compare(comparison::Condition),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingCondition {
    Predicate(predicate::Pending),
    Compare(comparison::Pending),
}

pub(super) fn parse(definition: &Value, field: &str) -> Result<Pending> {
    let field = format!("{field}.implies");
    let implication = definition
        .as_object()
        .ok_or_else(|| invalid(&field, "implies must be an object"))?;
    if implication.len() != 2
        || !implication.contains_key("when")
        || !implication.contains_key("then")
    {
        return Err(invalid(
            &field,
            "implies requires exactly `when` and `then`",
        ));
    }
    let when_field = format!("{field}.when");
    let when_value = implication
        .get("when")
        .ok_or_else(|| invalid(&field, "when is required"))?;
    let when = if let Some(compare) = when_value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("compare"))
    {
        PendingCondition::Compare(comparison::parse(
            compare,
            &format!("{when_field}.compare"),
        )?)
    } else {
        PendingCondition::Predicate(predicate::parse(when_value, &when_field)?)
    };
    let then_field = format!("{field}.then");
    let then = implication
        .get("then")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid(&then_field, "then must be an object"))?;
    if then.len() != 1 || !then.contains_key("all") {
        return Err(invalid(
            &then_field,
            "then currently supports exactly one `all` array",
        ));
    }
    let assertions = then
        .get("all")
        .and_then(Value::as_array)
        .filter(|assertions| !assertions.is_empty())
        .ok_or_else(|| invalid(&format!("{then_field}.all"), "all must not be empty"))?;
    let then = assertions
        .iter()
        .enumerate()
        .map(|(index, assertion)| predicate::parse(assertion, &format!("{then_field}.all.{index}")))
        .collect::<Result<Vec<_>>>()?;
    Ok(Pending { field, when, then })
}

pub(super) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pending: Pending,
) -> Result<Constraint> {
    let when = match pending.when {
        PendingCondition::Predicate(pending) => {
            predicate::compile(catalog, root, pending).map(Condition::Predicate)?
        }
        PendingCondition::Compare(pending) => {
            comparison::compile(catalog, root, pending).map(Condition::Compare)?
        }
    };
    let then = pending
        .then
        .into_iter()
        .map(|predicate| predicate::compile(catalog, root, predicate))
        .collect::<Result<Vec<_>>>()?;
    if !predicate::compatible(catalog, root, &then) {
        return Err(invalid(
            &format!("{}.then.all", pending.field),
            "assertions for the same path cannot all be satisfied",
        ));
    }
    Ok(Constraint { when, then })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraint: &Constraint,
) -> bool {
    !constraint.then.is_empty()
        && constraint.when.is_well_formed(catalog, root)
        && constraint
            .then
            .iter()
            .all(|predicate| predicate::is_well_formed(catalog, root, predicate))
        && predicate::compatible(catalog, root, &constraint.then)
}

pub(super) fn prove(constraint: &Constraint, value: &Value) -> Vec<Violation> {
    if !constraint.when.matches(value) {
        return Vec::new();
    }
    constraint
        .then
        .iter()
        .filter(|predicate| !predicate.matches(value))
        .map(|predicate| Violation {
            kind: ViolationKind::Implies,
            path: predicate.path.clone(),
            message: format!(
                "value must {} when {}",
                predicate.requirement(),
                constraint.when.description()
            ),
        })
        .collect()
}

impl Condition {
    fn matches(&self, value: &Value) -> bool {
        match self {
            Self::Predicate(predicate) => predicate.matches(value),
            Self::Compare(comparison) => comparison.matches(value),
        }
    }

    fn is_well_formed(&self, catalog: &SchemaCatalog, root: SchemaId) -> bool {
        match self {
            Self::Predicate(predicate) => predicate::is_well_formed(catalog, root, predicate),
            Self::Compare(comparison) => comparison::is_well_formed(catalog, root, comparison),
        }
    }

    fn description(&self) -> String {
        match self {
            Self::Predicate(predicate) => {
                format!("`{}` {}", predicate.path, predicate.condition())
            }
            Self::Compare(comparison) => comparison.description(),
        }
    }
}

/// Establishes all triggered consequences transactionally and returns whether a repair was made.
pub(super) fn establish(constraint: &Constraint, value: &mut Value) -> bool {
    if !constraint.when.matches(value) || constraint.then.iter().all(|item| item.matches(value)) {
        return false;
    }

    let mut candidate = value.clone();
    let mut by_path = BTreeMap::<&str, Vec<&predicate::Predicate>>::new();
    for predicate in &constraint.then {
        by_path.entry(&predicate.path).or_default().push(predicate);
    }
    for predicates in by_path.into_values() {
        if predicates
            .iter()
            .all(|predicate| predicate.matches(&candidate))
        {
            continue;
        }
        let mut replacement = candidate.clone();
        if !predicate::establish_all(&predicates, &mut replacement) {
            return false;
        }
        candidate = replacement;
    }

    if !constraint
        .then
        .iter()
        .all(|predicate| predicate.matches(&candidate))
        || candidate == *value
    {
        return false;
    }
    *value = candidate;
    true
}
