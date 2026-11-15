#![forbid(unsafe_code)]

mod adapters;
mod error;
mod normalize;

use semver::Version;
use serde_json::Value;
use simile_spec::{ApiContract, OpenApiFeature, OpenApiVersion};

pub use error::{Error, Result};

/// Parse a JSON or YAML OpenAPI document into Simile's version-neutral contract.
pub fn parse(source: &str) -> Result<ApiContract> {
    let document = parse_document(source)?;
    let exact = document
        .get("openapi")
        .and_then(Value::as_str)
        .ok_or(Error::MissingVersion)?;
    let version = Version::parse(exact).map_err(|source| Error::InvalidVersion {
        value: exact.to_owned(),
        source,
    })?;
    let feature = match (version.major, version.minor) {
        (3, 0) => OpenApiFeature::V3_0,
        (3, 1) => OpenApiFeature::V3_1,
        (3, 2) => OpenApiFeature::V3_2,
        _ => {
            return Err(Error::UnsupportedVersion {
                value: exact.to_owned(),
            });
        }
    };
    let version = OpenApiVersion {
        exact: exact.to_owned(),
        feature,
    };

    match feature {
        OpenApiFeature::V3_0 => adapters::v3_0::parse(&document, version),
        OpenApiFeature::V3_1 => adapters::v3_1::parse(&document, version),
        OpenApiFeature::V3_2 => adapters::v3_2::parse(&document, version),
    }
}

fn parse_document(source: &str) -> Result<Value> {
    match serde_json::from_str(source) {
        Ok(document) => Ok(document),
        Err(json) => serde_norway::from_str(source).map_err(|yaml| Error::Document { json, yaml }),
    }
}
