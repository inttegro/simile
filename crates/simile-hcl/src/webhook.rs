use std::collections::{BTreeMap, BTreeSet};

use hcl::expr::{Expression, ObjectKey};
use hcl::{Block, Body};
use simile_spec::{EffectValue, HttpMethod, WebhookOrdering, WebhookPolicy};

use crate::ast::{WebhookDeclaration, WebhookEmissionDeclaration, WebhookPolicyDeclaration};
use crate::parse::{
    attribute, duration_milliseconds, effect_value, labels, non_empty_string, optional_description,
    optional_u64, parse_duration, probability_ppm, reject_attributes, reject_blocks,
};
use crate::{Error, Result};

/// Parse the service-wide webhook delivery infrastructure policy.
///
/// Contract declarations and individual emissions belong to their own source
/// layers. This block only describes the ordinary behavior of the delivery
/// infrastructure: concurrency and the bounded probability that independent
/// occurrences overtake one another.
pub(crate) fn webhook_policy(block: &Block, source: &str) -> Result<WebhookPolicyDeclaration> {
    labels(block, source, 0)?;
    reject_blocks(block.body(), source, "webhooks")?;
    reject_attributes(
        block.body(),
        source,
        "webhooks",
        &[
            "ordering",
            "out_of_order_probability",
            "reorder_window",
            "concurrency",
        ],
    )?;

    let ordering = attribute(block.body(), "ordering")
        .map(|attribute| non_empty_string(attribute, source, "webhooks", "ordering"))
        .transpose()?
        .map_or(Ok(WebhookOrdering::BestEffort), |ordering| {
            match ordering.as_str() {
                "best_effort" => Ok(WebhookOrdering::BestEffort),
                "strict" => Ok(WebhookOrdering::Strict),
                _ => Err(invalid(source, "ordering", "`best_effort` or `strict`")),
            }
        })?;
    let out_of_order_probability_ppm = attribute(block.body(), "out_of_order_probability")
        .map(|attribute| {
            probability_ppm(
                attribute,
                source,
                "webhooks",
                "out_of_order_probability",
                true,
            )
        })
        .transpose()?;
    let reorder_window_milliseconds = attribute(block.body(), "reorder_window")
        .map(|attribute| duration_milliseconds(attribute, source, "webhooks", "reorder_window"))
        .transpose()?;
    let concurrency = match optional_u64(block.body(), source, "webhooks", "concurrency")? {
        Some(value) => Some(
            u16::try_from(value)
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    invalid(
                        source,
                        "concurrency",
                        "a positive integer no greater than 65535",
                    )
                })?,
        ),
        None => None,
    };

    if ordering == WebhookOrdering::Strict {
        if out_of_order_probability_ppm.is_some_and(|probability| probability > 0) {
            return Err(invalid(
                source,
                "out_of_order_probability",
                "zero or omitted when ordering is `strict`",
            ));
        }
        if reorder_window_milliseconds.is_some() {
            return Err(invalid(
                source,
                "reorder_window",
                "omitted when ordering is `strict`",
            ));
        }
    } else if concurrency == Some(1)
        && out_of_order_probability_ppm
            .unwrap_or(WebhookPolicy::default().out_of_order_probability_ppm)
            > 0
    {
        return Err(invalid(
            source,
            "concurrency",
            "at least 2 when out-of-order delivery has a non-zero probability",
        ));
    }

    Ok(WebhookPolicyDeclaration {
        ordering,
        out_of_order_probability_ppm,
        reorder_window_milliseconds,
        concurrency,
    })
}

/// Parse a provider-authored webhook contract missing from the API contract.
///
/// Its body is deliberately unconstrained here: effect emission supplies the
/// public payload value, while OpenAPI remains the preferred place for a
/// reusable request schema.
pub(crate) fn webhook_contract(block: &Block, source: &str) -> Result<WebhookDeclaration> {
    labels(block, source, 1)?;
    reject_blocks(block.body(), source, "webhook")?;
    reject_attributes(
        block.body(),
        source,
        "webhook",
        &["description", "method", "content_type"],
    )?;
    let name = block.labels()[0].as_str().trim();
    if name.is_empty() {
        return Err(Error::InvalidAttribute {
            file: source.to_owned(),
            block: "webhook".to_owned(),
            attribute: "label".to_owned(),
            expected: "a non-empty webhook name",
        });
    }
    let method = attribute(block.body(), "method")
        .map(|attribute| non_empty_string(attribute, source, "webhook", "method"))
        .transpose()?
        .map_or(Ok(HttpMethod::Post), |method| parse_method(source, &method))?;
    let content_type = attribute(block.body(), "content_type")
        .map(|attribute| non_empty_string(attribute, source, "webhook", "content_type"))
        .transpose()?
        .unwrap_or_else(|| "application/json".to_owned());
    Ok(WebhookDeclaration {
        name: name.to_owned(),
        description: optional_description(block.body(), source, "webhook")?,
        method,
        content_type,
    })
}

/// Parse the compact webhook maps owned by an effect.
///
/// `emit` is the occurrence set. `after`, `delay`, and `to` are sparse maps
/// that can only refine names in that set. Keeping the maps together makes a
/// multi-webhook consequence compact without weakening compile-time graph
/// validation.
pub(crate) fn webhook_emissions(
    body: &Body,
    source: &str,
) -> Result<Vec<WebhookEmissionDeclaration>> {
    let emissions = match attribute(body, "emit") {
        Some(attribute) => effect_value_map(attribute.expr(), source, "emit")?,
        None => {
            if ["after", "delay", "to"]
                .into_iter()
                .any(|name| attribute(body, name).is_some())
            {
                return Err(Error::MissingAttribute {
                    file: source.to_owned(),
                    block: "effect".to_owned(),
                    attribute: "emit",
                });
            }
            return Ok(Vec::new());
        }
    };
    if emissions.is_empty() {
        return Err(invalid_effect_map(
            source,
            "emit",
            "a non-empty object mapping webhook names to payload values",
        ));
    }

    let after = attribute(body, "after")
        .map(|attribute| string_array_map(attribute.expr(), source, "after"))
        .transpose()?
        .unwrap_or_default();
    let delay = attribute(body, "delay")
        .map(|attribute| duration_map(attribute.expr(), source, "delay"))
        .transpose()?
        .unwrap_or_default();
    let mut targets = attribute(body, "to")
        .map(|attribute| effect_value_map(attribute.expr(), source, "to"))
        .transpose()?
        .unwrap_or_default();

    for name in after.keys().chain(delay.keys()).chain(targets.keys()) {
        if !emissions.contains_key(name) {
            return Err(Error::UnknownWebhookEmissionOverride {
                file: source.to_owned(),
                webhook: name.clone(),
            });
        }
    }

    for (name, dependencies) in &after {
        let mut unique = BTreeSet::new();
        for dependency in dependencies {
            if dependency == name
                || !emissions.contains_key(dependency)
                || !unique.insert(dependency.clone())
            {
                return Err(Error::InvalidWebhookDependency {
                    file: source.to_owned(),
                    webhook: name.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
    }

    emissions
        .into_iter()
        .map(|(name, body)| {
            Ok(WebhookEmissionDeclaration {
                webhook: name.clone(),
                body,
                target: targets.remove(&name),
                after: after.get(&name).cloned().unwrap_or_default(),
                delay_milliseconds: delay.get(&name).copied().unwrap_or_default(),
            })
        })
        .collect()
}

fn effect_value_map(
    expression: &Expression,
    source: &str,
    attribute: &str,
) -> Result<BTreeMap<String, EffectValue>> {
    let Expression::Object(fields) = expression else {
        return Err(invalid_effect_map(
            source,
            attribute,
            "an object keyed by webhook name",
        ));
    };
    let mut values = BTreeMap::new();
    for (key, value) in fields {
        let name = object_key(key).ok_or_else(|| {
            invalid_effect_map(source, attribute, "an object keyed by webhook name")
        })?;
        if values
            .insert(name.clone(), effect_value(value, source, attribute)?)
            .is_some()
        {
            return Err(Error::DuplicateWebhookEmission {
                file: source.to_owned(),
                webhook: name,
            });
        }
    }
    Ok(values)
}

fn string_array_map(
    expression: &Expression,
    source: &str,
    attribute: &str,
) -> Result<BTreeMap<String, Vec<String>>> {
    let expected = "an object mapping webhook names to arrays of webhook names";
    let Expression::Object(fields) = expression else {
        return Err(invalid_effect_map(source, attribute, expected));
    };
    let mut mapped = BTreeMap::new();
    for (key, value) in fields {
        let name =
            object_key(key).ok_or_else(|| invalid_effect_map(source, attribute, expected))?;
        let Expression::Array(values) = value else {
            return Err(invalid_effect_map(source, attribute, expected));
        };
        let values = values
            .iter()
            .map(|value| match value {
                Expression::String(value) => Ok(value.clone()),
                _ => Err(invalid_effect_map(source, attribute, expected)),
            })
            .collect::<Result<Vec<_>>>()?;
        insert_unique(&mut mapped, name, values, source)?;
    }
    Ok(mapped)
}

fn duration_map(
    expression: &Expression,
    source: &str,
    attribute: &str,
) -> Result<BTreeMap<String, u64>> {
    let expected = "an object mapping webhook names to durations";
    let Expression::Object(fields) = expression else {
        return Err(invalid_effect_map(source, attribute, expected));
    };
    let mut mapped = BTreeMap::new();
    for (key, value) in fields {
        let name =
            object_key(key).ok_or_else(|| invalid_effect_map(source, attribute, expected))?;
        let Expression::String(value) = value else {
            return Err(invalid_effect_map(source, attribute, expected));
        };
        let value = parse_duration(value, source, "effect", attribute)?;
        insert_unique(&mut mapped, name, value, source)?;
    }
    Ok(mapped)
}

fn insert_unique<T>(
    values: &mut BTreeMap<String, T>,
    name: String,
    value: T,
    source: &str,
) -> Result<()> {
    if values.insert(name.clone(), value).is_some() {
        return Err(Error::DuplicateWebhookEmission {
            file: source.to_owned(),
            webhook: name,
        });
    }
    Ok(())
}

fn object_key(key: &ObjectKey) -> Option<String> {
    match key {
        ObjectKey::Identifier(key) => Some(key.as_str().to_owned()),
        ObjectKey::Expression(Expression::String(key)) => Some(key.clone()),
        _ => None,
    }
}

fn invalid_effect_map(source: &str, attribute: &str, expected: &'static str) -> Error {
    Error::InvalidAttribute {
        file: source.to_owned(),
        block: "effect".to_owned(),
        attribute: attribute.to_owned(),
        expected,
    }
}

fn parse_method(source: &str, method: &str) -> Result<HttpMethod> {
    match method.to_ascii_uppercase().as_str() {
        "GET" => Ok(HttpMethod::Get),
        "PUT" => Ok(HttpMethod::Put),
        "POST" => Ok(HttpMethod::Post),
        "DELETE" => Ok(HttpMethod::Delete),
        "OPTIONS" => Ok(HttpMethod::Options),
        "HEAD" => Ok(HttpMethod::Head),
        "PATCH" => Ok(HttpMethod::Patch),
        "TRACE" => Ok(HttpMethod::Trace),
        "QUERY" => Ok(HttpMethod::Query),
        _ if method
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || byte == b'-') =>
        {
            Ok(HttpMethod::Additional(method.to_ascii_uppercase()))
        }
        _ => Err(Error::InvalidAttribute {
            file: source.to_owned(),
            block: "webhook".to_owned(),
            attribute: "method".to_owned(),
            expected: "an HTTP method token",
        }),
    }
}

fn invalid(source: &str, attribute: &str, expected: &'static str) -> Error {
    Error::InvalidAttribute {
        file: source.to_owned(),
        block: "webhooks".to_owned(),
        attribute: attribute.to_owned(),
        expected,
    }
}
