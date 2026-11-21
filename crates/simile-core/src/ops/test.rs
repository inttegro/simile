use std::collections::BTreeMap;
use std::ffi::OsString;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

use super::{PreparedService, StartRequest, prepare_service, profile_environment};
use crate::{Context, Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestRequest {
    pub service: Option<String>,
    pub config: Option<PathBuf>,
    pub profile: Option<String>,
    pub port: Option<u16>,
    pub scenario: Option<String>,
    pub fault_profile: Option<String>,
    pub command: Vec<OsString>,
}

/// Everything required to bind a service and supervise one caller process.
///
/// The plan has no serialization or debug implementation because its environment may contain
/// issued profile secrets. Presentation adapters may report the service and profile names only.
pub struct TestPlan {
    pub service: PreparedService,
    pub state_path: PathBuf,
    pub profile: Option<String>,
    pub scenario: Option<String>,
    pub fault_profile: Option<String>,
    pub command: Vec<OsString>,
    pub environment: BTreeMap<String, String>,
}

pub fn prepare_test(ctx: &Context, request: TestRequest) -> Result<TestPlan> {
    if request.command.is_empty() {
        return Err(Error::TestCommandMissing);
    }
    if request.profile.is_some() && (request.service.is_none() || request.config.is_some()) {
        return Err(Error::TestProfileRequiresRegistration);
    }

    let profile = match (request.service.as_deref(), request.config.as_ref()) {
        (Some(service), None) => profile_environment(ctx, service, request.profile.as_deref())?,
        _ => None,
    };
    let service = prepare_service(
        ctx,
        &StartRequest {
            service: request.service,
            config: request.config,
            host: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            port: Some(request.port.unwrap_or(0)),
        },
    )?;
    let state_path = super::service_state_path(ctx, &service.report.service)?;
    let profile_name = profile.as_ref().map(|profile| profile.profile.clone());
    let environment = profile.map_or_else(BTreeMap::new, |profile| profile.variables);

    Ok(TestPlan {
        service,
        state_path,
        profile: profile_name,
        scenario: request.scenario,
        fault_profile: request.fault_profile,
        command: request.command,
        environment,
    })
}
