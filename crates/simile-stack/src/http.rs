use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CommandContext, ErrorCode, LogicalTime, ResourceError, Result};

pub type HeaderMap = BTreeMap<String, String>;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: HeaderMap,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    pub status: u16,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: HeaderMap,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpExpectation {
    pub id: String,
    pub method: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub required_headers: HeaderMap,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    pub response: HttpResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRequest {
    pub sequence: u64,
    pub at: LogicalTime,
    pub request: HttpRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_expectation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "action"
)]
pub enum HttpCommand {
    Register {
        expectation: HttpExpectation,
    },
    Call {
        request: HttpRequest,
    },
    Requests {
        #[serde(default = "default_list_limit")]
        limit: usize,
    },
    Clear,
}

const fn default_list_limit() -> usize {
    100
}

impl HttpCommand {
    #[must_use]
    pub const fn action(&self) -> &'static str {
        match self {
            Self::Register { .. } => "register",
            Self::Call { .. } => "call",
            Self::Requests { .. } => "requests",
            Self::Clear => "clear",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result", content = "value")]
pub enum HttpResult {
    Registered(HttpExpectation),
    Response(HttpResponse),
    Requests(Vec<RecordedRequest>),
    Cleared,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct HttpState {
    expectations: Vec<HttpExpectation>,
    requests: Vec<RecordedRequest>,
    next_request: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct HttpEngine {
    resources: BTreeMap<String, HttpState>,
}

impl HttpEngine {
    pub(crate) fn execute(
        &mut self,
        resource: &str,
        context: &CommandContext,
        command: HttpCommand,
    ) -> Result<HttpResult> {
        let state = self.resources.entry(resource.to_owned()).or_default();
        match command {
            HttpCommand::Register { mut expectation } => {
                expectation.method = normalize_method(&expectation.method)?;
                expectation.required_headers = normalize_headers(expectation.required_headers);
                if expectation.id.is_empty() || expectation.url.is_empty() {
                    return Err(ResourceError::new(
                        ErrorCode::InvalidValue,
                        "an HTTP expectation requires a non-empty id and URL",
                    ));
                }
                if state
                    .expectations
                    .iter()
                    .any(|existing| existing.id == expectation.id)
                {
                    return Err(ResourceError::new(
                        ErrorCode::Conflict,
                        "an HTTP expectation with this id already exists",
                    ));
                }
                state.expectations.push(expectation.clone());
                Ok(HttpResult::Registered(expectation))
            }
            HttpCommand::Call { mut request } => {
                request.method = normalize_method(&request.method)?;
                request.headers = normalize_headers(request.headers);
                let index = state
                    .expectations
                    .iter()
                    .position(|expectation| matches_request(expectation, &request));
                let (matched_expectation, response) = if let Some(index) = index {
                    let expectation = &mut state.expectations[index];
                    let response = expectation.response.clone();
                    let matched = Some(expectation.id.clone());
                    if let Some(remaining) = expectation.remaining.as_mut() {
                        *remaining = remaining.saturating_sub(1);
                    }
                    (matched, response)
                } else {
                    (
                        None,
                        HttpResponse {
                            status: 501,
                            headers: HeaderMap::new(),
                            body: Some(serde_json::json!({
                                "error": {
                                    "code": "unexpected_request",
                                    "message": "no local HTTP expectation matched the request"
                                }
                            })),
                        },
                    )
                };
                state.next_request = state.next_request.saturating_add(1);
                state.requests.push(RecordedRequest {
                    sequence: state.next_request,
                    at: context.at,
                    request,
                    matched_expectation,
                });
                state
                    .expectations
                    .retain(|expectation| expectation.remaining != Some(0));
                Ok(HttpResult::Response(response))
            }
            HttpCommand::Requests { limit } => Ok(HttpResult::Requests(
                state.requests.iter().rev().take(limit).cloned().collect(),
            )),
            HttpCommand::Clear => {
                *state = HttpState::default();
                Ok(HttpResult::Cleared)
            }
        }
    }
}

fn normalize_method(method: &str) -> Result<String> {
    let method = method.trim().to_ascii_uppercase();
    if method.is_empty()
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
    {
        Err(ResourceError::new(
            ErrorCode::InvalidValue,
            "HTTP methods must contain only ASCII letters or `-`",
        ))
    } else {
        Ok(method)
    }
}

fn normalize_headers(headers: HeaderMap) -> HeaderMap {
    headers
        .into_iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
        .collect()
}

fn matches_request(expectation: &HttpExpectation, request: &HttpRequest) -> bool {
    expectation.method == request.method
        && expectation.url == request.url
        && expectation
            .required_headers
            .iter()
            .all(|(name, value)| request.headers.get(name) == Some(value))
        && expectation
            .body
            .as_ref()
            .is_none_or(|body| request.body.as_ref() == Some(body))
        && expectation.remaining != Some(0)
}

#[cfg(test)]
mod tests {
    use super::{
        HeaderMap, HttpCommand, HttpEngine, HttpExpectation, HttpRequest, HttpResponse, HttpResult,
    };
    use crate::{CommandContext, CommandId, CommandMetadata, LogicalTime};

    #[test]
    fn calls_are_matched_and_recorded() {
        let mut engine = HttpEngine::default();
        let context = CommandContext {
            command_id: CommandId(1),
            at: LogicalTime::UNIX_EPOCH,
            metadata: CommandMetadata::default(),
        };
        let registered = engine.execute(
            "outbound",
            &context,
            HttpCommand::Register {
                expectation: HttpExpectation {
                    id: "tax".to_owned(),
                    method: "post".to_owned(),
                    url: "https://tax.test/calculate".to_owned(),
                    required_headers: HeaderMap::new(),
                    body: None,
                    response: HttpResponse {
                        status: 200,
                        headers: HeaderMap::new(),
                        body: None,
                    },
                    remaining: Some(1),
                },
            },
        );
        assert!(matches!(registered, Ok(HttpResult::Registered(_))));
        let called = engine.execute(
            "outbound",
            &context,
            HttpCommand::Call {
                request: HttpRequest {
                    method: "POST".to_owned(),
                    url: "https://tax.test/calculate".to_owned(),
                    headers: HeaderMap::new(),
                    body: None,
                },
            },
        );
        assert!(matches!(called, Ok(HttpResult::Response(response)) if response.status == 200));
    }
}
