use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use crate::{Direction, Exchange, ObservedBody};

pub(crate) struct InferredContract {
    pub document: Value,
    pub bindings: usize,
    pub unknowns: usize,
}

pub(crate) fn infer(
    service: &str,
    upstream: &str,
    session: &str,
    exchanges: &[Exchange],
) -> InferredContract {
    let api = exchanges
        .iter()
        .filter(|exchange| exchange.direction == Direction::Api)
        .collect::<Vec<_>>();
    let mut grouped = BTreeMap::<(String, String), Vec<&Exchange>>::new();
    for exchange in &api {
        grouped
            .entry((exchange.path.clone(), exchange.method.to_ascii_lowercase()))
            .or_default()
            .push(exchange);
    }

    let mut paths = Map::new();
    let mut binding_count = 0;
    let mut unknowns = 0;
    for ((path, method), observations) in grouped {
        let (operation, bindings, operation_unknowns) = operation(&method, &path, &observations);
        binding_count += bindings;
        unknowns += operation_unknowns;
        let path_item = paths
            .entry(path)
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(path_item) = path_item {
            path_item.insert(method, operation);
        }
    }

    let webhooks = webhook_contracts(exchanges);
    let mut document = json!({
        "openapi": "3.1.0",
        "info": {
            "title": service,
            "version": "0.0.0-learned",
            "description": "A draft contract inferred from observed public HTTP exchanges. Review the learning report before distribution."
        },
        "servers": [{ "url": upstream }],
        "paths": paths,
        "x-simile-learning": {
            "session": session,
            "evidence": "../evidence/observations.jsonl",
            "apiExchanges": api.len(),
            "status": "observed-draft"
        }
    });
    if !webhooks.is_empty() {
        if let Value::Object(document) = &mut document {
            document.insert("webhooks".to_owned(), Value::Object(webhooks));
        }
    }
    InferredContract {
        document,
        bindings: binding_count,
        unknowns,
    }
}

fn operation(method: &str, path: &str, exchanges: &[&Exchange]) -> (Value, usize, usize) {
    let mut operation = Map::new();
    operation.insert(
        "operationId".to_owned(),
        Value::String(operation_id(method, path)),
    );
    operation.insert(
        "summary".to_owned(),
        Value::String(format!("Observed {} {path}", method.to_ascii_uppercase())),
    );
    operation.insert(
        "x-simile-learning".to_owned(),
        json!({
            "observations": exchanges.len(),
            "confidence": if exchanges.len() > 1 { "supported" } else { "single-observation" }
        }),
    );

    let parameters = query_parameters(exchanges);
    if !parameters.is_empty() {
        operation.insert("parameters".to_owned(), Value::Array(parameters));
    }

    let request_content = message_content(
        exchanges
            .iter()
            .map(|exchange| (&exchange.request.content_type, &exchange.request.body)),
    );
    if !request_content.is_empty() {
        operation.insert(
            "requestBody".to_owned(),
            json!({
                "required": exchanges.iter().all(|exchange| !matches!(exchange.request.body, ObservedBody::Empty)),
                "content": request_content
            }),
        );
    }

    let mut by_status = BTreeMap::<u16, Vec<&Exchange>>::new();
    for exchange in exchanges {
        by_status.entry(exchange.status).or_default().push(exchange);
    }
    let mut responses = Map::new();
    for (status, observations) in by_status {
        let content = message_content(
            observations
                .iter()
                .map(|exchange| (&exchange.response.content_type, &exchange.response.body)),
        );
        let mut response = Map::new();
        response.insert(
            "description".to_owned(),
            Value::String(format!("Observed HTTP {status} response")),
        );
        if !content.is_empty() {
            response.insert("content".to_owned(), Value::Object(content));
        }
        responses.insert(status.to_string(), Value::Object(response));
    }
    operation.insert("responses".to_owned(), Value::Object(responses));

    let bindings = infer_bindings(exchanges);
    let binding_count = bindings.len();
    if !bindings.is_empty() {
        operation.insert(
            "x-simile-from-request".to_owned(),
            json!({ "bindings": bindings }),
        );
    }
    let opaque = exchanges
        .iter()
        .filter(|exchange| {
            matches!(exchange.request.body, ObservedBody::Opaque)
                || matches!(exchange.response.body, ObservedBody::Opaque)
        })
        .count();
    let insufficient = usize::from(exchanges.len() < 2);
    (
        Value::Object(operation),
        binding_count,
        opaque + insufficient,
    )
}

fn message_content<'a>(
    messages: impl Iterator<Item = (&'a Option<String>, &'a ObservedBody)>,
) -> Map<String, Value> {
    let mut grouped = BTreeMap::<String, Vec<&ObservedBody>>::new();
    for (content_type, body) in messages {
        if matches!(body, ObservedBody::Empty) {
            continue;
        }
        let content_type = content_type
            .as_deref()
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("application/octet-stream")
            .to_owned();
        grouped.entry(content_type).or_default().push(body);
    }
    grouped
        .into_iter()
        .map(|(content_type, bodies)| {
            let values = bodies
                .iter()
                .filter_map(|body| body.json())
                .collect::<Vec<_>>();
            let schema = if !values.is_empty() {
                schema(&values)
            } else if bodies.iter().all(|body| matches!(body, ObservedBody::Text)) {
                json!({ "type": "string" })
            } else {
                json!({ "type": "string", "format": "binary" })
            };
            (content_type, json!({ "schema": schema }))
        })
        .collect()
}

fn query_parameters(exchanges: &[&Exchange]) -> Vec<Value> {
    let mut values = BTreeMap::<String, Vec<String>>::new();
    let mut presence = BTreeMap::<String, usize>::new();
    for exchange in exchanges {
        let mut seen = BTreeSet::new();
        if let Some(query) = &exchange.query {
            for (name, value) in form_urlencoded::parse(query.as_bytes()) {
                values
                    .entry(name.clone().into_owned())
                    .or_default()
                    .push(value.into_owned());
                seen.insert(name.into_owned());
            }
        }
        for name in seen {
            *presence.entry(name).or_default() += 1;
        }
    }
    values
        .into_iter()
        .map(|(name, examples)| {
            json!({
                "name": name,
                "in": "query",
                "required": presence.get(&name).copied().unwrap_or_default() == exchanges.len(),
                "schema": { "type": "string" },
                "example": examples.first()
            })
        })
        .collect()
}

fn infer_bindings(exchanges: &[&Exchange]) -> Vec<Value> {
    if exchanges.len() < 2 {
        return Vec::new();
    }
    let mut candidates: Option<BTreeSet<(String, String)>> = None;
    for exchange in exchanges {
        let (Some(request), Some(response)) =
            (exchange.request.body.json(), exchange.response.body.json())
        else {
            return Vec::new();
        };
        let request = scalar_paths(request);
        let response = scalar_paths(response);
        let matches = request
            .iter()
            .flat_map(|(from, request_value)| {
                response.iter().filter_map(move |(to, response_value)| {
                    binding_value(request_value)
                        .filter(|value| *value == *response_value)
                        .map(|_| (from.clone(), to.clone()))
                })
            })
            .collect::<BTreeSet<_>>();
        candidates = Some(match candidates {
            Some(candidates) => candidates.intersection(&matches).cloned().collect(),
            None => matches,
        });
    }
    candidates
        .unwrap_or_default()
        .into_iter()
        .map(|(from, to)| json!({ "from": from, "to": to }))
        .collect()
}

fn binding_value(value: &Value) -> Option<&Value> {
    match value {
        Value::String(text) if text != "<redacted>" && text.len() >= 4 => Some(value),
        Value::Number(_) => Some(value),
        Value::Null | Value::Bool(_) | Value::String(_) | Value::Array(_) | Value::Object(_) => {
            None
        }
    }
}

fn scalar_paths(value: &Value) -> BTreeMap<String, &Value> {
    fn visit<'a>(value: &'a Value, path: &str, found: &mut BTreeMap<String, &'a Value>) {
        match value {
            Value::Object(values) => {
                for (name, value) in values {
                    let escaped = name.replace('~', "~0").replace('/', "~1");
                    visit(value, &format!("{path}/{escaped}"), found);
                }
            }
            Value::Array(_) => {}
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                found.insert(path.to_owned(), value);
            }
        }
    }
    let mut found = BTreeMap::new();
    visit(value, "", &mut found);
    found
}

fn schema(values: &[&Value]) -> Value {
    if values.iter().all(|value| value.is_object()) {
        return object_schema(values);
    }
    if values.iter().all(|value| value.is_array()) {
        let items = values
            .iter()
            .filter_map(|value| value.as_array())
            .flatten()
            .collect::<Vec<_>>();
        return if items.is_empty() {
            json!({ "type": "array", "items": {} })
        } else {
            json!({ "type": "array", "items": schema(&items) })
        };
    }
    let variants = values
        .iter()
        .map(|value| scalar_schema(value))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect::<Vec<_>>();
    if variants.len() == 1 {
        variants.into_iter().next().unwrap_or_else(|| json!({}))
    } else {
        json!({ "oneOf": variants })
    }
}

fn object_schema(values: &[&Value]) -> Value {
    let objects = values
        .iter()
        .filter_map(|value| value.as_object())
        .collect::<Vec<_>>();
    let names = objects
        .iter()
        .flat_map(|object| object.keys().cloned())
        .collect::<BTreeSet<_>>();
    let mut properties = Map::new();
    let mut required = Vec::new();
    for name in names {
        let samples = objects
            .iter()
            .filter_map(|object| object.get(&name))
            .collect::<Vec<_>>();
        if samples.len() == objects.len() {
            required.push(Value::String(name.clone()));
        }
        properties.insert(name, schema(&samples));
    }
    let mut inferred = Map::new();
    inferred.insert("type".to_owned(), Value::String("object".to_owned()));
    inferred.insert("properties".to_owned(), Value::Object(properties));
    if !required.is_empty() {
        inferred.insert("required".to_owned(), Value::Array(required));
    }
    Value::Object(inferred)
}

fn scalar_schema(value: &Value) -> (String, Value) {
    let schema = match value {
        Value::Null => json!({ "type": "null" }),
        Value::Bool(_) => json!({ "type": "boolean" }),
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            json!({ "type": "integer" })
        }
        Value::Number(_) => json!({ "type": "number" }),
        Value::String(_) => json!({ "type": "string" }),
        Value::Array(_) | Value::Object(_) => json!({}),
    };
    (schema.to_string(), schema)
}

fn operation_id(method: &str, path: &str) -> String {
    let mut result = method.to_ascii_lowercase();
    for part in path.split(|character: char| !character.is_ascii_alphanumeric()) {
        if part.is_empty() {
            continue;
        }
        let mut characters = part.chars();
        if let Some(first) = characters.next() {
            result.extend(first.to_uppercase());
            result.extend(characters);
        }
    }
    result
}

fn webhook_contracts(exchanges: &[Exchange]) -> Map<String, Value> {
    let mut grouped = BTreeMap::<String, Vec<&Exchange>>::new();
    for exchange in exchanges
        .iter()
        .filter(|exchange| exchange.direction == Direction::Webhook)
    {
        grouped
            .entry(webhook_name(exchange))
            .or_default()
            .push(exchange);
    }
    grouped
        .into_iter()
        .map(|(name, observations)| {
            let method = observations
                .first()
                .map(|exchange| exchange.method.to_ascii_lowercase())
                .unwrap_or_else(|| "post".to_owned());
            let content = message_content(
                observations
                    .iter()
                    .map(|exchange| (&exchange.request.content_type, &exchange.request.body)),
            );
            let operation = json!({
                "operationId": format!("receive{}", pascal(&name)),
                "summary": format!("Receive observed {name} webhook"),
                "requestBody": { "required": true, "content": content },
                "responses": { "204": { "description": "Webhook accepted" } },
                "x-simile-learning": { "observations": observations.len() }
            });
            let mut path_item = Map::new();
            path_item.insert(method, operation);
            (name, Value::Object(path_item))
        })
        .collect()
}

fn webhook_name(exchange: &Exchange) -> String {
    if let Some(Value::String(name)) = exchange
        .request
        .body
        .json()
        .and_then(|value| value.get("type").or_else(|| value.get("event")))
    {
        return name.clone();
    }
    "observed_event".to_owned()
}

fn pascal(value: &str) -> String {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
                .unwrap_or_default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::{Exchange, ObservedMessage};

    fn message(value: Value) -> ObservedMessage {
        ObservedMessage {
            headers: BTreeMap::new(),
            content_type: Some("application/json".to_owned()),
            bytes: value.to_string().len(),
            body: ObservedBody::Json { value },
        }
    }

    #[test]
    fn infers_a_binding_only_after_repeated_supporting_observations() {
        let exchanges = [("cu_first", "Ada Lovelace"), ("cu_second", "Grace Hopper")]
            .into_iter()
            .enumerate()
            .map(|(index, (id, name))| Exchange {
                sequence: index as u64 + 1,
                observed_at_milliseconds: 0,
                direction: Direction::Api,
                method: "POST".to_owned(),
                path: "/customers".to_owned(),
                query: None,
                request: message(json!({ "id": id, "name": name })),
                status: 201,
                response: message(json!({ "id": id, "name": name, "active": true })),
                elapsed_microseconds: Duration::from_millis(1).as_micros(),
            })
            .collect::<Vec<_>>();

        let inferred = infer("acme", "https://api.acme.test", "learn_1", &exchanges);
        let bindings =
            &inferred.document["paths"]["/customers"]["post"]["x-simile-from-request"]["bindings"];
        assert_eq!(inferred.bindings, 2);
        assert!(bindings.as_array().is_some_and(|bindings| {
            bindings.contains(&json!({ "from": "/id", "to": "/id" }))
                && bindings.contains(&json!({ "from": "/name", "to": "/name" }))
        }));
        simile_openapi::parse(
            &serde_norway::to_string(&inferred.document).unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn emits_observed_webhooks_as_openapi_webhook_contracts() {
        let exchange = Exchange {
            sequence: 1,
            observed_at_milliseconds: 0,
            direction: Direction::Webhook,
            method: "POST".to_owned(),
            path: "/.simile/webhooks/acme".to_owned(),
            query: None,
            request: message(json!({
                "type": "payment.succeeded",
                "data": { "id": "pay_123" }
            })),
            status: 204,
            response: ObservedMessage {
                headers: BTreeMap::new(),
                content_type: None,
                bytes: 0,
                body: ObservedBody::Empty,
            },
            elapsed_microseconds: Duration::from_millis(1).as_micros(),
        };

        let inferred = infer("acme", "https://api.acme.test", "learn_1", &[exchange]);
        assert!(inferred.document["webhooks"]["payment.succeeded"]["post"].is_object());
        simile_openapi::parse(
            &serde_norway::to_string(&inferred.document).unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
    }
}
