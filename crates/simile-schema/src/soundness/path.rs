use std::collections::BTreeSet;

use crate::{SchemaCatalog, SchemaId};

pub(super) fn resolve_property(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pointer: &str,
) -> Option<SchemaId> {
    let mut current = root;
    for segment in segments(pointer).ok()? {
        current = property(catalog, current, &segment, &mut BTreeSet::new())?;
    }
    Some(current)
}

pub(super) fn segments(pointer: &str) -> std::result::Result<Vec<String>, String> {
    let Some(pointer_body) = pointer.strip_prefix('/') else {
        return Err("path must be a non-empty absolute JSON Pointer".to_owned());
    };
    if pointer_body.is_empty() {
        return Err("path must identify a property below the constrained object".to_owned());
    }
    pointer_body
        .split('/')
        .map(|segment| decode_segment(pointer, segment))
        .collect()
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
