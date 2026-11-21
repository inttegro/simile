use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::Rng;
use serde::Serialize;

use crate::{Context, Error, Result};

const DEFAULT_PROXY_PORT: u16 = 8_484;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyRequest {
    pub service: String,
    pub upstream: String,
    pub learn: bool,
    pub public_url: Option<String>,
    pub webhook_destination: Option<String>,
    pub host: Option<IpAddr>,
    pub port: Option<u16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyPlan {
    pub report: ProxyReport,
    pub relay_path: Option<String>,
    pub learning_workspace: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyReport {
    pub service: String,
    pub listen: SocketAddr,
    pub upstream: String,
    pub api_base: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_relay: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_destination: Option<String>,
    pub learning: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
}

pub fn prepare_proxy(ctx: &Context, request: &ProxyRequest) -> Result<ProxyPlan> {
    crate::registry::validate_service_name(&request.service)?;
    let upstream = validate_url("upstream URL", &request.upstream, false)?;
    let public_url = request
        .public_url
        .as_deref()
        .map(|url| validate_url("public URL", url, true))
        .transpose()?;
    let webhook_destination = request
        .webhook_destination
        .as_deref()
        .map(|url| validate_url("webhook destination", url, false))
        .transpose()?;
    if webhook_destination.is_some() && public_url.is_none() {
        return Err(Error::WebhookDestinationRequiresPublicUrl);
    }

    let listen = SocketAddr::new(
        request.host.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        request.port.unwrap_or(DEFAULT_PROXY_PORT),
    );
    let relay_path = public_url
        .as_ref()
        .map(|_| format!("/.simile/webhooks/{}", request.service));
    let webhook_relay = public_url
        .as_ref()
        .zip(relay_path.as_ref())
        .map(|(url, path)| format!("{}{}", url.trim_end_matches('/'), path));
    let session = request.learn.then(new_session_id);
    let learning_workspace = match &session {
        Some(session) => Some(
            ctx.simile_home()?
                .join("learning")
                .join(&request.service)
                .join(session),
        ),
        None => None,
    };
    let api_base = format!("http://{listen}");
    Ok(ProxyPlan {
        report: ProxyReport {
            service: request.service.clone(),
            listen,
            upstream,
            api_base,
            webhook_relay,
            webhook_destination,
            learning: request.learn,
            session,
            workspace: learning_workspace.clone(),
        },
        relay_path,
        learning_workspace,
    })
}

fn validate_url(field: &'static str, value: &str, origin_only: bool) -> Result<String> {
    let uri = value
        .parse::<ureq::http::Uri>()
        .map_err(|_| Error::InvalidProxyUrl {
            field,
            value: value.to_owned(),
            reason: "expected an absolute HTTP or HTTPS URL",
        })?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.authority().is_none() {
        return Err(Error::InvalidProxyUrl {
            field,
            value: value.to_owned(),
            reason: "expected an absolute HTTP or HTTPS URL",
        });
    }
    if origin_only
        && uri
            .path_and_query()
            .is_some_and(|path| path.as_str() != "/")
    {
        return Err(Error::InvalidProxyUrl {
            field,
            value: value.to_owned(),
            reason: "expected an origin without a path or query",
        });
    }
    Ok(value.trim_end_matches('/').to_owned())
}

fn new_session_id() -> String {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let random = rand::rng().random::<u32>();
    format!("learn_{milliseconds:x}_{random:08x}")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::BuildInfo;

    fn context() -> Context {
        Context::with_simile_home(
            BuildInfo::new("0.3.0", None::<String>, "test", "test"),
            ".",
            Path::new("/tmp/simile-proxy-tests"),
        )
    }

    #[test]
    fn derives_one_proxy_origin_and_a_reserved_webhook_path() {
        let plan = prepare_proxy(
            &context(),
            &ProxyRequest {
                service: "acme".to_owned(),
                upstream: "https://api.acme.test".to_owned(),
                learn: true,
                public_url: Some("https://capture.example.test".to_owned()),
                webhook_destination: Some("http://127.0.0.1:4000/hooks/acme".to_owned()),
                host: None,
                port: None,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(plan.report.api_base, "http://127.0.0.1:8484");
        assert_eq!(
            plan.report.webhook_relay.as_deref(),
            Some("https://capture.example.test/.simile/webhooks/acme")
        );
        assert_eq!(plan.relay_path.as_deref(), Some("/.simile/webhooks/acme"));
        assert!(plan.learning_workspace.is_some());
    }

    #[test]
    fn requires_a_public_origin_for_webhook_forwarding() {
        let result = prepare_proxy(
            &context(),
            &ProxyRequest {
                service: "acme".to_owned(),
                upstream: "https://api.acme.test".to_owned(),
                learn: false,
                public_url: None,
                webhook_destination: Some("http://127.0.0.1:4000/hooks/acme".to_owned()),
                host: None,
                port: None,
            },
        );

        assert!(matches!(
            result,
            Err(Error::WebhookDestinationRequiresPublicUrl)
        ));
    }
}
