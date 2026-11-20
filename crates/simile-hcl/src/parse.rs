use std::str::FromStr;

use hcl::eval::{Context, Evaluate};
use hcl::expr::{Expression, ObjectKey, TraversalOperator};
use hcl::{Attribute, Block, Body, Structure};
use simile_spec::{
    EffectActionKind, EffectValue, FaultAction, FaultPhase, ResourceKind, ResourceRef,
};

use crate::ast::{
    ActionDeclaration, AsyncDeclaration, BackendDeclaration, BehaviorDeclaration,
    EffectDeclaration, ErrorSetApplicationDeclaration, ErrorSetDeclaration, FaultDeclaration,
    FaultProfileDeclaration, FaultUseDeclaration, Module, OutcomeDeclaration,
    ProtocolErrorDeclaration, ScenarioDeclaration, ScenarioFaultDeclaration,
    ScenarioSelectionDeclaration, Source,
};
use crate::auth::principal;
use crate::webhook::{webhook_contract, webhook_emissions, webhook_policy};
use crate::{Error, Result};

pub fn parse(source: Source<'_>) -> Result<Module> {
    let body = hcl::parse(source.contents).map_err(|error| Error::Parse {
        name: source.name.to_owned(),
        source: error,
    })?;
    let mut module = Module {
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
        principals: Vec::new(),
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
                "backend" => module.backends.push(backend(block, &module.source)?),
                "behavior" => module.behaviors.push(behavior(block, &module.source)?),
                "error_set" => module.error_sets.push(error_set(block, &module.source)?),
                "apply_error_set" => module
                    .error_set_applications
                    .push(apply_error_set(block, &module.source)?),
                "protocol_error" => module
                    .protocol_errors
                    .push(protocol_error(block, &module.source)?),
                "scenario" => module.scenarios.push(scenario(block, &module.source)?),
                "faults" => module.fault_uses.extend(fault_uses(block, &module.source)?),
                "fault" => module.faults.push(fault(block, &module.source)?),
                "fault_profile" => module
                    .fault_profiles
                    .push(fault_profile(block, &module.source)?),
                "async" => module
                    .async_operations
                    .push(async_operation(block, &module.source)?),
                "effect" => module.effects.push(effect(block, &module.source)?),
                "webhook" => module
                    .webhooks
                    .push(webhook_contract(block, &module.source)?),
                "webhooks" => module
                    .webhook_policies
                    .push(webhook_policy(block, &module.source)?),
                "principal" => module.principals.push(principal(block, &module.source)?),
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

pub(crate) fn fault_uses(block: &Block, source: &str) -> Result<Vec<FaultUseDeclaration>> {
    labels(block, source, 0)?;
    reject_attributes(block.body(), source, "faults", &[])?;
    block
        .body()
        .blocks()
        .map(|child| {
            if child.identifier() != "use" {
                return Err(Error::UnknownNestedBlock {
                    file: source.to_owned(),
                    block: "faults".to_owned(),
                    nested: child.identifier().to_owned(),
                });
            }
            labels(child, source, 1)?;
            reject_blocks(child.body(), source, "use")?;
            reject_attributes(child.body(), source, "use", &["as", "latency"])?;

            let raw_selector = child.labels()[0].as_str();
            let (exclude, selector) = raw_selector
                .strip_prefix('^')
                .map_or((false, raw_selector), |selector| (true, selector));
            if !valid_fault_selector(selector) {
                return Err(Error::InvalidFaultSelector {
                    file: source.to_owned(),
                    selector: raw_selector.to_owned(),
                });
            }
            let alias = attribute(child.body(), "as")
                .map(|attribute| non_empty_string(attribute, source, "use", "as"))
                .transpose()?;
            let latency_milliseconds = attribute(child.body(), "latency")
                .map(|attribute| duration_milliseconds(attribute, source, "use", "latency"))
                .transpose()?;

            if exclude && (alias.is_some() || latency_milliseconds.is_some()) {
                return Err(Error::ConfiguredFaultExclusion {
                    file: source.to_owned(),
                    selector: raw_selector.to_owned(),
                });
            }

            Ok(FaultUseDeclaration {
                selector: selector.to_owned(),
                exclude,
                alias,
                latency_milliseconds,
            })
        })
        .collect()
}

fn valid_fault_selector(selector: &str) -> bool {
    if selector.is_empty() {
        return false;
    }
    if selector == "*" {
        return true;
    }
    if let Some(namespace) = selector.strip_suffix(".*") {
        return !namespace.is_empty() && !namespace.contains('*');
    }
    !selector.contains('*')
}

pub(crate) fn backend(block: &Block, source: &str) -> Result<BackendDeclaration> {
    labels(block, source, 2)?;
    reject_blocks(block.body(), source, "backend")?;
    reject_attributes(block.body(), source, "backend", &["description"])?;
    let kind = block.labels()[0].as_str();
    let kind = ResourceKind::from_str(kind).map_err(|()| Error::UnknownBackendKind {
        file: source.to_owned(),
        kind: kind.to_owned(),
    })?;

    Ok(BackendDeclaration {
        kind,
        name: block.labels()[1].as_str().to_owned(),
        description: optional_description(block.body(), source, "backend")?,
    })
}

pub(crate) fn behavior(block: &Block, source: &str) -> Result<BehaviorDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(
        block.body(),
        source,
        "behavior",
        &["description", "error_sets"],
    )?;
    let description = optional_description(block.body(), source, "behavior")?;
    let mut response_status = None;
    let error_sets = attribute(block.body(), "error_sets")
        .map(|value| string_array(value, source, "behavior", "error_sets"))
        .transpose()?
        .unwrap_or_default();
    let mut outcomes = Vec::new();

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
        description,
        response_status,
        error_sets,
        outcomes,
    })
}

pub(crate) fn error_set(block: &Block, source: &str) -> Result<ErrorSetDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(block.body(), source, "error_set", &["description"])?;
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
        description: optional_description(block.body(), source, "error_set")?,
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
        &["description", "status", "content_type", "body"],
    )?;
    let description = optional_description(block.body(), source, "outcome")?;
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
        description,
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
        &["description", "statuses", "content_type", "body"],
    )?;
    let description = optional_description(block.body(), source, "protocol_error")?;
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
        description,
        statuses,
        content_type,
        body,
    })
}

pub(crate) fn scenario(block: &Block, source: &str) -> Result<ScenarioDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(block.body(), source, "scenario", &["description"])?;
    let mut selections = Vec::new();
    let mut faults = Vec::new();
    for child in block.body().blocks() {
        labels(child, source, 1)?;
        reject_blocks(child.body(), source, child.identifier())?;
        match child.identifier() {
            "select" => {
                reject_attributes(child.body(), source, "select", &["outcome"])?;
                selections.push(ScenarioSelectionDeclaration {
                    operation_id: child.labels()[0].as_str().to_owned(),
                    outcome: required_string(child.body(), source, "select", "outcome")?,
                });
            }
            "inject" => {
                reject_attributes(child.body(), source, "inject", &["fault"])?;
                faults.push(ScenarioFaultDeclaration {
                    operation_id: child.labels()[0].as_str().to_owned(),
                    fault: required_string(child.body(), source, "inject", "fault")?,
                });
            }
            unknown => {
                return Err(Error::UnknownNestedBlock {
                    file: source.to_owned(),
                    block: "scenario".to_owned(),
                    nested: unknown.to_owned(),
                });
            }
        }
    }
    Ok(ScenarioDeclaration {
        name: block.labels()[0].as_str().to_owned(),
        description: optional_description(block.body(), source, "scenario")?,
        selections,
        faults,
    })
}

pub(crate) fn fault(block: &Block, source: &str) -> Result<FaultDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(block.body(), source, "fault", &["description", "phase"])?;
    let phase = match required_string(block.body(), source, "fault", "phase")?.as_str() {
        "before_response" => FaultPhase::BeforeResponse,
        "after_commit" => FaultPhase::AfterCommit,
        _ => {
            return Err(Error::InvalidAttribute {
                file: source.to_owned(),
                block: "fault".to_owned(),
                attribute: "phase".to_owned(),
                expected: "`before_response` or `after_commit`",
            });
        }
    };
    let actions = block.body().blocks().collect::<Vec<_>>();
    if actions.len() != 1 {
        return Err(Error::FaultActionCount {
            file: source.to_owned(),
            fault: block.labels()[0].as_str().to_owned(),
        });
    }
    let action = match actions[0].identifier() {
        "delay" => {
            labels(actions[0], source, 0)?;
            reject_blocks(actions[0].body(), source, "delay")?;
            reject_attributes(actions[0].body(), source, "delay", &["milliseconds"])?;
            FaultAction::Delay {
                milliseconds: required_u64(
                    actions[0].body(),
                    source,
                    "delay",
                    "milliseconds",
                    1,
                    86_400_000,
                )?,
            }
        }
        "disconnect" => {
            labels(actions[0], source, 0)?;
            empty_body(actions[0].body(), source, "disconnect")?;
            FaultAction::Disconnect
        }
        unknown => {
            return Err(Error::UnknownNestedBlock {
                file: source.to_owned(),
                block: "fault".to_owned(),
                nested: unknown.to_owned(),
            });
        }
    };
    Ok(FaultDeclaration {
        name: block.labels()[0].as_str().to_owned(),
        description: optional_description(block.body(), source, "fault")?,
        phase,
        action,
    })
}

pub(crate) fn fault_profile(block: &Block, source: &str) -> Result<FaultProfileDeclaration> {
    labels(block, source, 1)?;
    reject_blocks(block.body(), source, "fault_profile")?;
    reject_attributes(
        block.body(),
        source,
        "fault_profile",
        &[
            "probability",
            "description",
            "faults",
            "operations",
            "max_consecutive",
            "cooldown_requests",
            "seed",
        ],
    )?;
    let probability_ppm = probability_ppm(
        attribute(block.body(), "probability").ok_or_else(|| Error::InvalidAttribute {
            file: source.to_owned(),
            block: "fault_profile".to_owned(),
            attribute: "probability".to_owned(),
            expected: "a number from 0.000001 through 1",
        })?,
        source,
        "fault_profile",
        "probability",
        false,
    )?;
    let faults = required_string_array(block.body(), source, "fault_profile", "faults")?;
    let operations = attribute(block.body(), "operations")
        .map(|value| string_array(value, source, "fault_profile", "operations"))
        .transpose()?
        .unwrap_or_default();
    let max_consecutive =
        optional_u32(block.body(), source, "fault_profile", "max_consecutive")?.unwrap_or(1);
    if max_consecutive == 0 {
        return Err(Error::InvalidAttribute {
            file: source.to_owned(),
            block: "fault_profile".to_owned(),
            attribute: "max_consecutive".to_owned(),
            expected: "a positive integer",
        });
    }
    let cooldown_requests =
        optional_u32(block.body(), source, "fault_profile", "cooldown_requests")?.unwrap_or(20);
    let seed = optional_u64(block.body(), source, "fault_profile", "seed")?;
    Ok(FaultProfileDeclaration {
        name: block.labels()[0].as_str().to_owned(),
        description: optional_description(block.body(), source, "fault_profile")?,
        probability_ppm,
        faults,
        operations,
        max_consecutive,
        cooldown_requests,
        seed,
    })
}

pub(crate) fn probability_ppm(
    attribute: &Attribute,
    source: &str,
    block: &str,
    name: &str,
    allow_zero: bool,
) -> Result<u32> {
    let minimum = if allow_zero { 0.0 } else { 0.000_001 };
    let probability = match attribute.expr() {
        Expression::Number(value) => value.as_f64(),
        _ => None,
    }
    .filter(|value| *value >= minimum && *value <= 1.0)
    .ok_or_else(|| Error::InvalidAttribute {
        file: source.to_owned(),
        block: block.to_owned(),
        attribute: name.to_owned(),
        expected: if allow_zero {
            "a number from 0 through 1"
        } else {
            "a number from 0.000001 through 1"
        },
    })?;
    Ok((probability * 1_000_000.0).round() as u32)
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

fn required_string_array(
    body: &Body,
    source: &str,
    block: &str,
    name: &'static str,
) -> Result<Vec<String>> {
    let values = attribute(body, name)
        .ok_or_else(|| Error::MissingAttribute {
            file: source.to_owned(),
            block: block.to_owned(),
            attribute: name,
        })
        .and_then(|attribute| string_array(attribute, source, block, name))?;
    if values.is_empty() {
        return Err(Error::InvalidAttribute {
            file: source.to_owned(),
            block: block.to_owned(),
            attribute: name.to_owned(),
            expected: "a non-empty array of strings",
        });
    }
    Ok(values)
}

fn required_u64(
    body: &Body,
    source: &str,
    block: &str,
    name: &'static str,
    minimum: u64,
    maximum: u64,
) -> Result<u64> {
    optional_u64(body, source, block, name)?
        .filter(|value| (*value >= minimum) && (*value <= maximum))
        .ok_or_else(|| Error::InvalidAttribute {
            file: source.to_owned(),
            block: block.to_owned(),
            attribute: name.to_owned(),
            expected: "an integer in the supported range",
        })
}

fn optional_u32(body: &Body, source: &str, block: &str, name: &str) -> Result<Option<u32>> {
    optional_u64(body, source, block, name).and_then(|value| {
        value
            .map(|value| {
                u32::try_from(value).map_err(|_| Error::InvalidAttribute {
                    file: source.to_owned(),
                    block: block.to_owned(),
                    attribute: name.to_owned(),
                    expected: "an unsigned 32-bit integer",
                })
            })
            .transpose()
    })
}

pub(crate) fn optional_u64(
    body: &Body,
    source: &str,
    block: &str,
    name: &str,
) -> Result<Option<u64>> {
    attribute(body, name)
        .map(|attribute| match attribute.expr() {
            Expression::Number(value) => value.as_u64().ok_or_else(|| Error::InvalidAttribute {
                file: source.to_owned(),
                block: block.to_owned(),
                attribute: name.to_owned(),
                expected: "an unsigned integer",
            }),
            _ => Err(Error::InvalidAttribute {
                file: source.to_owned(),
                block: block.to_owned(),
                attribute: name.to_owned(),
                expected: "an unsigned integer",
            }),
        })
        .transpose()
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

pub(crate) fn non_empty_string(
    attribute: &Attribute,
    source: &str,
    block: &str,
    name: &str,
) -> Result<String> {
    string_attribute(attribute, source, block, name).and_then(|value| {
        if value.trim().is_empty() {
            Err(Error::InvalidAttribute {
                file: source.to_owned(),
                block: block.to_owned(),
                attribute: name.to_owned(),
                expected: "a non-empty string",
            })
        } else {
            Ok(value)
        }
    })
}

pub(crate) fn duration_milliseconds(
    attribute: &Attribute,
    source: &str,
    block: &str,
    name: &str,
) -> Result<u64> {
    let value = match attribute.expr() {
        Expression::String(value) => value,
        _ => {
            return Err(invalid_duration(source, block, name));
        }
    };
    parse_duration(value, source, block, name)
}

pub(crate) fn parse_duration(value: &str, source: &str, block: &str, name: &str) -> Result<u64> {
    const MAX_MILLISECONDS: u64 = 86_400_000;
    let (number, multiplier) = if let Some(number) = value.strip_suffix("ms") {
        (number, 1)
    } else if let Some(number) = value.strip_suffix('s') {
        (number, 1_000)
    } else if let Some(number) = value.strip_suffix('m') {
        (number, 60_000)
    } else if let Some(number) = value.strip_suffix('h') {
        (number, 3_600_000)
    } else {
        return Err(invalid_duration(source, block, name));
    };
    number
        .parse::<u64>()
        .ok()
        .and_then(|number| number.checked_mul(multiplier))
        .filter(|milliseconds| (1..=MAX_MILLISECONDS).contains(milliseconds))
        .ok_or_else(|| invalid_duration(source, block, name))
}

fn invalid_duration(source: &str, block: &str, name: &str) -> Error {
    Error::InvalidAttribute {
        file: source.to_owned(),
        block: block.to_owned(),
        attribute: name.to_owned(),
        expected: "a duration from 1ms through 24h, such as `250ms`, `2s`, `1m`, or `1h`",
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
    reject_attributes(block.body(), source, "async", &["description", "uses"])?;
    let uses = attribute(block.body(), "uses")
        .map(|value| string_array(value, source, "async", "uses"))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(ResourceRef::parse)
        .collect();
    let actions = action_blocks(block.body(), source, "async")?;

    Ok(AsyncDeclaration {
        name: block.labels()[0].as_str().to_owned(),
        description: optional_description(block.body(), source, "async")?,
        uses,
        actions,
    })
}

pub(crate) fn effect(block: &Block, source: &str) -> Result<EffectDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(
        block.body(),
        source,
        "effect",
        &[
            "description",
            "operations",
            "async",
            "emit",
            "after",
            "delay",
            "to",
        ],
    )?;
    let operations = attribute(block.body(), "operations")
        .ok_or_else(|| Error::MissingAttribute {
            file: source.to_owned(),
            block: "effect".to_owned(),
            attribute: "operations",
        })
        .and_then(|value| string_array(value, source, "effect", "operations"))?;
    if operations.is_empty() {
        return Err(Error::InvalidAttribute {
            file: source.to_owned(),
            block: "effect".to_owned(),
            attribute: "operations".to_owned(),
            expected: "a non-empty array of OpenAPI operationIds",
        });
    }
    let async_operations = attribute(block.body(), "async")
        .map(|value| string_array(value, source, "effect", "async"))
        .transpose()?
        .unwrap_or_default();
    let actions = action_blocks(block.body(), source, "effect")?;
    let webhooks = webhook_emissions(block.body(), source)?;
    if actions.is_empty() && async_operations.is_empty() && webhooks.is_empty() {
        return Err(Error::EmptyEffect {
            file: source.to_owned(),
            effect: block.labels()[0].as_str().to_owned(),
        });
    }
    Ok(EffectDeclaration {
        name: block.labels()[0].as_str().to_owned(),
        description: optional_description(block.body(), source, "effect")?,
        operations,
        actions,
        async_operations,
        webhooks,
    })
}

fn action_blocks(body: &Body, source: &str, owner: &str) -> Result<Vec<ActionDeclaration>> {
    body.blocks()
        .map(|block| {
            if block.identifier() != "action" {
                return Err(Error::UnknownNestedBlock {
                    file: source.to_owned(),
                    block: owner.to_owned(),
                    nested: block.identifier().to_owned(),
                });
            }
            action(block, source)
        })
        .collect()
}

fn action(block: &Block, source: &str) -> Result<ActionDeclaration> {
    labels(block, source, 2)?;
    reject_blocks(block.body(), source, "action")?;
    let backend = block.labels()[0].as_str();
    let (kind, name) = backend
        .split_once('.')
        .ok_or_else(|| Error::InvalidActionBackend {
            file: source.to_owned(),
            backend: backend.to_owned(),
        })?;
    let resource_kind = ResourceKind::from_str(kind).map_err(|()| Error::InvalidActionBackend {
        file: source.to_owned(),
        backend: backend.to_owned(),
    })?;
    if name.is_empty() || name.contains('.') {
        return Err(Error::InvalidActionBackend {
            file: source.to_owned(),
            backend: backend.to_owned(),
        });
    }
    let command = block.labels()[1].as_str();
    let kind =
        EffectActionKind::from_resource_and_name(resource_kind, command).ok_or_else(|| {
            Error::UnknownEffectAction {
                file: source.to_owned(),
                resource_kind,
                action: command.to_owned(),
            }
        })?;
    let mut arguments = std::collections::BTreeMap::new();
    for attribute in block.body().attributes() {
        if !kind.accepts_argument(attribute.key()) {
            return Err(Error::UnknownEffectArgument {
                file: source.to_owned(),
                backend: backend.to_owned(),
                action: command.to_owned(),
                argument: attribute.key().to_owned(),
            });
        }
        arguments.insert(
            attribute.key().to_owned(),
            effect_value(attribute.expr(), source, attribute.key())?,
        );
    }
    for required in kind.required_arguments() {
        if !arguments.contains_key(*required) {
            return Err(Error::MissingEffectArgument {
                file: source.to_owned(),
                backend: backend.to_owned(),
                action: command.to_owned(),
                argument: required,
            });
        }
    }
    Ok(ActionDeclaration {
        backend: ResourceRef::new(resource_kind, name),
        kind,
        arguments,
    })
}

pub(crate) fn effect_value(
    expression: &Expression,
    source: &str,
    attribute: &str,
) -> Result<EffectValue> {
    match expression {
        Expression::Null | Expression::Bool(_) | Expression::Number(_) | Expression::String(_) => {
            literal_effect_value(expression, source, attribute)
        }
        Expression::Array(items) => Ok(EffectValue::Array {
            items: items
                .iter()
                .map(|item| effect_value(item, source, attribute))
                .collect::<Result<Vec<_>>>()?,
        }),
        Expression::Object(fields) => {
            let mut compiled = std::collections::BTreeMap::new();
            for (key, value) in fields {
                let key = match key {
                    ObjectKey::Identifier(key) => key.as_str().to_owned(),
                    ObjectKey::Expression(Expression::String(key)) => key.clone(),
                    _ => return Err(invalid_effect_value(source, attribute)),
                };
                compiled.insert(key, effect_value(value, source, attribute)?);
            }
            Ok(EffectValue::Object { fields: compiled })
        }
        Expression::Variable(variable) => {
            effect_projection(variable.as_str(), &[], source, attribute)
        }
        Expression::Traversal(traversal) => {
            let Expression::Variable(variable) = &traversal.expr else {
                return Err(invalid_effect_value(source, attribute));
            };
            effect_projection(variable.as_str(), &traversal.operators, source, attribute)
        }
        Expression::Parenthesis(inner) => effect_value(inner, source, attribute),
        _ => Err(invalid_effect_value(source, attribute)),
    }
}

fn literal_effect_value(
    expression: &Expression,
    source: &str,
    attribute: &str,
) -> Result<EffectValue> {
    let value = expression
        .clone()
        .evaluate(&Context::new())
        .ok()
        .and_then(|value| hcl::from_value::<serde_json::Value>(value).ok())
        .ok_or_else(|| invalid_effect_value(source, attribute))?;
    Ok(EffectValue::Literal { value })
}

fn effect_projection(
    root: &str,
    operators: &[TraversalOperator],
    source: &str,
    attribute: &str,
) -> Result<EffectValue> {
    let mut pointer = String::new();
    for operator in operators {
        let segment = match operator {
            TraversalOperator::GetAttr(name) => name.as_str().to_owned(),
            TraversalOperator::Index(Expression::String(value)) => value.clone(),
            TraversalOperator::Index(Expression::Number(value)) => value.to_string(),
            TraversalOperator::LegacyIndex(value) => value.to_string(),
            TraversalOperator::AttrSplat
            | TraversalOperator::FullSplat
            | TraversalOperator::Index(_) => return Err(invalid_effect_value(source, attribute)),
        };
        pointer.push('/');
        pointer.push_str(&segment.replace('~', "~0").replace('/', "~1"));
    }
    match root {
        "request" => Ok(EffectValue::Request { pointer }),
        "response" => Ok(EffectValue::Response { pointer }),
        _ => Err(invalid_effect_value(source, attribute)),
    }
}

fn invalid_effect_value(source: &str, attribute: &str) -> Error {
    Error::InvalidEffectValue {
        file: source.to_owned(),
        attribute: attribute.to_owned(),
    }
}

pub(crate) fn optional_description(
    body: &Body,
    source: &str,
    block: &str,
) -> Result<Option<String>> {
    attribute(body, "description")
        .map(|attribute| non_empty_string(attribute, source, block, "description"))
        .transpose()
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
