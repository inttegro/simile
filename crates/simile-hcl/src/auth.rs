use hcl::Block;
use simile_spec::{PrincipalCredential, PrincipalStatus};

use crate::ast::{PrincipalCredentialDeclaration, PrincipalDeclaration};
use crate::parse::{
    attribute, labels, non_empty_string, optional_description, reject_attributes, reject_blocks,
    string_array,
};
use crate::{Error, Result};

pub(crate) fn principal(block: &Block, source: &str) -> Result<PrincipalDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(
        block.body(),
        source,
        "principal",
        &["description", "status", "grants"],
    )?;
    let name = block.labels()[0].as_str().trim();
    if name.is_empty() {
        return Err(invalid(
            source,
            "principal",
            "label",
            "a non-empty principal name",
        ));
    }
    let status = attribute(block.body(), "status")
        .map(|value| non_empty_string(value, source, "principal", "status"))
        .transpose()?
        .map_or(Ok(PrincipalStatus::Active), |status| {
            match status.as_str() {
                "active" => Ok(PrincipalStatus::Active),
                "revoked" => Ok(PrincipalStatus::Revoked),
                "expired" => Ok(PrincipalStatus::Expired),
                _ => Err(invalid(
                    source,
                    "principal",
                    "status",
                    "`active`, `revoked`, or `expired`",
                )),
            }
        })?;
    let grants = attribute(block.body(), "grants")
        .map(|value| string_array(value, source, "principal", "grants"))
        .transpose()?
        .unwrap_or_default();
    let credentials = block
        .body()
        .blocks()
        .map(|credential| {
            if credential.identifier() != "credential" {
                return Err(Error::UnknownNestedBlock {
                    file: source.to_owned(),
                    block: "principal".to_owned(),
                    nested: credential.identifier().to_owned(),
                });
            }
            parse_credential(credential, source)
        })
        .collect::<Result<Vec<_>>>()?;
    if credentials.is_empty() {
        return Err(invalid(
            source,
            "principal",
            "credential",
            "at least one credential block",
        ));
    }
    Ok(PrincipalDeclaration {
        name: name.to_owned(),
        description: optional_description(block.body(), source, "principal")?,
        status,
        credentials,
        grants,
    })
}

fn parse_credential(block: &Block, source: &str) -> Result<PrincipalCredentialDeclaration> {
    labels(block, source, 1)?;
    reject_blocks(block.body(), source, "credential")?;
    reject_attributes(
        block.body(),
        source,
        "credential",
        &["value", "username", "password"],
    )?;
    let scheme = block.labels()[0].as_str().trim();
    if scheme.is_empty() {
        return Err(invalid(
            source,
            "credential",
            "label",
            "a non-empty OpenAPI security scheme name",
        ));
    }
    let value = attribute(block.body(), "value")
        .map(|value| non_empty_string(value, source, "credential", "value"))
        .transpose()?;
    let username = attribute(block.body(), "username")
        .map(|value| non_empty_string(value, source, "credential", "username"))
        .transpose()?;
    let password = attribute(block.body(), "password")
        .map(|value| non_empty_string(value, source, "credential", "password"))
        .transpose()?;
    let credential = match (value, username, password) {
        (Some(value), None, None) => PrincipalCredential::Secret { value },
        (None, Some(username), Some(password)) => PrincipalCredential::Basic { username, password },
        _ => {
            return Err(invalid(
                source,
                "credential",
                "value",
                "either `value`, or both `username` and `password`",
            ));
        }
    };
    Ok(PrincipalCredentialDeclaration {
        scheme: scheme.to_owned(),
        credential,
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
