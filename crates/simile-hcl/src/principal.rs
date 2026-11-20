use std::collections::BTreeMap;

use hcl::Block;
use simile_spec::{
    ApiContract, BehaviorSpec, DescriptionEntry, DescriptionKind, Principal, PrincipalCredential,
};

use crate::ast::{PrincipalCredentialDeclaration, PrincipalDeclaration};
use crate::parse::{
    attribute, labels, optional_description, reject_attributes, reject_blocks, string_array,
};
use crate::{Error, Result};

pub(crate) fn compile(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = PrincipalDeclaration>,
    api: &ApiContract,
) -> Result<()> {
    for declaration in declarations {
        let mut credentials = BTreeMap::new();
        for declared in declaration.credentials {
            if !api.security_schemes.contains_key(&declared.scheme) {
                return Err(Error::UnknownPrincipalSecurityScheme {
                    principal: declaration.name.clone(),
                    scheme: declared.scheme,
                });
            }
            if credentials
                .insert(
                    declared.scheme.clone(),
                    PrincipalCredential {
                        grants: declared.grants.into_iter().collect(),
                    },
                )
                .is_some()
            {
                return Err(Error::DuplicatePrincipalCredential {
                    principal: declaration.name.clone(),
                    scheme: declared.scheme,
                });
            }
        }
        let principal = Principal {
            name: declaration.name.clone(),
            credentials,
        };
        if let Some(text) = declaration.description {
            spec.descriptions.insert(DescriptionEntry {
                kind: DescriptionKind::Principal,
                parent: None,
                name: declaration.name.clone(),
                text,
            });
        }
        if spec
            .principals
            .insert(declaration.name.clone(), principal)
            .is_some()
        {
            return Err(Error::DuplicatePrincipal {
                name: declaration.name,
            });
        }
    }
    Ok(())
}

pub(crate) fn principal(block: &Block, source: &str) -> Result<PrincipalDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(block.body(), source, "principal", &["description"])?;
    let name = block.labels()[0].as_str().trim();
    if name.is_empty() {
        return Err(invalid(
            source,
            "principal",
            "label",
            "a non-empty principal name",
        ));
    }
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
    Ok(PrincipalDeclaration {
        name: name.to_owned(),
        description: optional_description(block.body(), source, "principal")?,
        credentials,
    })
}

fn parse_credential(block: &Block, source: &str) -> Result<PrincipalCredentialDeclaration> {
    labels(block, source, 1)?;
    reject_blocks(block.body(), source, "credential")?;
    reject_attributes(block.body(), source, "credential", &["grants"])?;
    let scheme = block.labels()[0].as_str().trim();
    if scheme.is_empty() {
        return Err(invalid(
            source,
            "credential",
            "label",
            "a non-empty API security scheme name",
        ));
    }
    Ok(PrincipalCredentialDeclaration {
        scheme: scheme.to_owned(),
        grants: attribute(block.body(), "grants")
            .map(|value| string_array(value, source, "credential", "grants"))
            .transpose()?
            .unwrap_or_default(),
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
