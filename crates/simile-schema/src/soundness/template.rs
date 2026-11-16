//! Evaluates a string field rendered from literal text and scalar field placeholders.
//!
//! For example:
//!
//! ```yaml
//! x-simile-constraints:
//!   - template:
//!       path: /url
//!       value: "https://api.example.test/invoices/{/id}"
//! ```
//!
//! Every `{/json/pointer}` source must be a required, non-null scalar. Rendering inserts the JSON
//! scalar's textual value without URL encoding or other transformation. Proof requires the target
//! string to equal the rendering exactly; establishment renders from the current payload and
//! replaces the target.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Violation, ViolationKind, invalid, path, scalar};
use crate::{Result, SchemaCatalog, SchemaId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Constraint {
    #[serde(rename = "p")]
    path: String,
    #[serde(rename = "s")]
    segments: Vec<Segment>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Segment {
    #[serde(rename = "t")]
    Text(String),
    #[serde(rename = "p")]
    Path(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Pending {
    field: String,
    path: String,
    segments: Vec<Segment>,
}

pub(super) fn parse(definition: &Value, field: &str) -> Result<Pending> {
    let field = format!("{field}.template");
    let template = definition
        .as_object()
        .ok_or_else(|| invalid(&field, "template must be an object"))?;
    if template.len() != 2 || !template.contains_key("path") || !template.contains_key("value") {
        return Err(invalid(
            &field,
            "template requires exactly `path` and `value`",
        ));
    }
    let target = template
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&format!("{field}.path"), "path must be a JSON Pointer"))?;
    path::segments(target).map_err(|reason| invalid(&format!("{field}.path"), &reason))?;
    let source = template
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&format!("{field}.value"), "value must be a string"))?;
    let segments = parse_segments(source, &format!("{field}.value"))?;
    if !segments
        .iter()
        .any(|segment| matches!(segment, Segment::Path(_)))
    {
        return Err(invalid(
            &format!("{field}.value"),
            "template must contain at least one `{/<path>}` placeholder",
        ));
    }
    Ok(Pending {
        field,
        path: target.to_owned(),
        segments,
    })
}

pub(super) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pending: Pending,
) -> Result<Constraint> {
    check(
        catalog,
        root,
        &pending.path,
        &pending.segments,
        &pending.field,
    )?;
    Ok(Constraint {
        path: pending.path,
        segments: pending.segments,
    })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraint: &Constraint,
) -> bool {
    check(
        catalog,
        root,
        &constraint.path,
        &constraint.segments,
        "template",
    )
    .is_ok()
}

pub(super) fn prove(constraint: &Constraint, value: &Value) -> Vec<Violation> {
    let expected = render(&constraint.segments, value);
    let actual = value.pointer(&constraint.path).and_then(Value::as_str);
    if expected
        .as_deref()
        .is_some_and(|expected| Some(expected) == actual)
    {
        Vec::new()
    } else {
        vec![Violation {
            kind: ViolationKind::Template,
            path: constraint.path.clone(),
            message: "value must match the declared field template".to_owned(),
        }]
    }
}

/// Renders the template into its target and returns whether the payload changed.
pub(super) fn establish(constraint: &Constraint, value: &mut Value) -> bool {
    let Some(expected) = render(&constraint.segments, value) else {
        return false;
    };
    let expected = Value::String(expected);
    if value.pointer(&constraint.path) == Some(&expected) {
        return false;
    }
    path::apply(value, &constraint.path, expected) && prove(constraint, value).is_empty()
}

fn check(
    catalog: &SchemaCatalog,
    root: SchemaId,
    target: &str,
    segments: &[Segment],
    field: &str,
) -> Result<()> {
    if !segments
        .iter()
        .any(|segment| matches!(segment, Segment::Path(_)))
    {
        return Err(invalid(field, "template must contain a placeholder"));
    }
    let target = path::resolve_property(catalog, root, target)
        .ok_or_else(|| invalid(field, "template target does not resolve to a property"))?;
    if scalar::kind(catalog, target.schema) != Some(scalar::Kind::String) {
        return Err(invalid(field, "template target must be a string"));
    }
    for source in segments.iter().filter_map(|segment| match segment {
        Segment::Path(source) => Some(source),
        Segment::Text(_) => None,
    }) {
        let resolved = path::resolve_property(catalog, root, source).ok_or_else(|| {
            invalid(
                field,
                &format!("template source `{source}` does not resolve to a property"),
            )
        })?;
        if !resolved.required {
            return Err(invalid(
                field,
                &format!("template source `{source}` must be required by its schema"),
            ));
        }
        if scalar::kind(catalog, resolved.schema).is_none() {
            return Err(invalid(
                field,
                &format!("template source `{source}` must be scalar"),
            ));
        }
        if scalar::accepts(catalog, resolved.schema, &Value::Null) {
            return Err(invalid(
                field,
                &format!("template source `{source}` must not be nullable"),
            ));
        }
    }
    Ok(())
}

fn render(segments: &[Segment], value: &Value) -> Option<String> {
    let mut rendered = String::new();
    for segment in segments {
        match segment {
            Segment::Text(text) => rendered.push_str(text),
            Segment::Path(pointer) => match value.pointer(pointer)? {
                Value::String(text) => rendered.push_str(text),
                Value::Number(number) => rendered.push_str(&number.to_string()),
                Value::Bool(value) => rendered.push_str(if *value { "true" } else { "false" }),
                Value::Null | Value::Array(_) | Value::Object(_) => return None,
            },
        }
    }
    Some(rendered)
}

fn parse_segments(template: &str, field: &str) -> Result<Vec<Segment>> {
    let mut remaining = template;
    let mut segments = Vec::new();
    while let Some(open) = remaining.find('{') {
        let (text, tail) = remaining.split_at(open);
        if !text.is_empty() {
            if text.contains('}') {
                return Err(invalid(field, "template contains an unmatched `}`"));
            }
            segments.push(Segment::Text(text.to_owned()));
        }
        let tail = &tail[1..];
        let close = tail
            .find('}')
            .ok_or_else(|| invalid(field, "template contains an unmatched `{`"))?;
        let (pointer, rest) = tail.split_at(close);
        path::segments(pointer)
            .map_err(|reason| invalid(field, &format!("invalid placeholder: {reason}")))?;
        segments.push(Segment::Path(pointer.to_owned()));
        remaining = &rest[1..];
    }
    if remaining.contains('}') {
        return Err(invalid(field, "template contains an unmatched `}`"));
    }
    if !remaining.is_empty() {
        segments.push(Segment::Text(remaining.to_owned()));
    }
    Ok(segments)
}
