use hcl::expr::Expression;
use hcl::{Attribute, Block, Body, Structure};

use crate::ast::{
    ListenDeclaration, Module, OpenApiDeclaration, ServiceDeclaration, ServiceDocument, Source,
};
use crate::parse::{
    apply_error_set, async_operation, attribute, behavior, error_set, labels, protocol_error,
    reject_attributes, reject_blocks, resource, scenario, string_array,
};
use crate::{Error, Result};

const DEFAULT_HOST: &str = "127.0.0.1";

/// Parse a complete runnable-service document and any inline behavior declarations it contains.
pub fn parse_service(source: Source<'_>) -> Result<ServiceDocument> {
    let body = hcl::parse(source.contents).map_err(|error| Error::Parse {
        name: source.name.to_owned(),
        source: error,
    })?;
    let mut service = None;
    let mut inline_behavior = Module {
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
                    file: source.name.to_owned(),
                    attribute: attribute.key().to_owned(),
                });
            }
            Structure::Block(block) => match block.identifier() {
                "service" => {
                    if service.is_some() {
                        return Err(Error::DuplicateService {
                            file: source.name.to_owned(),
                        });
                    }
                    service = Some(parse_service_block(block, source.name)?);
                }
                "resource" => inline_behavior
                    .resources
                    .push(resource(block, source.name)?),
                "behavior" => inline_behavior
                    .behaviors
                    .push(behavior(block, source.name)?),
                "error_set" => inline_behavior
                    .error_sets
                    .push(error_set(block, source.name)?),
                "apply_error_set" => inline_behavior
                    .error_set_applications
                    .push(apply_error_set(block, source.name)?),
                "protocol_error" => inline_behavior
                    .protocol_errors
                    .push(protocol_error(block, source.name)?),
                "scenario" => inline_behavior
                    .scenarios
                    .push(scenario(block, source.name)?),
                "async" => inline_behavior
                    .async_operations
                    .push(async_operation(block, source.name)?),
                unknown => {
                    return Err(Error::UnknownBlock {
                        file: source.name.to_owned(),
                        block: unknown.to_owned(),
                    });
                }
            },
        }
    }

    let service = service.ok_or_else(|| Error::MissingService {
        file: source.name.to_owned(),
    })?;
    Ok(ServiceDocument {
        service,
        inline_behavior,
    })
}

fn parse_service_block(block: &Block, source: &str) -> Result<ServiceDeclaration> {
    labels(block, source, 1)?;
    let name = block.labels()[0].as_str();
    if name.trim().is_empty() {
        return Err(Error::InvalidAttribute {
            file: source.to_owned(),
            block: "service".to_owned(),
            attribute: "label".to_owned(),
            expected: "a non-empty service name",
        });
    }
    reject_attributes(block.body(), source, "service", &["behaviors"])?;
    let behaviors = attribute(block.body(), "behaviors")
        .map(|value| string_array(value, source, "service", "behaviors"))
        .transpose()?
        .unwrap_or_default();
    let listen = one_nested(block.body(), source, "service", "listen", parse_listen)?;
    let openapi = one_nested(block.body(), source, "service", "openapi", parse_openapi)?;
    for nested in block.body().blocks() {
        if nested.identifier() != "listen" && nested.identifier() != "openapi" {
            return Err(Error::UnknownNestedBlock {
                file: source.to_owned(),
                block: "service".to_owned(),
                nested: nested.identifier().to_owned(),
            });
        }
    }

    Ok(ServiceDeclaration {
        name: name.to_owned(),
        listen,
        openapi,
        behaviors,
    })
}

fn one_nested<T>(
    body: &Body,
    source: &str,
    parent: &str,
    name: &'static str,
    parse: impl FnOnce(&Block, &str) -> Result<T>,
) -> Result<T> {
    let mut blocks = body.blocks().filter(|block| block.identifier() == name);
    let block = blocks.next().ok_or_else(|| Error::MissingNestedBlock {
        file: source.to_owned(),
        block: parent.to_owned(),
        nested: name,
    })?;
    if blocks.next().is_some() {
        return Err(Error::DuplicateNestedBlock {
            file: source.to_owned(),
            block: parent.to_owned(),
            nested: name,
        });
    }
    parse(block, source)
}

fn parse_listen(block: &Block, source: &str) -> Result<ListenDeclaration> {
    labels(block, source, 0)?;
    reject_blocks(block.body(), source, "listen")?;
    reject_attributes(block.body(), source, "listen", &["host", "port"])?;
    let host = attribute(block.body(), "host")
        .map(|value| string(value, source, "listen", "host"))
        .transpose()?
        .unwrap_or_else(|| DEFAULT_HOST.to_owned());
    let port = attribute(block.body(), "port")
        .ok_or_else(|| Error::MissingAttribute {
            file: source.to_owned(),
            block: "listen".to_owned(),
            attribute: "port",
        })?
        .expr();
    let port = match port {
        Expression::Number(value) => value.as_u64(),
        _ => None,
    }
    .and_then(|value| u16::try_from(value).ok())
    .filter(|value| *value != 0)
    .ok_or_else(|| Error::InvalidAttribute {
        file: source.to_owned(),
        block: "listen".to_owned(),
        attribute: "port".to_owned(),
        expected: "an integer from 1 through 65535",
    })?;

    Ok(ListenDeclaration { host, port })
}

fn parse_openapi(block: &Block, source: &str) -> Result<OpenApiDeclaration> {
    labels(block, source, 0)?;
    reject_blocks(block.body(), source, "openapi")?;
    reject_attributes(block.body(), source, "openapi", &["path", "url"])?;
    let path = attribute(block.body(), "path")
        .map(|value| string(value, source, "openapi", "path"))
        .transpose()?;
    let url = attribute(block.body(), "url")
        .map(|value| string(value, source, "openapi", "url"))
        .transpose()?;
    match (path, url) {
        (Some(path), None) => Ok(OpenApiDeclaration::Path(path)),
        (None, Some(url)) => Ok(OpenApiDeclaration::Url(url)),
        _ => Err(Error::ExclusiveAttributes {
            file: source.to_owned(),
            block: "openapi".to_owned(),
            attributes: "`path` or `url`",
        }),
    }
}

fn string(attribute: &Attribute, source: &str, block: &str, name: &str) -> Result<String> {
    match attribute.expr() {
        Expression::String(value) if !value.trim().is_empty() => Ok(value.clone()),
        _ => Err(Error::InvalidAttribute {
            file: source.to_owned(),
            block: block.to_owned(),
            attribute: name.to_owned(),
            expected: "a non-empty string",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_service_and_inline_behavior() {
        let document = parse_service(Source::new(
            "inttegro.hcl",
            r#"
service "inttegro" {
  listen {
    host = "127.0.0.1"
    port = 4242
  }

  openapi {
    path = "./commerce.yml"
  }

  behaviors = ["./orders.hcl"]
}

resource "queue" "orders" {}

async "dispatch_order" {
  uses = ["queue.orders"]
}

behavior "createOrder" {
  response { status = 201 }
  trigger "dispatch_order" {}
}
"#,
        ))
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(document.service.name, "inttegro");
        assert_eq!(document.service.listen.host, "127.0.0.1");
        assert_eq!(document.service.listen.port, 4242);
        assert_eq!(
            document.service.openapi,
            OpenApiDeclaration::Path("./commerce.yml".to_owned())
        );
        assert_eq!(document.service.behaviors, ["./orders.hcl"]);
        assert_eq!(document.inline_behavior.resources.len(), 1);
        assert_eq!(document.inline_behavior.async_operations.len(), 1);
        assert_eq!(document.inline_behavior.behaviors.len(), 1);
    }

    #[test]
    fn rejects_ambiguous_openapi_sources_and_zero_ports() {
        let ambiguous = parse_service(Source::new(
            "service.hcl",
            r#"
service "demo" {
  listen { port = 4242 }
  openapi {
    path = "./openapi.yml"
    url  = "https://example.test/openapi.yml"
  }
}
"#,
        ));
        assert!(matches!(ambiguous, Err(Error::ExclusiveAttributes { .. })));

        let zero = parse_service(Source::new(
            "service.hcl",
            r#"
service "demo" {
  listen { port = 0 }
  openapi { path = "./openapi.yml" }
}
"#,
        ));
        assert!(matches!(zero, Err(Error::InvalidAttribute { .. })));
    }
}
