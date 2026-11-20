use std::collections::BTreeMap;

use hcl::Block;
use hcl::expr::{Expression, TraversalOperator};
use simile_spec::{
    ApiContract, BehaviorSpec, DescriptionEntry, DescriptionKind, Profile, ProfileCredential,
    ProfileStatus, SecurityScheme,
};

use crate::ast::{ProfileCredentialDeclaration, ProfileDeclaration};
use crate::parse::{
    attribute, labels, non_empty_string, optional_description, reject_attributes, reject_blocks,
};
use crate::{Error, Result};

pub(crate) fn compile(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = ProfileDeclaration>,
    api: &ApiContract,
) -> Result<()> {
    for declaration in declarations {
        let principal = spec.principals.get(&declaration.principal).ok_or_else(|| {
            Error::UnknownProfilePrincipal {
                profile: declaration.name.clone(),
                principal: declaration.principal.clone(),
            }
        })?;
        let mut credentials = BTreeMap::new();
        for declared in declaration.credentials {
            if !principal.credentials.contains_key(&declared.scheme) {
                return Err(Error::UnknownProfileCredential {
                    profile: declaration.name.clone(),
                    principal: declaration.principal.clone(),
                    credential: declared.scheme,
                });
            }
            let scheme = &api.security_schemes[&declared.scheme];
            let compatible = match (&declared.credential, scheme) {
                (ProfileCredential::Basic { .. }, SecurityScheme::Http { scheme, .. }) => {
                    scheme.eq_ignore_ascii_case("basic")
                }
                (ProfileCredential::Secret { .. }, SecurityScheme::Http { scheme, .. }) => {
                    !scheme.eq_ignore_ascii_case("basic")
                }
                (
                    ProfileCredential::Secret { .. },
                    SecurityScheme::ApiKey { .. }
                    | SecurityScheme::OAuth2
                    | SecurityScheme::OpenIdConnect,
                ) => true,
                _ => false,
            };
            if !compatible {
                return Err(Error::InvalidProfileCredential {
                    profile: declaration.name.clone(),
                    credential: declared.scheme,
                });
            }
            if credentials
                .insert(declared.scheme.clone(), declared.credential)
                .is_some()
            {
                return Err(Error::DuplicateProfileCredential {
                    profile: declaration.name.clone(),
                    credential: declared.scheme,
                });
            }
        }
        let profile = Profile {
            name: declaration.name.clone(),
            principal: declaration.principal,
            status: declaration.status,
            credentials,
        };
        if let Some(text) = declaration.description {
            spec.descriptions.insert(DescriptionEntry {
                kind: DescriptionKind::Profile,
                parent: Some(profile.principal.clone()),
                name: profile.name.clone(),
                text,
            });
        }
        if spec
            .profiles
            .insert(profile.name.clone(), profile)
            .is_some()
        {
            return Err(Error::DuplicateProfile {
                name: declaration.name,
            });
        }
    }
    Ok(())
}

pub(crate) fn profile(block: &Block, source: &str) -> Result<ProfileDeclaration> {
    labels(block, source, 1)?;
    reject_attributes(
        block.body(),
        source,
        "profile",
        &["description", "principal", "status"],
    )?;
    let name = block.labels()[0].as_str().trim();
    if name.is_empty() {
        return Err(invalid(
            source,
            "profile",
            "label",
            "a non-empty profile name",
        ));
    }
    let principal = attribute(block.body(), "principal")
        .ok_or_else(|| Error::MissingAttribute {
            file: source.to_owned(),
            block: "profile".to_owned(),
            attribute: "principal",
        })
        .and_then(|value| principal_reference(value.expr(), source))?;
    let status = attribute(block.body(), "status")
        .map(|value| non_empty_string(value, source, "profile", "status"))
        .transpose()?
        .map_or(Ok(ProfileStatus::Active), |status| match status.as_str() {
            "active" => Ok(ProfileStatus::Active),
            "revoked" => Ok(ProfileStatus::Revoked),
            "expired" => Ok(ProfileStatus::Expired),
            _ => Err(invalid(
                source,
                "profile",
                "status",
                "`active`, `revoked`, or `expired`",
            )),
        })?;
    let credentials = block
        .body()
        .blocks()
        .map(|credential| {
            if credential.identifier() != "credential" {
                return Err(Error::UnknownNestedBlock {
                    file: source.to_owned(),
                    block: "profile".to_owned(),
                    nested: credential.identifier().to_owned(),
                });
            }
            parse_credential(credential, source)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ProfileDeclaration {
        name: name.to_owned(),
        description: optional_description(block.body(), source, "profile")?,
        principal,
        status,
        credentials,
    })
}

fn parse_credential(block: &Block, source: &str) -> Result<ProfileCredentialDeclaration> {
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
            "a non-empty principal credential name",
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
        (Some(value), None, None) => ProfileCredential::Secret { value },
        (None, Some(username), Some(password)) => ProfileCredential::Basic { username, password },
        _ => {
            return Err(invalid(
                source,
                "credential",
                "value",
                "either `value`, or both `username` and `password`",
            ));
        }
    };
    Ok(ProfileCredentialDeclaration {
        scheme: scheme.to_owned(),
        credential,
    })
}

fn principal_reference(expression: &Expression, source: &str) -> Result<String> {
    let Expression::Traversal(traversal) = expression else {
        return Err(invalid(
            source,
            "profile",
            "principal",
            "a `principal.NAME` reference",
        ));
    };
    let Expression::Variable(root) = &traversal.expr else {
        return Err(invalid(
            source,
            "profile",
            "principal",
            "a `principal.NAME` reference",
        ));
    };
    let [TraversalOperator::GetAttr(name)] = traversal.operators.as_slice() else {
        return Err(invalid(
            source,
            "profile",
            "principal",
            "a `principal.NAME` reference",
        ));
    };
    if root.as_str() != "principal" || name.as_str().trim().is_empty() {
        return Err(invalid(
            source,
            "profile",
            "principal",
            "a `principal.NAME` reference",
        ));
    }
    Ok(name.as_str().to_owned())
}

fn invalid(source: &str, block: &str, attribute: &str, expected: &'static str) -> Error {
    Error::InvalidAttribute {
        file: source.to_owned(),
        block: block.to_owned(),
        attribute: attribute.to_owned(),
        expected,
    }
}
