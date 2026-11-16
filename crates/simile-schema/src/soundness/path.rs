use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::{SchemaCatalog, SchemaId};

pub(super) struct ResolvedProperty {
    pub schema: SchemaId,
    pub required: bool,
}

pub(super) struct ResolvedSelector {
    pub schema: SchemaId,
    pub required: bool,
    pub multiple: bool,
}

pub(super) struct SelectedValue<'a> {
    pub pointer: String,
    pub value: &'a Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SelectorSegment {
    Property(String),
    Each,
}

pub(super) fn resolve_property(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pointer: &str,
) -> Option<ResolvedProperty> {
    let resolved = resolve_selector(catalog, root, pointer)?;
    if resolved.multiple {
        return None;
    }
    Some(ResolvedProperty {
        schema: resolved.schema,
        required: resolved.required,
    })
}

pub(super) fn resolve_selector(
    catalog: &SchemaCatalog,
    root: SchemaId,
    selector: &str,
) -> Option<ResolvedSelector> {
    let mut current = root;
    let mut required = true;
    let mut multiple = false;
    for segment in selector_segments(selector).ok()? {
        match segment {
            SelectorSegment::Property(name) => {
                required &= property_is_required(catalog, current, &name, &mut BTreeSet::new());
                current = property(catalog, current, &name, &mut BTreeSet::new())?;
            }
            SelectorSegment::Each => {
                current = items(catalog, current, &mut BTreeSet::new())?;
                multiple = true;
            }
        }
    }
    Some(ResolvedSelector {
        schema: current,
        required,
        multiple,
    })
}

pub(super) fn resolve_selector_syntax(selector: &str) -> std::result::Result<(), String> {
    selector_segments(selector).map(|_| ())
}

pub(super) fn select<'a>(value: &'a Value, selector: &str) -> Option<Vec<SelectedValue<'a>>> {
    let segments = selector_segments(selector).ok()?;
    let mut selected = Vec::new();
    select_inner(value, &segments, "", &mut selected);
    Some(selected)
}

pub(super) fn apply(value: &mut Value, pointer: &str, replacement: Value) -> bool {
    if let Some(target) = value.pointer_mut(pointer) {
        *target = replacement;
        return true;
    }
    let Ok(mut segments) = segments(pointer) else {
        return false;
    };
    let Some(property) = segments.pop() else {
        return false;
    };
    let mut current = value;
    for segment in segments {
        let Some(object) = current.as_object_mut() else {
            return false;
        };
        current = object
            .entry(segment)
            .or_insert_with(|| Value::Object(Map::new()));
    }
    let Some(object) = current.as_object_mut() else {
        return false;
    };
    object.insert(property, replacement);
    true
}

pub(super) fn remove(value: &mut Value, pointer: &str) -> bool {
    let Ok(mut segments) = segments(pointer) else {
        return false;
    };
    let Some(property) = segments.pop() else {
        return false;
    };
    let mut current = value;
    for segment in segments {
        let Some(object) = current.as_object_mut() else {
            return false;
        };
        let Some(next) = object.get_mut(&segment) else {
            return true;
        };
        current = next;
    }
    current
        .as_object_mut()
        .is_some_and(|object| object.remove(&property).is_some())
}

pub(super) fn segments(pointer: &str) -> std::result::Result<Vec<String>, String> {
    selector_segments(pointer).and_then(|segments| {
        segments
            .into_iter()
            .map(|segment| match segment {
                SelectorSegment::Property(name) => Ok(name),
                SelectorSegment::Each => {
                    Err(format!("path `{pointer}` cannot contain an array wildcard"))
                }
            })
            .collect()
    })
}

fn selector_segments(pointer: &str) -> std::result::Result<Vec<SelectorSegment>, String> {
    let Some(pointer_body) = pointer.strip_prefix('/') else {
        return Err("path must be a non-empty absolute JSON Pointer".to_owned());
    };
    if pointer_body.is_empty() {
        return Err("path must identify a property below the constrained object".to_owned());
    }
    pointer_body
        .split('/')
        .map(|segment| {
            if segment == "*" {
                Ok(SelectorSegment::Each)
            } else {
                decode_segment(pointer, segment).map(SelectorSegment::Property)
            }
        })
        .collect()
}

fn select_inner<'a>(
    value: &'a Value,
    segments: &[SelectorSegment],
    pointer: &str,
    selected: &mut Vec<SelectedValue<'a>>,
) {
    let Some((segment, tail)) = segments.split_first() else {
        selected.push(SelectedValue {
            pointer: pointer.to_owned(),
            value,
        });
        return;
    };
    match segment {
        SelectorSegment::Property(name) => {
            if let Some(next) = value.as_object().and_then(|object| object.get(name)) {
                let next_pointer = format!("{pointer}/{}", encode_segment(name));
                select_inner(next, tail, &next_pointer, selected);
            }
        }
        SelectorSegment::Each => {
            if let Some(values) = value.as_array() {
                for (index, next) in values.iter().enumerate() {
                    let next_pointer = format!("{pointer}/{index}");
                    select_inner(next, tail, &next_pointer, selected);
                }
            }
        }
    }
}

fn property(
    catalog: &SchemaCatalog,
    id: SchemaId,
    name: &str,
    visited: &mut BTreeSet<SchemaId>,
) -> Option<SchemaId> {
    if !visited.insert(id) {
        return None;
    }
    let node = catalog.nodes.get(id as usize)?;
    if let Some(property) = node.properties.get(name) {
        return Some(*property);
    }
    node.all_of.iter().find_map(|child| {
        let mut branch = visited.clone();
        property(catalog, *child, name, &mut branch)
    })
}

fn items(
    catalog: &SchemaCatalog,
    id: SchemaId,
    visited: &mut BTreeSet<SchemaId>,
) -> Option<SchemaId> {
    if !visited.insert(id) {
        return None;
    }
    let node = catalog.nodes.get(id as usize)?;
    node.items.or_else(|| {
        node.all_of.iter().find_map(|child| {
            let mut branch = visited.clone();
            items(catalog, *child, &mut branch)
        })
    })
}

fn property_is_required(
    catalog: &SchemaCatalog,
    id: SchemaId,
    name: &str,
    visited: &mut BTreeSet<SchemaId>,
) -> bool {
    if !visited.insert(id) {
        return false;
    }
    let Some(node) = catalog.nodes.get(id as usize) else {
        return false;
    };
    node.required.contains(name)
        || node.all_of.iter().any(|child| {
            let mut branch = visited.clone();
            property_is_required(catalog, *child, name, &mut branch)
        })
}

fn decode_segment(pointer: &str, segment: &str) -> std::result::Result<String, String> {
    let mut decoded = String::new();
    let mut characters = segment.chars();
    while let Some(character) = characters.next() {
        if character != '~' {
            decoded.push(character);
            continue;
        }
        match characters.next() {
            Some('0') => decoded.push('~'),
            Some('1') => decoded.push('/'),
            _ => {
                return Err(format!(
                    "path `{pointer}` contains an invalid JSON Pointer escape"
                ));
            }
        }
    }
    Ok(decoded)
}

fn encode_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}
