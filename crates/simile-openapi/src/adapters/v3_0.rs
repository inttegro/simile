use serde_json::Value;
use simile_spec::{ApiContract, OpenApiVersion};

use crate::Result;
use crate::normalize;

pub(crate) fn parse(document: &Value, version: OpenApiVersion) -> Result<ApiContract> {
    normalize::contract(document, version, false, false)
}
