//! Evaluates a finite lifecycle snapshot against a declared transition graph.
//!
//! For example:
//!
//! ```yaml
//! x-simile-constraints:
//!   - lifecycle:
//!       state: /status
//!       created-at: /created_at
//!       transitions:
//!         - { from: draft, to: open, at: /opened_at }
//!         - { from: open, to: paid, at: /paid_at }
//! ```
//!
//! Compilation requires enum states, an acyclic graph with at least one root, and compatible
//! numeric or RFC 3339 timestamps. For the current state, proof accepts any declared path from a
//! root whose used timestamps are populated and ordered after `created-at`; timestamps outside
//! that path must be null or absent. Establishment chooses the first deterministic compiled path,
//! sets its timestamps to a common valid anchor, and clears every inactive transition timestamp.
//! This models a sound current snapshot, not a historical event log.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Violation, ViolationKind, invalid, ordered, path, scalar};
use crate::{Result, SchemaCatalog, SchemaId};

const MAX_PATHS: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Constraint {
    #[serde(rename = "s")]
    state: String,
    #[serde(rename = "c")]
    created_at: String,
    #[serde(rename = "e")]
    edges: Vec<Edge>,
    #[serde(rename = "t")]
    timestamps: Vec<Timestamp>,
    #[serde(rename = "p")]
    plans: Vec<StatePlan>,
    #[serde(rename = "k")]
    kind: ordered::Kind,
    #[serde(rename = "a")]
    anchor: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Edge {
    #[serde(rename = "f")]
    from: Value,
    #[serde(rename = "t")]
    to: Value,
    #[serde(rename = "a")]
    at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Timestamp {
    #[serde(rename = "p")]
    path: String,
    #[serde(rename = "c")]
    clear: Clear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Clear {
    #[serde(rename = "m")]
    Missing,
    #[serde(rename = "n")]
    Null,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct StatePlan {
    #[serde(rename = "s")]
    state: Value,
    #[serde(rename = "p")]
    paths: Vec<Vec<usize>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Pending {
    field: String,
    state: String,
    created_at: String,
    edges: Vec<Edge>,
}

pub(super) fn parse(definition: &Value, field: &str) -> Result<Pending> {
    let field = format!("{field}.lifecycle");
    let lifecycle = definition
        .as_object()
        .ok_or_else(|| invalid(&field, "lifecycle must be an object"))?;
    if lifecycle.len() != 3
        || !lifecycle.contains_key("state")
        || !lifecycle.contains_key("created-at")
        || !lifecycle.contains_key("transitions")
    {
        return Err(invalid(
            &field,
            "lifecycle requires exactly `state`, `created-at`, and `transitions`",
        ));
    }
    let state = pointer(lifecycle.get("state"), &format!("{field}.state"))?;
    let created_at = pointer(lifecycle.get("created-at"), &format!("{field}.created-at"))?;
    let transitions = lifecycle
        .get("transitions")
        .and_then(Value::as_array)
        .filter(|transitions| !transitions.is_empty())
        .ok_or_else(|| {
            invalid(
                &format!("{field}.transitions"),
                "transitions must be a non-empty array",
            )
        })?;
    let mut edges = Vec::new();
    for (index, transition) in transitions.iter().enumerate() {
        let transition_field = format!("{field}.transitions.{index}");
        let transition = transition
            .as_object()
            .ok_or_else(|| invalid(&transition_field, "transition must be an object"))?;
        if transition.len() != 3
            || !transition.contains_key("from")
            || !transition.contains_key("to")
            || !transition.contains_key("at")
        {
            return Err(invalid(
                &transition_field,
                "transition requires exactly `from`, `to`, and `at`",
            ));
        }
        let from = states(
            transition
                .get("from")
                .ok_or_else(|| invalid(&transition_field, "from is required"))?,
            &format!("{transition_field}.from"),
        )?;
        let to = state_value(
            transition
                .get("to")
                .ok_or_else(|| invalid(&transition_field, "to is required"))?,
            &format!("{transition_field}.to"),
        )?;
        let at = pointer(transition.get("at"), &format!("{transition_field}.at"))?;
        for from in from {
            if from == to {
                return Err(invalid(
                    &transition_field,
                    "a lifecycle transition cannot lead to the same state",
                ));
            }
            edges.push(Edge {
                from,
                to: to.clone(),
                at: at.clone(),
            });
        }
    }
    if edges
        .iter()
        .enumerate()
        .any(|(index, edge)| edges[..index].iter().any(|candidate| candidate == edge))
    {
        return Err(invalid(
            &format!("{field}.transitions"),
            "transitions must not contain duplicate edges",
        ));
    }
    Ok(Pending {
        field,
        state,
        created_at,
        edges,
    })
}

pub(super) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pending: Pending,
) -> Result<Constraint> {
    let state = path::resolve_property(catalog, root, &pending.state).ok_or_else(|| {
        invalid(
            &pending.field,
            "lifecycle state does not resolve to a property",
        )
    })?;
    if !state.required {
        return Err(invalid(
            &pending.field,
            "lifecycle state must be required by its schema",
        ));
    }
    let states = scalar::enum_values(catalog, state.schema)
        .filter(|states| !states.is_empty())
        .ok_or_else(|| {
            invalid(
                &pending.field,
                "lifecycle state must declare a finite non-empty enum",
            )
        })?;
    if states
        .iter()
        .any(|state| state.is_null() || matches!(state, Value::Array(_) | Value::Object(_)))
    {
        return Err(invalid(
            &pending.field,
            "lifecycle state enum values must be non-null scalars",
        ));
    }
    for edge in &pending.edges {
        if !states.contains(&edge.from) || !states.contains(&edge.to) {
            return Err(invalid(
                &pending.field,
                "every transition state must be declared by the state enum",
            ));
        }
    }
    reject_cycles(&states, &pending.edges, &pending.field)?;
    let plans = build_plans(&states, &pending.edges, &pending.field)?;

    let created = path::resolve_property(catalog, root, &pending.created_at).ok_or_else(|| {
        invalid(
            &pending.field,
            "lifecycle created-at does not resolve to a property",
        )
    })?;
    if !created.required {
        return Err(invalid(
            &pending.field,
            "lifecycle created-at must be required by its schema",
        ));
    }
    let kind = ordered::kind_for_schema(catalog, created.schema).ok_or_else(|| {
        invalid(
            &pending.field,
            "lifecycle timestamps must be numeric or use format `date-time`",
        )
    })?;
    let mut timestamp_paths = pending
        .edges
        .iter()
        .map(|edge| edge.at.clone())
        .collect::<Vec<_>>();
    timestamp_paths.sort();
    timestamp_paths.dedup();
    let mut timestamps = Vec::new();
    let mut schemas = vec![created.schema];
    for pointer in timestamp_paths {
        let resolved = path::resolve_property(catalog, root, &pointer).ok_or_else(|| {
            invalid(
                &pending.field,
                &format!("transition timestamp `{pointer}` does not resolve to a property"),
            )
        })?;
        if ordered::kind_for_schema(catalog, resolved.schema) != Some(kind) {
            return Err(invalid(
                &pending.field,
                "all lifecycle timestamps must use compatible value types",
            ));
        }
        let clear = if !resolved.required {
            Clear::Missing
        } else if scalar::accepts(catalog, resolved.schema, &Value::Null) {
            Clear::Null
        } else {
            return Err(invalid(
                &pending.field,
                &format!(
                    "transition timestamp `{pointer}` must be optional or nullable so inactive paths can be cleared"
                ),
            ));
        };
        schemas.push(resolved.schema);
        timestamps.push(Timestamp {
            path: pointer,
            clear,
        });
    }
    let anchor = scalar::common_witness(catalog, &schemas).ok_or_else(|| {
        invalid(
            &pending.field,
            "lifecycle timestamps do not share a constructible value",
        )
    })?;
    Ok(Constraint {
        state: pending.state,
        created_at: pending.created_at,
        edges: pending.edges,
        timestamps,
        plans,
        kind,
        anchor,
    })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraint: &Constraint,
) -> bool {
    let Some(state) = path::resolve_property(catalog, root, &constraint.state) else {
        return false;
    };
    let Some(created) = path::resolve_property(catalog, root, &constraint.created_at) else {
        return false;
    };
    state.required
        && created.required
        && ordered::kind_for_schema(catalog, created.schema) == Some(constraint.kind)
        && scalar::accepts(catalog, created.schema, &constraint.anchor)
        && !constraint.edges.is_empty()
        && !constraint.plans.is_empty()
        && compiled_plans_are_valid(constraint)
        && constraint.timestamps.iter().all(|timestamp| {
            path::resolve_property(catalog, root, &timestamp.path).is_some_and(|resolved| {
                ordered::kind_for_schema(catalog, resolved.schema) == Some(constraint.kind)
                    && scalar::accepts(catalog, resolved.schema, &constraint.anchor)
                    && (!resolved.required || timestamp.clear == Clear::Null)
                    && (timestamp.clear != Clear::Null
                        || scalar::accepts(catalog, resolved.schema, &Value::Null))
            })
        })
}

pub(super) fn prove(constraint: &Constraint, value: &Value) -> Vec<Violation> {
    let Some(state) = value.pointer(&constraint.state) else {
        return violation(constraint, "lifecycle state is missing");
    };
    let Some(plan) = constraint.plans.iter().find(|plan| plan.state == *state) else {
        return violation(
            constraint,
            "lifecycle state is not declared by the transition graph",
        );
    };
    if plan
        .paths
        .iter()
        .any(|candidate| path_is_sound(constraint, candidate, value))
    {
        Vec::new()
    } else {
        violation(
            constraint,
            "transition timestamps do not describe a valid path to the current state",
        )
    }
}

/// Materializes the deterministic path for the current state and clears inactive timestamps.
pub(super) fn establish(constraint: &Constraint, value: &mut Value) -> bool {
    let Some(state) = value.pointer(&constraint.state) else {
        return false;
    };
    let Some(plan) = constraint.plans.iter().find(|plan| plan.state == *state) else {
        return false;
    };
    let Some(selected) = plan.paths.first() else {
        return false;
    };
    let used = selected
        .iter()
        .filter_map(|index| constraint.edges.get(*index))
        .map(|edge| edge.at.as_str())
        .collect::<BTreeSet<_>>();
    let before = value.clone();
    path::apply(value, &constraint.created_at, constraint.anchor.clone());
    for timestamp in &constraint.timestamps {
        if used.contains(timestamp.path.as_str()) {
            path::apply(value, &timestamp.path, constraint.anchor.clone());
        } else {
            match timestamp.clear {
                Clear::Missing => {
                    path::remove(value, &timestamp.path);
                }
                Clear::Null => {
                    path::apply(value, &timestamp.path, Value::Null);
                }
            }
        }
    }
    before != *value && prove(constraint, value).is_empty()
}

fn path_is_sound(constraint: &Constraint, path: &[usize], value: &Value) -> bool {
    let used = path
        .iter()
        .filter_map(|index| constraint.edges.get(*index))
        .map(|edge| edge.at.as_str())
        .collect::<BTreeSet<_>>();
    if constraint.timestamps.iter().any(|timestamp| {
        let selected = value.pointer(&timestamp.path);
        if used.contains(timestamp.path.as_str()) {
            selected.is_none_or(Value::is_null)
        } else {
            selected.is_some_and(|value| !value.is_null())
        }
    }) {
        return false;
    }
    let Some(mut previous) = value.pointer(&constraint.created_at) else {
        return false;
    };
    if previous.is_null() {
        return false;
    }
    for index in path {
        let Some(current) = constraint
            .edges
            .get(*index)
            .and_then(|edge| value.pointer(&edge.at))
        else {
            return false;
        };
        if current.is_null()
            || !matches!(
                constraint.kind.compare(previous, current),
                Some(Ordering::Less | Ordering::Equal)
            )
        {
            return false;
        }
        previous = current;
    }
    true
}

fn build_plans(states: &[Value], edges: &[Edge], field: &str) -> Result<Vec<StatePlan>> {
    let destinations = edges.iter().map(|edge| &edge.to).collect::<Vec<_>>();
    let roots = states
        .iter()
        .filter(|state| !destinations.contains(state))
        .cloned()
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return Err(invalid(
            field,
            "lifecycle transition graph has no initial state",
        ));
    }
    let mut plans = states
        .iter()
        .cloned()
        .map(|state| StatePlan {
            state,
            paths: Vec::new(),
        })
        .collect::<Vec<_>>();
    for root in roots {
        let mut visited = vec![root.clone()];
        collect_paths(&root, &[], &mut visited, edges, &mut plans, field)?;
    }
    if plans.iter().any(|plan| plan.paths.is_empty()) {
        return Err(invalid(
            field,
            "every lifecycle state must be reachable from an initial state",
        ));
    }
    for plan in &mut plans {
        plan.paths.sort_by_key(Vec::len);
    }
    Ok(plans)
}

fn collect_paths(
    current: &Value,
    path: &[usize],
    visited: &mut Vec<Value>,
    edges: &[Edge],
    plans: &mut [StatePlan],
    field: &str,
) -> Result<()> {
    let plan = plans
        .iter_mut()
        .find(|plan| plan.state == *current)
        .ok_or_else(|| invalid(field, "transition graph references an unknown state"))?;
    if plan.paths.len() >= MAX_PATHS {
        return Err(invalid(
            field,
            "lifecycle transition graph has too many paths to compile safely",
        ));
    }
    plan.paths.push(path.to_vec());
    for (index, edge) in edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| edge.from == *current)
    {
        if visited.contains(&edge.to) {
            continue;
        }
        let mut next_path = path.to_vec();
        next_path.push(index);
        visited.push(edge.to.clone());
        collect_paths(&edge.to, &next_path, visited, edges, plans, field)?;
        visited.pop();
    }
    Ok(())
}

fn reject_cycles(states: &[Value], edges: &[Edge], field: &str) -> Result<()> {
    fn visit(
        state: &Value,
        edges: &[Edge],
        visiting: &mut Vec<Value>,
        visited: &mut Vec<Value>,
    ) -> bool {
        if visiting.contains(state) {
            return false;
        }
        if visited.contains(state) {
            return true;
        }
        visiting.push(state.clone());
        for edge in edges.iter().filter(|edge| edge.from == *state) {
            if !visit(&edge.to, edges, visiting, visited) {
                return false;
            }
        }
        visiting.pop();
        visited.push(state.clone());
        true
    }

    let mut visiting = Vec::new();
    let mut visited = Vec::new();
    if states
        .iter()
        .all(|state| visit(state, edges, &mut visiting, &mut visited))
    {
        Ok(())
    } else {
        Err(invalid(
            field,
            "lifecycle transition graph must not contain cycles",
        ))
    }
}

fn compiled_plans_are_valid(constraint: &Constraint) -> bool {
    let mut states = Vec::new();
    for plan in &constraint.plans {
        if states.contains(&plan.state) || plan.paths.is_empty() {
            return false;
        }
        states.push(plan.state.clone());
        for path in &plan.paths {
            let Some(first) = path.first().and_then(|index| constraint.edges.get(*index)) else {
                if !path.is_empty() || constraint.edges.iter().any(|edge| edge.to == plan.state) {
                    return false;
                }
                continue;
            };
            if constraint.edges.iter().any(|edge| edge.to == first.from) {
                return false;
            }
            let mut current = &first.from;
            for index in path {
                let Some(edge) = constraint.edges.get(*index) else {
                    return false;
                };
                if edge.from != *current
                    || !constraint
                        .timestamps
                        .iter()
                        .any(|timestamp| timestamp.path == edge.at)
                {
                    return false;
                }
                current = &edge.to;
            }
            if *current != plan.state {
                return false;
            }
        }
    }
    constraint
        .timestamps
        .iter()
        .enumerate()
        .all(|(index, timestamp)| {
            constraint.timestamps[..index]
                .iter()
                .all(|candidate| candidate.path != timestamp.path)
        })
        && constraint.edges.iter().all(|edge| {
            states.contains(&edge.from)
                && states.contains(&edge.to)
                && constraint
                    .timestamps
                    .iter()
                    .any(|timestamp| timestamp.path == edge.at)
        })
}

fn violation(constraint: &Constraint, message: &str) -> Vec<Violation> {
    vec![Violation {
        kind: ViolationKind::Lifecycle,
        path: constraint.state.clone(),
        message: message.to_owned(),
    }]
}

fn pointer(value: Option<&Value>, field: &str) -> Result<String> {
    let pointer = value
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(field, "value must be a JSON Pointer"))?;
    path::segments(pointer).map_err(|reason| invalid(field, &reason))?;
    Ok(pointer.to_owned())
}

fn states(value: &Value, field: &str) -> Result<Vec<Value>> {
    if let Some(values) = value.as_array() {
        if values.is_empty() {
            return Err(invalid(field, "state list must not be empty"));
        }
        values
            .iter()
            .map(|value| state_value(value, field))
            .collect()
    } else {
        state_value(value, field).map(|value| vec![value])
    }
}

fn state_value(value: &Value, field: &str) -> Result<Value> {
    if value.is_null() || matches!(value, Value::Array(_) | Value::Object(_)) {
        Err(invalid(field, "state must be a non-null scalar"))
    } else {
        Ok(value.clone())
    }
}
