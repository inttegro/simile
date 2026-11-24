//! Projects one logical operation instant onto schema-declared response timestamps.
//!
//! Response generation owns the operation semantics; this module owns schema traversal. It only
//! writes properties whose compiled schema declares a temporal format, and it leaves explicit
//! request-to-response targets untouched.

use std::collections::BTreeSet;

use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{JsonType, SchemaCatalog, SchemaId, SchemaRef, SynthesisError, validate};

const MAX_DEPTH: usize = 64;

pub(crate) fn apply(
    catalog: &SchemaCatalog,
    root: SchemaRef,
    value: &mut Value,
    logical_time_milliseconds: i64,
    materialize: &BTreeSet<String>,
    refresh: &BTreeSet<String>,
    protected: &BTreeSet<String>,
) -> Result<BTreeSet<String>, SynthesisError> {
    let instant = OffsetDateTime::from_unix_timestamp_nanos(
        i128::from(logical_time_milliseconds).saturating_mul(1_000_000),
    )
    .map_err(|error| SynthesisError::Unreachable {
        reason: format!("logical response time is outside the RFC 3339 range: {error}"),
    })?;
    let values = TimestampValues {
        rfc3339: instant
            .format(&Rfc3339)
            .map_err(|error| SynthesisError::Unreachable {
                reason: format!("logical response time could not be formatted: {error}"),
            })?,
        date: instant.date().to_string(),
        unix_seconds: logical_time_milliseconds.div_euclid(1_000),
        unix_milliseconds: logical_time_milliseconds,
    };
    let mut changed = BTreeSet::new();
    visit(
        catalog,
        root.0,
        value,
        "",
        &values,
        materialize,
        refresh,
        protected,
        &mut changed,
        0,
    );
    Ok(changed)
}

struct TimestampValues {
    rfc3339: String,
    date: String,
    unix_seconds: i64,
    unix_milliseconds: i64,
}

#[allow(clippy::too_many_arguments)]
fn visit(
    catalog: &SchemaCatalog,
    id: SchemaId,
    value: &mut Value,
    pointer: &str,
    timestamps: &TimestampValues,
    materialize: &BTreeSet<String>,
    refresh: &BTreeSet<String>,
    protected: &BTreeSet<String>,
    changed: &mut BTreeSet<String>,
    depth: usize,
) {
    if depth >= MAX_DEPTH {
        return;
    }
    let Some(node) = catalog.nodes.get(id as usize) else {
        return;
    };

    if let Some(object) = value.as_object_mut() {
        for (name, child_id) in &node.properties {
            let child_pointer = format!("{pointer}/{}", escape_pointer(name));
            let should_materialize = materialize.contains(name);
            let should_refresh = refresh.contains(name) && object.contains_key(name);
            if (should_materialize || should_refresh) && !protected.contains(&child_pointer) {
                let timestamp =
                    timestamp_value(catalog, *child_id, timestamps, 0, &mut BTreeSet::new());
                if let Some(timestamp) = timestamp {
                    object.insert(name.clone(), timestamp);
                    changed.insert(child_pointer.clone());
                }
            }
            if let Some(child) = object.get_mut(name) {
                visit(
                    catalog,
                    *child_id,
                    child,
                    &child_pointer,
                    timestamps,
                    materialize,
                    refresh,
                    protected,
                    changed,
                    depth + 1,
                );
            }
        }
    }

    if let (Some(items), Some(values)) = (node.items, value.as_array_mut()) {
        for (index, child) in values.iter_mut().enumerate() {
            visit(
                catalog,
                items,
                child,
                &format!("{pointer}/{index}"),
                timestamps,
                materialize,
                refresh,
                protected,
                changed,
                depth + 1,
            );
        }
    }

    let original = value.clone();
    for child in &node.all_of {
        visit(
            catalog,
            *child,
            value,
            pointer,
            timestamps,
            materialize,
            refresh,
            protected,
            changed,
            depth + 1,
        );
    }
    for branches in [&node.one_of, &node.any_of] {
        if let Some(child) = branches
            .iter()
            .find(|child| validate::value(catalog, SchemaRef(**child), &original).is_empty())
        {
            visit(
                catalog,
                *child,
                value,
                pointer,
                timestamps,
                materialize,
                refresh,
                protected,
                changed,
                depth + 1,
            );
        }
    }
}

fn timestamp_value(
    catalog: &SchemaCatalog,
    id: SchemaId,
    timestamps: &TimestampValues,
    depth: usize,
    active: &mut BTreeSet<SchemaId>,
) -> Option<Value> {
    if depth >= MAX_DEPTH || !active.insert(id) {
        return None;
    }
    let result = catalog.nodes.get(id as usize).and_then(|node| {
        let accepts = |kind| node.types.is_empty() || node.types.contains(&kind);
        match node.format.as_deref() {
            Some("date-time") if accepts(JsonType::String) => {
                Some(Value::String(timestamps.rfc3339.clone()))
            }
            Some("date") if accepts(JsonType::String) => {
                Some(Value::String(timestamps.date.clone()))
            }
            Some("unix-time") if accepts(JsonType::Integer) => {
                Some(Value::from(timestamps.unix_seconds))
            }
            Some("unix-time-ms" | "unix-milliseconds") if accepts(JsonType::Integer) => {
                Some(Value::from(timestamps.unix_milliseconds))
            }
            _ => node
                .all_of
                .iter()
                .chain(&node.any_of)
                .chain(&node.one_of)
                .copied()
                .find_map(|child| timestamp_value(catalog, child, timestamps, depth + 1, active)),
        }
    });
    active.remove(&id);
    result
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
