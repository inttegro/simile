use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Comparable {
    Number,
    DateTime,
}

pub(crate) fn comparable(schema: &Value) -> Option<Comparable> {
    if is_number(schema) {
        Some(Comparable::Number)
    } else if has_type(schema, "string")
        && schema.get("format").and_then(Value::as_str) == Some("date-time")
    {
        Some(Comparable::DateTime)
    } else {
        None
    }
}

pub(crate) fn is_number(schema: &Value) -> bool {
    has_type(schema, "number") || has_type(schema, "integer")
}

pub(crate) fn is_integer(schema: &Value) -> bool {
    has_type(schema, "integer")
}

pub(crate) fn is_array(schema: &Value) -> bool {
    has_type(schema, "array")
}

fn has_type(schema: &Value, expected: &str) -> bool {
    match schema.get("type") {
        Some(Value::String(value)) => value == expected,
        Some(Value::Array(values)) => values.iter().any(|value| value.as_str() == Some(expected)),
        _ => false,
    }
}

pub(crate) fn pointer(field: &str) -> String {
    format!("/{}", field.replace('~', "~0").replace('/', "~1"))
}
