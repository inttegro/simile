use std::str::FromStr;

use hcl::eval::{Context, Evaluate};
use hcl::expr::Expression;
use hcl::{Attribute, Block, Body, Structure};
use simile_spec::{ResourceKind, ResourceRef};

use crate::ast::{
    AsyncDeclaration, BehaviorDeclaration, ErrorSetApplicationDeclaration, ErrorSetDeclaration,
    Module, OutcomeDeclaration, ProtocolErrorDeclaration, ResourceDeclaration, ScenarioDeclaration,
    ScenarioSelectionDeclaration, Source,
};
use crate::{Error, Result};

pub fn parse(source: Source<'_>) -> Result<Module> {
    let body = hcl::parse(source.contents).map_err(|error| Error::Parse {
        name: source.name.to_owned(),
        source: error,
    })?;
    let mut module = Module {
        source: source.name.to_owned(),
        resources: Vec::new(),
        behaviors: Vec::new(),
        error_sets: Vec::new(),
        error_set_applications: Vec::new(),
        protocol_errors: Vec::new(),
        scenarios: Vec::new(),
        async_operations: Vec::new(),
    };

    for structure in body.iter() {
        match structure {
            Structure::Attribute(attribute) => {
                return Err(Error::TopLevelAttribute {
                    file: module.source,
                    attribute: attribute.key().to_owned(),
                });
            }
            Structure::Block(block) => match block.identifier() {
                "resource" => module.resources.push(resource(block, &module.source)?),
                "behavior" => module.behaviors.push(behavior(block, &module.source)?),
                "error_set" => module.error_sets.push(error_set(block, &module.source)?),
                "apply_error_set" => module
                    .error_set_applications
                    .push(apply_error_set(block, &module.source)?),
                "protocol_error" => module
                    .protocol_errors
                    .push(protocol_error(block, &module.source)?),
                "scenario" => module.scenarios.push(scenario(block, &module.source)?),
                "async" => module
                    .async_operations
                    .push(async_operation(block, &module.source)?),
                unknown => {
                    return Err(Error::UnknownBlock {
                        file: module.source,
                        block: unknown.to_owned(),
                    });
                }
            },
        }
    }

    Ok(module)
}

pub(crate) fn resource(block: &Block, source: &str) -> Result<ResourceDeclaration> {
    labels(block, source, 2)?;
    empty_body(block.body(), source, "resource")?;
    let kind = block.labels()[0].as_str();
    let kind = ResourceKind::from_str(kind).map_err(|()| Error::UnknownResourceKind {
        file: source.to_owned(),
        kind: kind.to_owned(),
    })?;

    Ok(ResourceDeclaration {
        kind,
        name: block.labels()[1].as_str().to_owned(),
    })
}

pub(crate) fn behavior(block: &Block, source: &str) -> Result<BehaviorDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(block.body(), source, "behavior", &["error_sets"])?;
    let mut response_status = None;
    let error_sets = attribute(block.body(), "error_sets")
        .map(|value| string_array(value, source, "behavior", "error_sets"))
        .transpose()?
        .unwrap_or_default();
    let mut outcomes = Vec::new();
    let mut triggers = Vec::new();

    for child in block.body().blocks() {
        match child.identifier() {
            "response" => {
                labels(child, source, 0)?;
                if response_status.is_some() {
                    return Err(Error::UnknownNestedBlock {
                        file: source.to_owned(),
                        block: "behavior".to_owned(),
                        nested: "duplicate response".to_owned(),
                    });
                }
                response_status = Some(response(child, source)?);
            }
            "trigger" => {
                labels(child, source, 1)?;
                empty_body(child.body(), source, "trigger")?;
                triggers.push(child.labels()[0].as_str().to_owned());
            }
            "outcome" => outcomes.push(outcome(child, source)?),
            unknown => {
                return Err(Error::UnknownNestedBlock {
                    file: source.to_owned(),
                    block: "behavior".to_owned(),
                    nested: unknown.to_owned(),
                });
            }
        }
    }

    Ok(BehaviorDeclaration {
        operation_id: block.labels()[0].as_str().to_owned(),
        response_status,
        error_sets,
        outcomes,
        triggers,
    })
}

pub(crate) fn error_set(block: &Block, source: &str) -> Result<ErrorSetDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(block.body(), source, "error_set", &[])?;
    let outcomes = block
        .body()
        .blocks()
        .map(|child| {
            if child.identifier() != "outcome" {
                return Err(Error::UnknownNestedBlock {
                    file: source.to_owned(),
                    block: "error_set".to_owned(),
                    nested: child.identifier().to_owned(),
                });
            }
            outcome(child, source)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ErrorSetDeclaration {
        name: block.labels()[0].as_str().to_owned(),
        outcomes,
    })
}

pub(crate) fn apply_error_set(
    block: &Block,
    source: &str,
) -> Result<ErrorSetApplicationDeclaration> {
    labels(block, source, 1)?;
    reject_blocks(block.body(), source, "apply_error_set")?;
    reject_attributes(block.body(), source, "apply_error_set", &["tags"])?;
    let tags = attribute(block.body(), "tags")
        .ok_or_else(|| Error::MissingAttribute {
            file: source.to_owned(),
            block: "apply_error_set".to_owned(),
            attribute: "tags",
        })
        .and_then(|attribute| string_array(attribute, source, "apply_error_set", "tags"))?;
    if tags.is_empty() {
        return Err(Error::InvalidAttribute {
            file: source.to_owned(),
            block: "apply_error_set".to_owned(),
            attribute: "tags".to_owned(),
            expected: "a non-empty array of OpenAPI tags",
        });
    }
    Ok(ErrorSetApplicationDeclaration {
        error_set: block.labels()[0].as_str().to_owned(),
        tags,
    })
}

fn outcome(block: &Block, source: &str) -> Result<OutcomeDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(
        block.body(),
        source,
        "outcome",
        &["status", "content_type", "body"],
    )?;
    let status = status_attribute(block.body(), source, "outcome", "status")?;
    let content_type = attribute(block.body(), "content_type")
        .map(|attribute| string_attribute(attribute, source, "outcome", "content_type"))
        .transpose()?;
    let body = attribute(block.body(), "body")
        .map(|attribute| literal_value(attribute, source, "outcome", "body"))
        .transpose()?;
    let when = block
        .body()
        .blocks()
        .map(|child| {
            if child.identifier() != "when" {
                return Err(Error::UnknownNestedBlock {
                    file: source.to_owned(),
                    block: "outcome".to_owned(),
                    nested: child.identifier().to_owned(),
                });
            }
            labels(child, source, 0)?;
            reject_blocks(child.body(), source, "when")?;
            reject_attributes(child.body(), source, "when", &["path", "equals"])?;
            let path = required_string(child.body(), source, "when", "path")?;
            let equals = attribute(child.body(), "equals")
                .ok_or_else(|| Error::MissingAttribute {
                    file: source.to_owned(),
                    block: "when".to_owned(),
                    attribute: "equals",
                })
                .and_then(|attribute| literal_value(attribute, source, "when", "equals"))?;
            Ok(simile_spec::OutcomeCondition { path, equals })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(OutcomeDeclaration {
        name: block.labels()[0].as_str().to_owned(),
        status,
        content_type,
        body,
        when,
    })
}

pub(crate) fn protocol_error(block: &Block, source: &str) -> Result<ProtocolErrorDeclaration> {
    labels(block, source, 1)?;
    reject_blocks(block.body(), source, "protocol_error")?;
    reject_attributes(
        block.body(),
        source,
        "protocol_error",
        &["statuses", "content_type", "body"],
    )?;
    let statuses = attribute(block.body(), "statuses")
        .ok_or_else(|| Error::MissingAttribute {
            file: source.to_owned(),
            block: "protocol_error".to_owned(),
            attribute: "statuses",
        })
        .and_then(|attribute| status_array(attribute, source, "protocol_error", "statuses"))?;
    let content_type = attribute(block.body(), "content_type")
        .map(|attribute| string_attribute(attribute, source, "protocol_error", "content_type"))
        .transpose()?;
    let body = attribute(block.body(), "body")
        .map(|attribute| literal_value(attribute, source, "protocol_error", "body"))
        .transpose()?;
    Ok(ProtocolErrorDeclaration {
        cause: block.labels()[0].as_str().to_owned(),
        statuses,
        content_type,
        body,
    })
}

pub(crate) fn scenario(block: &Block, source: &str) -> Result<ScenarioDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(block.body(), source, "scenario", &[])?;
    let selections = block
        .body()
        .blocks()
        .map(|child| {
            if child.identifier() != "select" {
                return Err(Error::UnknownNestedBlock {
                    file: source.to_owned(),
                    block: "scenario".to_owned(),
                    nested: child.identifier().to_owned(),
                });
            }
            labels(child, source, 1)?;
            reject_blocks(child.body(), source, "select")?;
            reject_attributes(child.body(), source, "select", &["outcome"])?;
            Ok(ScenarioSelectionDeclaration {
                operation_id: child.labels()[0].as_str().to_owned(),
                outcome: required_string(child.body(), source, "select", "outcome")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ScenarioDeclaration {
        name: block.labels()[0].as_str().to_owned(),
        selections,
    })
}

fn response(block: &Block, source: &str) -> Result<u16> {
    reject_blocks(block.body(), source, "response")?;
    reject_attributes(block.body(), source, "response", &["status"])?;
    status_attribute(block.body(), source, "response", "status")
}

fn status_attribute(body: &Body, source: &str, block: &str, name: &'static str) -> Result<u16> {
    let status = attribute(body, name).ok_or_else(|| Error::MissingAttribute {
        file: source.to_owned(),
        block: block.to_owned(),
        attribute: name,
    })?;
    let value = match status.expr() {
        Expression::Number(value) => value.as_u64(),
        _ => None,
    }
    .and_then(|value| u16::try_from(value).ok())
    .filter(|value| (100..=599).contains(value))
    .ok_or_else(|| Error::InvalidAttribute {
        file: source.to_owned(),
        block: block.to_owned(),
        attribute: name.to_owned(),
        expected: "an HTTP status integer from 100 through 599",
    })?;

    Ok(value)
}

fn status_array(attribute: &Attribute, source: &str, block: &str, name: &str) -> Result<Vec<u16>> {
    let Expression::Array(values) = attribute.expr() else {
        return Err(Error::InvalidAttribute {
            file: source.to_owned(),
            block: block.to_owned(),
            attribute: name.to_owned(),
            expected: "a non-empty array of HTTP status integers",
        });
    };
    let statuses = values
        .iter()
        .map(|value| match value {
            Expression::Number(value) => value.as_u64(),
            _ => None,
        })
        .map(|value| {
            value
                .and_then(|value| u16::try_from(value).ok())
                .filter(|value| (100..=599).contains(value))
                .ok_or_else(|| Error::InvalidAttribute {
                    file: source.to_owned(),
                    block: block.to_owned(),
                    attribute: name.to_owned(),
                    expected: "a non-empty array of HTTP status integers",
                })
        })
        .collect::<Result<Vec<_>>>()?;
    if statuses.is_empty() {
        return Err(Error::InvalidAttribute {
            file: source.to_owned(),
            block: block.to_owned(),
            attribute: name.to_owned(),
            expected: "a non-empty array of HTTP status integers",
        });
    }
    Ok(statuses)
}

fn required_string(body: &Body, source: &str, block: &str, name: &'static str) -> Result<String> {
    attribute(body, name)
        .ok_or_else(|| Error::MissingAttribute {
            file: source.to_owned(),
            block: block.to_owned(),
            attribute: name,
        })
        .and_then(|attribute| string_attribute(attribute, source, block, name))
}

fn string_attribute(
    attribute: &Attribute,
    source: &str,
    block: &str,
    name: &str,
) -> Result<String> {
    match attribute.expr() {
        Expression::String(value) => Ok(value.clone()),
        _ => Err(Error::InvalidAttribute {
            file: source.to_owned(),
            block: block.to_owned(),
            attribute: name.to_owned(),
            expected: "a string",
        }),
    }
}

fn literal_value(
    attribute: &Attribute,
    source: &str,
    block: &str,
    name: &str,
) -> Result<serde_json::Value> {
    let value = attribute
        .expr()
        .evaluate(&Context::new())
        .ok()
        .and_then(|value| hcl::from_value::<serde_json::Value>(value).ok());
    value.ok_or_else(|| Error::InvalidAttribute {
        file: source.to_owned(),
        block: block.to_owned(),
        attribute: name.to_owned(),
        expected: "a literal JSON-compatible HCL value",
    })
}

pub(crate) fn async_operation(block: &Block, source: &str) -> Result<AsyncDeclaration> {
    labels(block, source, 1)?;
    reject_blocks(block.body(), source, "async")?;
    reject_attributes(block.body(), source, "async", &["uses"])?;
    let uses = attribute(block.body(), "uses")
        .map(|value| string_array(value, source, "async", "uses"))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(ResourceRef::parse)
        .collect();

    Ok(AsyncDeclaration {
        name: block.labels()[0].as_str().to_owned(),
        uses,
    })
}

pub(crate) fn string_array(
    attribute: &Attribute,
    source: &str,
    block: &str,
    name: &str,
) -> Result<Vec<String>> {
    let values = match attribute.expr() {
        Expression::Array(values) => values,
        _ => {
            return Err(Error::InvalidAttribute {
                file: source.to_owned(),
                block: block.to_owned(),
                attribute: name.to_owned(),
                expected: "an array of strings",
            });
        }
    };
    values
        .iter()
        .map(|value| match value {
            Expression::String(value) => Ok(value.clone()),
            _ => Err(Error::InvalidAttribute {
                file: source.to_owned(),
                block: block.to_owned(),
                attribute: name.to_owned(),
                expected: "an array of strings",
            }),
        })
        .collect()
}

pub(crate) fn labels(block: &Block, source: &str, expected: usize) -> Result<()> {
    if block.labels().len() == expected {
        return Ok(());
    }
    Err(Error::Labels {
        file: source.to_owned(),
        block: block.identifier().to_owned(),
        expected,
        actual: block.labels().len(),
    })
}

fn empty_body(body: &Body, source: &str, block: &str) -> Result<()> {
    reject_attributes(body, source, block, &[])?;
    reject_blocks(body, source, block)
}

pub(crate) fn reject_attributes(
    body: &Body,
    source: &str,
    block: &str,
    allowed: &[&str],
) -> Result<()> {
    if let Some(attribute) = body
        .attributes()
        .find(|attribute| !allowed.contains(&attribute.key()))
    {
        return Err(Error::UnknownAttribute {
            file: source.to_owned(),
            block: block.to_owned(),
            attribute: attribute.key().to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn reject_blocks(body: &Body, source: &str, block: &str) -> Result<()> {
    if let Some(nested) = body.blocks().next() {
        return Err(Error::UnknownNestedBlock {
            file: source.to_owned(),
            block: block.to_owned(),
            nested: nested.identifier().to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn attribute<'a>(body: &'a Body, name: &str) -> Option<&'a Attribute> {
    body.attributes().find(|attribute| attribute.key() == name)
}
