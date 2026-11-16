use std::collections::BTreeSet;

use serde_json::{Number, Value};

use crate::{JsonType, SchemaCatalog, SchemaId, SchemaRef, validate};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Kind {
    Boolean,
    Number,
    String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Numeric {
    Integer(i128),
    Float(f64),
}

pub(super) fn kind(catalog: &SchemaCatalog, id: SchemaId) -> Option<Kind> {
    kind_inner(catalog, id, &mut BTreeSet::new())
}

pub(super) fn accepts(catalog: &SchemaCatalog, id: SchemaId, value: &Value) -> bool {
    validate::value(catalog, SchemaRef(id), value).is_empty()
}

pub(super) fn common_witness(catalog: &SchemaCatalog, schemas: &[SchemaId]) -> Option<Value> {
    schemas.iter().find_map(|schema| {
        catalog
            .synthesize_representative_matching(SchemaRef(*schema), |value| {
                schemas
                    .iter()
                    .all(|candidate| accepts(catalog, *candidate, value))
            })
            .ok()
    })
}

pub(super) fn enum_values(catalog: &SchemaCatalog, id: SchemaId) -> Option<Vec<Value>> {
    enum_values_inner(catalog, id, &mut BTreeSet::new())
}

impl Numeric {
    pub(super) fn from_value(value: &Value) -> Option<Self> {
        let number = value.as_number()?;
        if let Some(value) = number.as_i64() {
            Some(Self::Integer(i128::from(value)))
        } else if let Some(value) = number.as_u64() {
            Some(Self::Integer(i128::from(value)))
        } else {
            number.as_f64().map(Self::Float)
        }
    }

    pub(super) fn add(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Integer(left), Self::Integer(right)) => {
                left.checked_add(right).map(Self::Integer)
            }
            (left, right) => finite(left.as_f64()? + right.as_f64()?),
        }
    }

    pub(super) fn subtract(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Integer(left), Self::Integer(right)) => {
                left.checked_sub(right).map(Self::Integer)
            }
            (left, right) => finite(left.as_f64()? - right.as_f64()?),
        }
    }

    pub(super) fn into_value(self) -> Option<Value> {
        match self {
            Self::Integer(value) => i64::try_from(value)
                .ok()
                .map(Number::from)
                .or_else(|| u64::try_from(value).ok().map(Number::from))
                .map(Value::Number),
            Self::Float(value) => Number::from_f64(value).map(Value::Number),
        }
    }

    fn as_f64(self) -> Option<f64> {
        match self {
            Self::Integer(value) => value.to_string().parse().ok(),
            Self::Float(value) => Some(value),
        }
    }
}

fn finite(value: f64) -> Option<Numeric> {
    value.is_finite().then_some(Numeric::Float(value))
}

fn kind_inner(
    catalog: &SchemaCatalog,
    id: SchemaId,
    visited: &mut BTreeSet<SchemaId>,
) -> Option<Kind> {
    if !visited.insert(id) {
        return None;
    }
    let node = catalog.nodes.get(id as usize)?;
    let non_null = node
        .types
        .iter()
        .filter(|kind| **kind != JsonType::Null)
        .copied()
        .collect::<BTreeSet<_>>();
    let direct = if !non_null.is_empty()
        && non_null
            .iter()
            .all(|kind| matches!(kind, JsonType::Integer | JsonType::Number))
    {
        Some(Kind::Number)
    } else if non_null == BTreeSet::from([JsonType::String]) {
        Some(Kind::String)
    } else if non_null == BTreeSet::from([JsonType::Boolean]) {
        Some(Kind::Boolean)
    } else {
        enum_kind(node.enum_values.as_deref())
    };
    direct.or_else(|| {
        node.all_of.iter().find_map(|child| {
            let mut branch = visited.clone();
            kind_inner(catalog, *child, &mut branch)
        })
    })
}

fn enum_values_inner(
    catalog: &SchemaCatalog,
    id: SchemaId,
    visited: &mut BTreeSet<SchemaId>,
) -> Option<Vec<Value>> {
    if !visited.insert(id) {
        return None;
    }
    let node = catalog.nodes.get(id as usize)?;
    node.enum_values.clone().or_else(|| {
        node.all_of.iter().find_map(|child| {
            let mut branch = visited.clone();
            enum_values_inner(catalog, *child, &mut branch)
        })
    })
}

fn enum_kind(values: Option<&[Value]>) -> Option<Kind> {
    let kinds = values?
        .iter()
        .filter(|value| !value.is_null())
        .map(|value| match value {
            Value::Bool(_) => Some(Kind::Boolean),
            Value::Number(_) => Some(Kind::Number),
            Value::String(_) => Some(Kind::String),
            _ => None,
        })
        .collect::<Option<BTreeSet<_>>>()?;
    (kinds.len() == 1)
        .then(|| kinds.iter().next().copied())
        .flatten()
}
