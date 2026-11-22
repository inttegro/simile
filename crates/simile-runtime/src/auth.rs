use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use simile_artifact::{Artifact, ArtifactCredential, ArtifactProfile};
use simile_spec::{ParameterLocation, Profile, ProfileCredential, ProfileStatus, SecurityScheme};

use crate::{Error, IncomingParameters, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedProfile {
    name: String,
    principal: String,
    dynamic: bool,
}

impl AuthenticatedProfile {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn principal(&self) -> &str {
        &self.principal
    }

    /// Whether this profile was supplied by the running service rather than
    /// compiled as a static contract fixture.
    #[must_use]
    pub const fn is_dynamic(&self) -> bool {
        self.dynamic
    }
}

/// Authenticate one operation using its effective OpenAPI security requirements.
///
/// Each requirement is an alternative. Schemes inside a requirement must be
/// satisfied by one active profile, which keeps multi-scheme identities
/// coherent and makes scope checks predictable.
pub fn authenticate(
    artifact: &Artifact,
    operation: &str,
    incoming: &IncomingParameters,
) -> Result<Option<AuthenticatedProfile>> {
    authenticate_with_profiles(artifact, operation, incoming, &[])
}

/// Authenticate against compiled fixtures plus runtime-issued service profiles.
pub fn authenticate_with_profiles(
    artifact: &Artifact,
    operation: &str,
    incoming: &IncomingParameters,
    dynamic_profiles: &[Profile],
) -> Result<Option<AuthenticatedProfile>> {
    let operation = artifact
        .operation(operation)
        .ok_or_else(|| Error::UnknownOperation {
            operation: operation.to_owned(),
        })?;
    let requirements = operation.security_requirements().collect::<Vec<_>>();
    if requirements.is_empty() {
        return Ok(None);
    }

    let profiles = artifact.profiles().collect::<Vec<_>>();
    let anonymous_allowed = requirements
        .iter()
        .any(|requirement| requirement.schemes().next().is_none());
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
            // Why: optional authentication must remain anonymous when no credential matches, but
            // a valid presented identity must win over the empty OpenAPI alternative so scoped
            // state and authenticated response behavior remain reachable.
            continue;
        }
        let Some(presented) = schemes
            .iter()
            .map(|(_, scheme, _)| presented_credential(scheme, incoming))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };

        for profile in &profiles {
            if !profile_matches(profile, &schemes, &presented) {
                continue;
            }
            if profile.status() != ProfileStatus::Active {
                continue;
            }
            let principal = artifact.principal(profile.principal()).ok_or_else(|| {
                Error::AuthenticationConfiguration {
                    message: format!(
                        "profile `{}` references unknown principal `{}`",
                        profile.name(),
                        profile.principal()
                    ),
                }
            })?;
            let grants = schemes.iter().all(|(scheme, _, scopes)| {
                scopes.iter().all(|scope| principal.grants(scheme, scope))
            });
            if grants {
                return Ok(Some(AuthenticatedProfile {
                    name: profile.name().to_owned(),
                    principal: profile.principal().to_owned(),
                    dynamic: false,
                }));
            }
            permission_denied = true;
        }
        for profile in dynamic_profiles {
            if !dynamic_profile_matches(profile, &schemes, &presented) {
                continue;
            }
            if profile.status != ProfileStatus::Active {
                continue;
            }
            let principal = artifact.principal(&profile.principal).ok_or_else(|| {
                Error::AuthenticationConfiguration {
                    message: format!(
                        "profile `{}` references unknown principal `{}`",
                        profile.name, profile.principal
                    ),
                }
            })?;
            let grants = schemes.iter().all(|(scheme, _, scopes)| {
                scopes.iter().all(|scope| principal.grants(scheme, scope))
            });
            if grants {
                return Ok(Some(AuthenticatedProfile {
                    name: profile.name.clone(),
                    principal: profile.principal.clone(),
                    dynamic: true,
                }));
            }
            permission_denied = true;
        }
    }

    if anonymous_allowed {
        Ok(None)
    } else if permission_denied {
        Err(Error::PermissionDenied {
            operation: operation.key().to_owned(),
        })
    } else {
        Err(Error::AuthenticationRequired {
            operation: operation.key().to_owned(),
        })
    }
}

fn dynamic_profile_matches(
    profile: &Profile,
    schemes: &[(&str, SecurityScheme, Vec<&str>)],
    presented: &[PresentedCredential],
) -> bool {
    schemes
        .iter()
        .zip(presented)
        .all(|((name, _, _), presented)| {
            let Some(expected) = profile.credentials.get(*name) else {
                return false;
            };
            match (expected, presented) {
                (ProfileCredential::Secret { value }, PresentedCredential::Secret(actual)) => {
                    equal(value.as_bytes(), actual.as_bytes())
                }
                (
                    ProfileCredential::Basic { username, password },
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

fn profile_matches(
    profile: &ArtifactProfile<'_>,
    schemes: &[(&str, SecurityScheme, Vec<&str>)],
    presented: &[PresentedCredential],
) -> bool {
    schemes
        .iter()
        .zip(presented)
        .all(|((name, _, _), presented)| {
            let Some(expected) = profile.credential(name) else {
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
