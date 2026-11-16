use std::str::FromStr;

use hcl::expr::Expression;
use hcl::{Attribute, Block, Body, Structure};
use simile_spec::{ResourceKind, ResourceRef};

use crate::ast::{AsyncDeclaration, BehaviorDeclaration, Module, ResourceDeclaration, Source};
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
    reject_attributes(block.body(), source, "behavior", &[])?;
    let mut response_status = None;
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
        triggers,
    })
}

fn response(block: &Block, source: &str) -> Result<u16> {
    reject_blocks(block.body(), source, "response")?;
    reject_attributes(block.body(), source, "response", &["status"])?;
    let status = attribute(block.body(), "status").ok_or_else(|| Error::MissingAttribute {
        file: source.to_owned(),
        block: "response".to_owned(),
        attribute: "status",
    })?;
    let value = match status.expr() {
        Expression::Number(value) => value.as_u64(),
        _ => None,
    }
    .and_then(|value| u16::try_from(value).ok())
    .filter(|value| (100..=599).contains(value))
    .ok_or_else(|| Error::InvalidAttribute {
        file: source.to_owned(),
        block: "response".to_owned(),
        attribute: "status".to_owned(),
        expected: "an HTTP status integer from 100 through 599",
    })?;

    Ok(value)
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
