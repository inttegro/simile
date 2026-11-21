use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use simile_service::{Service, ServiceProfile, ServiceProfileSummary};
use simile_spec::PrincipalSetup;

use super::service_state_path;
use crate::registry::{Registry, validate_service_name};
use crate::{Context, Error, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct SetupProfileRequest {
    pub service: String,
    pub principal: Option<String>,
    pub profile: Option<String>,
    pub values: BTreeMap<String, Value>,
    pub select: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupProfileReport {
    pub service: String,
    pub profile: ServiceProfileSummary,
    pub created: bool,
    pub selected: bool,
    pub credentials_file: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesReport {
    pub service: String,
    pub profiles: Vec<ServiceProfileSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileReport {
    pub service: String,
    pub profile: ServiceProfileSummary,
    pub credentials_file: PathBuf,
}

/// Environment supplied to a caller running against one service profile.
///
/// This value intentionally has no serialization implementation: issued values may contain
/// secrets and belong only in the child process environment.
#[derive(Clone, PartialEq)]
pub(crate) struct ProfileEnvironment {
    pub(crate) profile: String,
    pub(crate) variables: BTreeMap<String, String>,
}

pub fn setup_contract(
    ctx: &Context,
    service: &str,
    principal: Option<&str>,
) -> Result<PrincipalSetup> {
    validate_service_name(service)?;
    let registered = Registry::new(ctx.simile_home()?).load(service)?;
    if let Some(principal) = principal {
        return registered
            .artifact
            .principal_setup(principal)
            .ok_or_else(|| Error::PrincipalSetupNotDeclared {
                service: service.to_owned(),
                principal: principal.to_owned(),
            });
    }
    let mut setups = registered.artifact.principal_setups();
    let Some(first) = setups.next() else {
        return Err(Error::SetupNotDeclared {
            service: service.to_owned(),
        });
    };
    let Some(second) = setups.next() else {
        return Ok(first);
    };
    let mut principals = vec![first.principal, second.principal];
    principals.extend(setups.map(|setup| setup.principal));
    Err(Error::PrincipalSetupAmbiguous {
        service: service.to_owned(),
        principals,
    })
}

pub fn setup_profile(ctx: &Context, request: &SetupProfileRequest) -> Result<SetupProfileReport> {
    let (service, listen) = open_service(ctx, &request.service)?;
    let result = service.setup_profile(
        request.principal.as_deref(),
        request.profile.as_deref(),
        request.values.clone(),
        request.select,
    )?;
    let profile_name = result.profile.name.clone();
    let credentials_file = credentials_path(ctx, &request.service, &profile_name)?;
    write_credentials(&credentials_file, &request.service, &result.profile, listen)?;
    let summary =
        service
            .profile_summary(&profile_name)?
            .ok_or_else(|| Error::ProfileNotFound {
                service: request.service.clone(),
                profile: profile_name,
            })?;
    Ok(SetupProfileReport {
        service: request.service.clone(),
        profile: summary,
        created: result.created,
        selected: result.selected,
        credentials_file,
    })
}

pub fn profiles(ctx: &Context, service: &str) -> Result<ProfilesReport> {
    let (runtime, _) = open_service(ctx, service)?;
    Ok(ProfilesReport {
        service: service.to_owned(),
        profiles: runtime.profiles()?,
    })
}

pub fn profile(ctx: &Context, service: &str, name: &str) -> Result<ProfileReport> {
    let (runtime, _) = open_service(ctx, service)?;
    let profile = runtime
        .profile_summary(name)?
        .ok_or_else(|| Error::ProfileNotFound {
            service: service.to_owned(),
            profile: name.to_owned(),
        })?;
    Ok(ProfileReport {
        service: service.to_owned(),
        profile,
        credentials_file: credentials_path(ctx, service, name)?,
    })
}

pub fn use_profile(ctx: &Context, service: &str, name: &str) -> Result<ProfileReport> {
    let (runtime, _) = open_service(ctx, service)?;
    let profile = runtime.select_profile(name)?;
    Ok(ProfileReport {
        service: service.to_owned(),
        profile,
        credentials_file: credentials_path(ctx, service, name)?,
    })
}

/// Resolve an explicit profile, or the service's current profile when `name` is absent.
///
/// The returned values are suitable for a supervised caller process. They must never be printed
/// or included in a machine-readable command report.
pub(crate) fn profile_environment(
    ctx: &Context,
    service: &str,
    name: Option<&str>,
) -> Result<Option<ProfileEnvironment>> {
    let (runtime, listen) = open_service(ctx, service)?;
    let profile = match name {
        Some(name) => Some(
            runtime
                .profile(name)?
                .ok_or_else(|| Error::ProfileNotFound {
                    service: service.to_owned(),
                    profile: name.to_owned(),
                })?,
        ),
        None => runtime.current_profile()?,
    };
    Ok(profile.map(|profile| ProfileEnvironment {
        profile: profile.name.clone(),
        variables: caller_environment(service, &profile, listen),
    }))
}

fn open_service(ctx: &Context, service: &str) -> Result<(Service, SocketAddr)> {
    validate_service_name(service)?;
    let registered = Registry::new(ctx.simile_home()?).load(service)?;
    let listen = SocketAddr::new(
        registered.manifest.listen.host,
        registered.manifest.listen.port,
    );
    let runtime = Service::durable(registered.artifact, service_state_path(ctx, service)?)?;
    Ok((runtime, listen))
}

fn credentials_path(ctx: &Context, service: &str, profile: &str) -> Result<PathBuf> {
    validate_service_name(service)?;
    simile_service::validate_profile_name(profile)?;
    Ok(ctx
        .simile_home()?
        .join("profiles")
        .join(service)
        .join(format!("{profile}.env")))
}

fn write_credentials(
    path: &Path,
    service: &str,
    profile: &ServiceProfile,
    listen: SocketAddr,
) -> Result<()> {
    let parent = path.parent().ok_or_else(|| Error::ProfileCredentialsIo {
        action: "resolve",
        path: path.to_path_buf(),
        source: std::io::Error::other("credentials path has no parent"),
    })?;
    fs::create_dir_all(parent).map_err(|source| Error::ProfileCredentialsIo {
        action: "create directory for",
        path: parent.to_path_buf(),
        source,
    })?;
    private_directory(parent)?;
    let temporary = path.with_extension(format!("env.tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = private_file(&temporary)?;
        writeln!(file, "SIMILE_SERVICE={}", json_string(service))?;
        writeln!(file, "SIMILE_PROFILE={}", json_string(&profile.name))?;
        writeln!(file, "SIMILE_PRINCIPAL={}", json_string(&profile.principal))?;
        writeln!(
            file,
            "SIMILE_BASE_URL={}",
            json_string(&format!("http://{listen}"))
        )?;
        for (name, issued) in &profile.issued {
            let encoded = serde_json::to_string(&issued.value).map_err(std::io::Error::other)?;
            writeln!(file, "{}={encoded}", environment_name(name))?;
        }
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok::<(), std::io::Error>(())
    })();
    if let Err(source) = result {
        drop(fs::remove_file(&temporary));
        return Err(Error::ProfileCredentialsIo {
            action: "write",
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

fn caller_environment(
    service: &str,
    profile: &ServiceProfile,
    listen: SocketAddr,
) -> BTreeMap<String, String> {
    let mut variables = BTreeMap::from([
        ("SIMILE_SERVICE".to_owned(), service.to_owned()),
        ("SIMILE_PROFILE".to_owned(), profile.name.clone()),
        ("SIMILE_PRINCIPAL".to_owned(), profile.principal.clone()),
        ("SIMILE_BASE_URL".to_owned(), format!("http://{listen}")),
    ]);
    variables.extend(profile.issued.iter().map(|(name, issued)| {
        let value = match &issued.value {
            Value::String(value) => value.clone(),
            value => value.to_string(),
        };
        (environment_name(name), value)
    }));
    variables
}

fn json_string(value: &str) -> String {
    format!("{value:?}")
}

fn environment_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(unix)]
fn private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        Error::ProfileCredentialsIo {
            action: "secure",
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn private_directory(_path: &Path) -> Result<()> {
    Ok(())
}

fn private_file(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}
