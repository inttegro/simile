use hcl::expr::Expression;
use hcl::{Attribute, Block, Body, Structure};

use crate::ast::{
    ContractDeclaration, ListenDeclaration, Module, ServiceDeclaration, ServiceDocument, Source,
    SourceDeclaration,
};
use crate::parse::{
    apply_error_set, async_operation, attribute, backend, behavior, effect, error_set, fault,
    fault_profile, fault_uses, labels, optional_description, protocol_error, reject_attributes,
    reject_blocks, scenario, string_array,
};
use crate::webhook::{webhook_contract, webhook_policy};
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
        backends: Vec::new(),
        behaviors: Vec::new(),
        error_sets: Vec::new(),
        error_set_applications: Vec::new(),
        protocol_errors: Vec::new(),
        scenarios: Vec::new(),
        fault_uses: Vec::new(),
        faults: Vec::new(),
        fault_profiles: Vec::new(),
        async_operations: Vec::new(),
        effects: Vec::new(),
        webhooks: Vec::new(),
        webhook_policies: Vec::new(),
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
                "backend" => inline_behavior.backends.push(backend(block, source.name)?),
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
                "faults" => inline_behavior
                    .fault_uses
                    .extend(fault_uses(block, source.name)?),
                "fault" => inline_behavior.faults.push(fault(block, source.name)?),
                "fault_profile" => inline_behavior
                    .fault_profiles
                    .push(fault_profile(block, source.name)?),
                "async" => inline_behavior
                    .async_operations
                    .push(async_operation(block, source.name)?),
                "effect" => inline_behavior.effects.push(effect(block, source.name)?),
                "webhook" => inline_behavior
                    .webhooks
                    .push(webhook_contract(block, source.name)?),
                "webhooks" => inline_behavior
                    .webhook_policies
                    .push(webhook_policy(block, source.name)?),
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
    reject_attributes(
        block.body(),
        source,
        "service",
        &["behaviors", "description"],
    )?;
    let behaviors = attribute(block.body(), "behaviors")
        .map(|value| string_array(value, source, "service", "behaviors"))
        .transpose()?
        .unwrap_or_default();
    let listen = one_nested(block.body(), source, "service", "listen", parse_listen)?;
    let contract = contract_nested(block.body(), source)?;
    for nested in block.body().blocks() {
        if nested.identifier() != "listen"
            && nested.identifier() != "openapi"
            && nested.identifier() != "smithy"
            && nested.identifier() != "typescript"
        {
            return Err(Error::UnknownNestedBlock {
                file: source.to_owned(),
                block: "service".to_owned(),
                nested: nested.identifier().to_owned(),
            });
        }
    }

    Ok(ServiceDeclaration {
        name: name.to_owned(),
        description: optional_description(block.body(), source, "service")?,
        listen,
        contract,
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

fn contract_nested(body: &Body, source: &str) -> Result<ContractDeclaration> {
    let mut contracts = body.blocks().filter_map(|block| {
        let format = match block.identifier() {
            "openapi" => ContractFormat::OpenApi,
            "smithy" => ContractFormat::Smithy,
            "typescript" => ContractFormat::TypeScript,
            _ => return None,
        };
        Some((format, block))
    });
    let Some((format, block)) = contracts.next() else {
        return Err(Error::MissingNestedBlock {
            file: source.to_owned(),
            block: "service".to_owned(),
            nested: "openapi, smithy, or typescript",
        });
    };
    if contracts.next().is_some() {
        return Err(Error::DuplicateNestedBlock {
            file: source.to_owned(),
            block: "service".to_owned(),
            nested: "contract source",
        });
    }
    let source_declaration = parse_contract_source(block, source)?;
    Ok(match format {
        ContractFormat::OpenApi => ContractDeclaration::OpenApi(source_declaration),
        ContractFormat::Smithy => ContractDeclaration::Smithy(source_declaration),
        ContractFormat::TypeScript => ContractDeclaration::TypeScript(source_declaration),
    })
}

#[derive(Clone, Copy)]
enum ContractFormat {
    OpenApi,
    Smithy,
    TypeScript,
}

fn parse_contract_source(block: &Block, source: &str) -> Result<SourceDeclaration> {
    let name = block.identifier();
    labels(block, source, 0)?;
    reject_blocks(block.body(), source, name)?;
    reject_attributes(block.body(), source, name, &["path", "url"])?;
    let path = attribute(block.body(), "path")
        .map(|value| string(value, source, name, "path"))
        .transpose()?;
    let url = attribute(block.body(), "url")
        .map(|value| string(value, source, name, "url"))
        .transpose()?;
    match (path, url) {
        (Some(path), None) => Ok(SourceDeclaration::Path(path)),
        (None, Some(url)) => Ok(SourceDeclaration::Url(url)),
        _ => Err(Error::ExclusiveAttributes {
            file: source.to_owned(),
            block: name.to_owned(),
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

backend "queue" "orders" {}

async "dispatch_order" {
  uses = ["queue.orders"]
}

behavior "createOrder" {
  response { status = 201 }
}

fault "slow_response" {
  phase = "before_response"
  delay { milliseconds = 25 }
}

faults {
  use "http.response_lost" {}
}

fault_profile "rough_network" {
  probability = 0.01
  faults      = ["slow_response"]
}

scenario "slow_order" {
  inject "createOrder" { fault = "slow_response" }
}

webhooks {
  ordering                    = "best_effort"
  out_of_order_probability    = 0.2
  reorder_window              = "3s"
  concurrency                 = 4
}
"#,
        ))
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(document.service.name, "inttegro");
        assert_eq!(document.service.listen.host, "127.0.0.1");
        assert_eq!(document.service.listen.port, 4242);
        assert_eq!(
            document.service.contract,
            ContractDeclaration::OpenApi(SourceDeclaration::Path("./commerce.yml".to_owned()))
        );
        assert_eq!(document.service.behaviors, ["./orders.hcl"]);
        assert_eq!(document.inline_behavior.backends.len(), 1);
        assert_eq!(document.inline_behavior.async_operations.len(), 1);
        assert_eq!(document.inline_behavior.behaviors.len(), 1);
        assert_eq!(document.inline_behavior.fault_uses.len(), 1);
        assert_eq!(document.inline_behavior.faults.len(), 1);
        assert_eq!(document.inline_behavior.fault_profiles.len(), 1);
        assert_eq!(document.inline_behavior.scenarios[0].faults.len(), 1);
        assert_eq!(document.inline_behavior.webhook_policies.len(), 1);
        assert_eq!(
            document.inline_behavior.webhook_policies[0],
            crate::WebhookPolicyDeclaration {
                ordering: simile_spec::WebhookOrdering::BestEffort,
                out_of_order_probability_ppm: Some(200_000),
                reorder_window_milliseconds: Some(3_000),
                concurrency: Some(4),
            }
        );
    }

    #[test]
    fn rejects_reordering_controls_for_strict_webhooks() {
        let result = parse_service(Source::new(
            "service.hcl",
            r#"
service "demo" {
  listen { port = 4242 }
  openapi { path = "./openapi.yml" }
}

webhooks {
  ordering                 = "strict"
  out_of_order_probability = 0.1
}
"#,
        ));
        assert!(matches!(result, Err(Error::InvalidAttribute { .. })));
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

    #[test]
    fn accepts_exactly_one_supported_contract_frontend() {
        let smithy = parse_service(Source::new(
            "service.hcl",
            r#"
service "demo" {
  listen { port = 4242 }
  smithy { path = "./service.smithy" }
}
"#,
        ))
        .unwrap_or_else(|error| panic!("Smithy service must parse: {error}"));
        assert_eq!(
            smithy.service.contract,
            ContractDeclaration::Smithy(SourceDeclaration::Path("./service.smithy".to_owned()))
        );

        let typescript = parse_service(Source::new(
            "service.hcl",
            r#"
service "demo" {
  listen { port = 4242 }
  typescript { path = "./typescript/commerce.d.ts" }
}
"#,
        ))
        .unwrap_or_else(|error| panic!("TypeScript service must parse: {error}"));
        assert_eq!(
            typescript.service.contract,
            ContractDeclaration::TypeScript(SourceDeclaration::Path(
                "./typescript/commerce.d.ts".to_owned()
            ))
        );

        let multiple = parse_service(Source::new(
            "service.hcl",
            r#"
service "demo" {
  listen { port = 4242 }
  openapi { path = "./openapi.yml" }
  smithy { path = "./service.smithy" }
}
"#,
        ));
        assert!(matches!(multiple, Err(Error::DuplicateNestedBlock { .. })));
    }
}
