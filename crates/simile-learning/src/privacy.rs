use std::collections::BTreeMap;

use serde_json::Value;

use crate::ObservedBody;

const REDACTED: &str = "<redacted>";

pub(crate) fn sanitize_headers(
    headers: impl IntoIterator<Item = (String, Vec<String>)>,
) -> BTreeMap<String, Vec<String>> {
    headers
        .into_iter()
        .map(|(name, values)| {
            let normalized = name.to_ascii_lowercase();
            if sensitive_name(&normalized) {
                (normalized, vec![REDACTED.to_owned()])
            } else {
                (normalized, values)
            }
        })
        .collect()
}

pub(crate) fn sanitize_body(content_type: Option<&str>, bytes: &[u8]) -> ObservedBody {
    if bytes.is_empty() {
        return ObservedBody::Empty;
    }
    let media_type = content_type
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .unwrap_or_default();
    if media_type == "application/json" || media_type.ends_with("+json") {
        if let Ok(mut value) = serde_json::from_slice(bytes) {
            redact_json(&mut value, None);
            return ObservedBody::Json { value };
        }
    }
    if media_type.starts_with("text/") {
        return ObservedBody::Text;
    }
    ObservedBody::Opaque
}

pub(crate) fn sanitize_query(query: &str) -> String {
    let pairs = form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
    let mut sanitized = form_urlencoded::Serializer::new(String::new());
    for (name, value) in pairs {
        let normalized = name.to_ascii_lowercase();
        sanitized.append_pair(
            &name,
            if sensitive_name(&normalized) {
                REDACTED
            } else {
                &value
            },
        );
    }
    sanitized.finish()
}

fn redact_json(value: &mut Value, name: Option<&str>) {
    if name.is_some_and(sensitive_name) {
        *value = Value::String(REDACTED.to_owned());
        return;
    }
    match value {
        Value::Array(values) => {
            for value in values {
                redact_json(value, None);
            }
        }
        Value::Object(values) => {
            for (name, value) in values {
                redact_json(value, Some(&name.to_ascii_lowercase()));
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn sensitive_name(name: &str) -> bool {
    matches!(
        name,
        "authorization"
            | "cookie"
            | "set-cookie"
            | "proxy-authorization"
            | "x-api-key"
            | "api-key"
            | "api_key"
            | "access-token"
            | "access_token"
            | "refresh-token"
            | "refresh_token"
            | "password"
            | "secret"
            | "client-secret"
            | "client_secret"
            | "card-number"
            | "card_number"
            | "cvv"
            | "cvc"
    ) || name.ends_with("_secret")
        || name.ends_with("_token")
        || name.ends_with("-secret")
        || name.ends_with("-token")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn redacts_credentials_before_evidence_is_created() {
        let body = serde_json::to_vec(&json!({
            "name": "Ada",
            "api_key": "sk_live_secret",
            "nested": { "access_token": "token", "visible": 42 }
        }))
        .unwrap_or_else(|error| panic!("{error}"));
        let captured = sanitize_body(Some("application/json"), &body);
        assert_eq!(
            captured,
            ObservedBody::Json {
                value: json!({
                    "name": "Ada",
                    "api_key": "<redacted>",
                    "nested": { "access_token": "<redacted>", "visible": 42 }
                })
            }
        );
    }

    #[test]
    fn redacts_sensitive_query_values() {
        assert_eq!(
            sanitize_query("expand=customer&api_key=sk_live_secret"),
            "expand=customer&api_key=%3Credacted%3E"
        );
    }

    #[test]
    fn records_text_shape_without_persisting_text_content() {
        assert_eq!(
            sanitize_body(Some("text/plain"), b"secret free-form text"),
            ObservedBody::Text
        );
    }
}
