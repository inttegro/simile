use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::registry::SourceLocation;
use crate::{Context, Error, Result};

const MAX_REMOTE_CONFIG_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ServiceConfigSource {
    Path(PathBuf),
    Url(String),
}

impl ServiceConfigSource {
    pub(crate) fn from_argument(value: impl Into<String>) -> Self {
        let value = value.into();
        if value.starts_with("https://") || value.starts_with("http://") {
            Self::Url(value)
        } else {
            Self::Path(PathBuf::from(value))
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedServiceConfig {
    pub source: SourceLocation,
    pub name: String,
    pub contents: String,
    pub base: Option<PathBuf>,
}

impl LoadedServiceConfig {
    pub(crate) fn load(ctx: &Context, source: ServiceConfigSource) -> Result<Self> {
        match source {
            ServiceConfigSource::Path(path) => {
                let path = absolute(ctx.current_dir(), &path);
                let contents =
                    std::fs::read_to_string(&path).map_err(|source| Error::ReadSource {
                        path: path.clone(),
                        source,
                    })?;
                let base = path.parent().unwrap_or_else(|| Path::new(".")).to_owned();
                Ok(Self {
                    source: SourceLocation::Path {
                        value: path.to_string_lossy().into_owned(),
                    },
                    name: path.to_string_lossy().into_owned(),
                    contents,
                    base: Some(base),
                })
            }
            ServiceConfigSource::Url(url) => {
                let contents = fetch(&url)?;
                Ok(Self {
                    source: SourceLocation::Url { value: url.clone() },
                    name: url,
                    contents,
                    base: None,
                })
            }
        }
    }

    pub(crate) fn snapshot(source: SourceLocation, contents: String) -> Self {
        match &source {
            SourceLocation::Path { value } => {
                let path = PathBuf::from(value);
                let base = path.parent().unwrap_or_else(|| Path::new(".")).to_owned();
                Self {
                    name: value.clone(),
                    source,
                    contents,
                    base: Some(base),
                }
            }
            SourceLocation::Url { value } => Self {
                name: value.clone(),
                source,
                contents,
                base: None,
            },
        }
    }

    pub(crate) fn resolve(&self, value: &str, field: &'static str) -> Result<PathBuf> {
        let base = self.base.as_ref().ok_or_else(|| Error::RemoteConfigPath {
            config: self.name.clone(),
            field,
            value: value.to_owned(),
        })?;
        let path = Path::new(value);
        if path.is_absolute() {
            return Ok(path.to_owned());
        }
        Ok(base.join(path))
    }
}

fn fetch(url: &str) -> Result<String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build();
    let agent: ureq::Agent = config.into();
    let response = agent.get(url).call().map_err(|source| Error::FetchConfig {
        url: url.to_owned(),
        source,
    })?;
    let mut reader = response
        .into_body()
        .into_reader()
        .take(MAX_REMOTE_CONFIG_BYTES + 1);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|source| Error::ReadRemoteConfig {
            url: url.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > MAX_REMOTE_CONFIG_BYTES {
        return Err(Error::RemoteConfigTooLarge {
            url: url.to_owned(),
            limit: MAX_REMOTE_CONFIG_BYTES,
        });
    }
    String::from_utf8(bytes).map_err(|source| Error::RemoteConfigUtf8 {
        url: url.to_owned(),
        source,
    })
}

fn absolute(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    }
}
