use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use simile_artifact::{Artifact, ArtifactCredential, ArtifactPrincipal};
use simile_spec::{ParameterLocation, PrincipalStatus, SecurityScheme};

use crate::{Error, IncomingParameters, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedPrincipal {
    name: String,
}

impl AuthenticatedPrincipal {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Authenticate one operation using its effective OpenAPI security requirements.
///
/// Each requirement is an alternative. Schemes inside a requirement must be
/// satisfied by one active test principal, which keeps multi-scheme identities
/// coherent and makes scope checks predictable.
pub fn authenticate(
    artifact: &Artifact,
    operation: &str,
    incoming: &IncomingParameters,
) -> Result<Option<AuthenticatedPrincipal>> {
    let operation = artifact
        .operation(operation)
        .ok_or_else(|| Error::UnknownOperation {
            operation: operation.to_owned(),
        })?;
    let requirements = operation.security_requirements().collect::<Vec<_>>();
    if requirements.is_empty() {
        return Ok(None);
    }

    let principals = artifact.principals().collect::<Vec<_>>();
    let mut permission_denied = false;
    for requirement in requirements {
        let schemes = requirement
            .schemes()
            .map(|(name, scopes)| {
                let scheme = artifact.security_scheme(name).ok_or_else(|| {
                    Error::AuthenticationConfiguration {
                        message: format!("operation references unknown security scheme `{name}`"),
                    }
                })?;
                Ok((name, scheme.scheme(), scopes.collect::<Vec<_>>()))
            })
            .collect::<Result<Vec<_>>>()?;
        if schemes.is_empty() {
            return Ok(None);
        }
        let Some(presented) = schemes
            .iter()
            .map(|(_, scheme, _)| presented_credential(scheme, incoming))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };

        for principal in &principals {
            if !principal_matches(principal, &schemes, &presented) {
                continue;
            }
            if principal.status() != PrincipalStatus::Active {
                continue;
            }
            let grants = schemes
                .iter()
                .flat_map(|(_, _, scopes)| scopes)
                .all(|scope| principal.grants(scope));
            if grants {
                return Ok(Some(AuthenticatedPrincipal {
                    name: principal.name().to_owned(),
                }));
            }
            permission_denied = true;
        }
    }

    if permission_denied {
        Err(Error::PermissionDenied {
            operation: operation.key().to_owned(),
        })
    } else {
        Err(Error::AuthenticationRequired {
            operation: operation.key().to_owned(),
        })
    }
}

#[derive(Clone, Debug)]
enum PresentedCredential {
    Secret(String),
    Basic { username: String, password: String },
}

fn presented_credential(
    scheme: &SecurityScheme,
    incoming: &IncomingParameters,
) -> Option<PresentedCredential> {
    match scheme {
        SecurityScheme::ApiKey { location, name } => incoming
            .first(*location, name)
            .map(|value| PresentedCredential::Secret(value.to_owned())),
        SecurityScheme::Http { scheme, .. } if scheme.eq_ignore_ascii_case("basic") => {
            let value = authorization(incoming, "Basic")?;
            let decoded = STANDARD.decode(value).ok()?;
            let decoded = String::from_utf8(decoded).ok()?;
            let (username, password) = decoded.split_once(':')?;
            Some(PresentedCredential::Basic {
                username: username.to_owned(),
                password: password.to_owned(),
            })
        }
        SecurityScheme::Http { scheme, .. } if scheme.eq_ignore_ascii_case("bearer") => {
            authorization(incoming, "Bearer")
                .map(|value| PresentedCredential::Secret(value.to_owned()))
        }
        SecurityScheme::Http { scheme, .. } => authorization(incoming, scheme)
            .map(|value| PresentedCredential::Secret(value.to_owned())),
        SecurityScheme::OAuth2 | SecurityScheme::OpenIdConnect => authorization(incoming, "Bearer")
            .map(|value| PresentedCredential::Secret(value.to_owned())),
        SecurityScheme::MutualTls => None,
    }
}

fn authorization<'a>(incoming: &'a IncomingParameters, scheme: &str) -> Option<&'a str> {
    let value = incoming.first(ParameterLocation::Header, "authorization")?;
    let (presented_scheme, credential) = value.split_once(' ')?;
    presented_scheme
        .eq_ignore_ascii_case(scheme)
        .then_some(credential.trim())
        .filter(|credential| !credential.is_empty())
}

fn principal_matches(
    principal: &ArtifactPrincipal<'_>,
    schemes: &[(&str, SecurityScheme, Vec<&str>)],
    presented: &[PresentedCredential],
) -> bool {
    schemes
        .iter()
        .zip(presented)
        .all(|((name, _, _), presented)| {
            let Some(expected) = principal.credential(name) else {
                return false;
            };
            match (expected, presented) {
                (ArtifactCredential::Secret(expected), PresentedCredential::Secret(actual)) => {
                    equal(expected.as_bytes(), actual.as_bytes())
                }
                (
                    ArtifactCredential::Basic { username, password },
                    PresentedCredential::Basic {
                        username: actual_username,
                        password: actual_password,
                    },
                ) => {
                    equal(username.as_bytes(), actual_username.as_bytes())
                        && equal(password.as_bytes(), actual_password.as_bytes())
                }
                _ => false,
            }
        })
}

fn equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}
