use hcl::Block;
use hcl::expr::{Expression, TraversalOperator};
use simile_spec::{SetupBinding, SetupValueType};

use crate::ast::{PrincipalSetupDeclaration, SetupCollectDeclaration, SetupIssueDeclaration};
use crate::parse::{
    attribute, labels, non_empty_string, optional_description, reject_attributes, reject_blocks,
};
use crate::{Error, Result};

pub(crate) fn principal_setup(block: &Block, source: &str) -> Result<PrincipalSetupDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(
        block.body(),
        source,
        "principal_setup",
        &["description", "operation"],
    )?;
    let principal = non_empty_label(block, source, "principal_setup")?;
    let operation_id = attribute(block.body(), "operation")
        .ok_or_else(|| Error::MissingAttribute {
            file: source.to_owned(),
            block: "principal_setup".to_owned(),
            attribute: "operation",
        })
        .and_then(|value| non_empty_string(value, source, "principal_setup", "operation"))?;
    let mut collects = Vec::new();
    let mut issues = Vec::new();
    for nested in block.body().blocks() {
        match nested.identifier() {
            "collect" => collects.push(collect(nested, source)?),
            "issue" => issues.push(issue(nested, source)?),
            unknown => {
                return Err(Error::UnknownNestedBlock {
                    file: source.to_owned(),
                    block: "principal_setup".to_owned(),
                    nested: unknown.to_owned(),
                });
            }
        }
    }
    if issues.is_empty() {
        return Err(invalid(
            source,
            "principal_setup",
            "issue",
            "at least one issued value",
        ));
    }
    Ok(PrincipalSetupDeclaration {
        principal,
        description: optional_description(block.body(), source, "principal_setup")?,
        operation_id,
        collects,
        issues,
    })
}

fn collect(block: &Block, source: &str) -> Result<SetupCollectDeclaration> {
    labels(block, source, 1)?;
    reject_blocks(block.body(), source, "collect")?;
    reject_attributes(
        block.body(),
        source,
        "collect",
        &["description", "required", "type", "to", "bind"],
    )?;
    let name = non_empty_label(block, source, "collect")?;
    let required = boolean(block, source, "collect", "required")?.unwrap_or(false);
    let value_type = optional_string(block, source, "collect", "type")?.map_or(
        Ok(SetupValueType::String),
        |value| match value.as_str() {
            "string" => Ok(SetupValueType::String),
            "url" => Ok(SetupValueType::Url),
            "boolean" => Ok(SetupValueType::Boolean),
            "integer" => Ok(SetupValueType::Integer),
            "json" => Ok(SetupValueType::Json),
            _ => Err(invalid(
                source,
                "collect",
                "type",
                "`string`, `url`, `boolean`, `integer`, or `json`",
            )),
        },
    )?;
    let request_pointer = attribute(block.body(), "to")
        .map(|value| projection(value.expr(), "request", source, "collect", "to"))
        .transpose()?
        .unwrap_or_else(|| pointer(&[name.as_str()]));
    let binding = attribute(block.body(), "bind")
        .map(|value| binding(value.expr(), source, "collect"))
        .transpose()?;
    Ok(SetupCollectDeclaration {
        name,
        description: optional_description(block.body(), source, "collect")?,
        required,
        value_type,
        request_pointer,
        binding,
    })
}

fn issue(block: &Block, source: &str) -> Result<SetupIssueDeclaration> {
    labels(block, source, 1)?;
    reject_blocks(block.body(), source, "issue")?;
    reject_attributes(
        block.body(),
        source,
        "issue",
        &[
            "description",
            "from",
            "generate",
            "prefix",
            "credential",
            "sensitive",
            "bind",
        ],
    )?;
    let name = non_empty_label(block, source, "issue")?;
    let response_pointer = attribute(block.body(), "from")
        .map(|value| projection(value.expr(), "response", source, "issue", "from"))
        .transpose()?;
    let generate = optional_string(block, source, "issue", "generate")?;
    if generate.as_deref().is_some_and(|value| value != "token") {
        return Err(invalid(source, "issue", "generate", "`token`"));
    }
    if response_pointer.is_some() == generate.is_some() {
        return Err(Error::ExclusiveAttributes {
            file: source.to_owned(),
            block: "issue".to_owned(),
            attributes: "`from` or `generate`",
        });
    }
    let prefix = optional_string(block, source, "issue", "prefix")?;
    if prefix.is_some() && generate.is_none() {
        return Err(invalid(
            source,
            "issue",
            "prefix",
            "used only with `generate = \"token\"`",
        ));
    }
    let binding = attribute(block.body(), "bind")
        .map(|value| binding(value.expr(), source, "issue"))
        .transpose()?;
    Ok(SetupIssueDeclaration {
        name,
        description: optional_description(block.body(), source, "issue")?,
        response_pointer,
        token_prefix: generate.map(|_| prefix.unwrap_or_default()),
        credential: optional_string(block, source, "issue", "credential")?,
        sensitive: boolean(block, source, "issue", "sensitive")?.unwrap_or(false),
        binding,
    })
}

fn boolean(block: &Block, source: &str, owner: &str, name: &str) -> Result<Option<bool>> {
    attribute(block.body(), name)
        .map(|attribute| match attribute.expr() {
            Expression::Bool(value) => Ok(*value),
            _ => Err(invalid(source, owner, name, "a boolean")),
        })
        .transpose()
}

fn optional_string(block: &Block, source: &str, owner: &str, name: &str) -> Result<Option<String>> {
    attribute(block.body(), name)
        .map(|value| non_empty_string(value, source, owner, name))
        .transpose()
}

fn non_empty_label(block: &Block, source: &str, owner: &str) -> Result<String> {
    let name = block.labels()[0].as_str().trim();
    if name.is_empty() {
        Err(invalid(source, owner, "label", "a non-empty name"))
    } else {
        Ok(name.to_owned())
    }
}

fn projection(
    expression: &Expression,
    root: &str,
    source: &str,
    block: &str,
    attribute: &str,
) -> Result<String> {
    let Expression::Traversal(traversal) = expression else {
        return Err(invalid(source, block, attribute, "a field projection"));
    };
    let Expression::Variable(variable) = &traversal.expr else {
        return Err(invalid(source, block, attribute, "a field projection"));
    };
    if variable.as_str() != root || traversal.operators.is_empty() {
        return Err(invalid(source, block, attribute, "a field projection"));
    }
    let segments = traversal
        .operators
        .iter()
        .map(|operator| match operator {
            TraversalOperator::GetAttr(name) => Ok(name.as_str().to_owned()),
            TraversalOperator::Index(Expression::String(value)) => Ok(value.clone()),
            TraversalOperator::Index(Expression::Number(value)) => Ok(value.to_string()),
            TraversalOperator::LegacyIndex(value) => Ok(value.to_string()),
            _ => Err(invalid(source, block, attribute, "a field projection")),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(pointer(
        &segments.iter().map(String::as_str).collect::<Vec<_>>(),
    ))
}

fn binding(expression: &Expression, source: &str, block: &str) -> Result<SetupBinding> {
    let Expression::Traversal(traversal) = expression else {
        return Err(invalid(source, block, "bind", "a supported setup binding"));
    };
    let Expression::Variable(root) = &traversal.expr else {
        return Err(invalid(source, block, "bind", "a supported setup binding"));
    };
    let path = traversal
        .operators
        .iter()
        .map(|operator| match operator {
            TraversalOperator::GetAttr(name) => Some(name.as_str()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    match (root.as_str(), path.as_deref()) {
        ("webhook", Some(["target"])) => Ok(SetupBinding::WebhookTarget),
        ("webhook", Some(["signing_secret"])) => Ok(SetupBinding::WebhookSigningSecret),
        _ => Err(invalid(
            source,
            block,
            "bind",
            "`webhook.target` or `webhook.signing_secret`",
        )),
    }
}

fn pointer(segments: &[&str]) -> String {
    segments.iter().fold(String::new(), |mut pointer, segment| {
        pointer.push('/');
        pointer.push_str(&segment.replace('~', "~0").replace('/', "~1"));
        pointer
    })
}

fn invalid(source: &str, block: &str, attribute: &str, expected: &'static str) -> Error {
    Error::InvalidAttribute {
        file: source.to_owned(),
        block: block.to_owned(),
        attribute: attribute.to_owned(),
        expected,
    }
}
