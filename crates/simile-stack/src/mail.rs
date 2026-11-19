use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{CommandContext, ErrorCode, LogicalTime, ResourceError, Result};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAttachment {
    pub name: String,
    pub content_type: String,
    pub object_ref: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MailState {
    Accepted,
    Delivered,
    Bounced,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailMessage {
    pub id: String,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<MailAttachment>,
    pub state: MailState,
    pub accepted_at: LogicalTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<LogicalTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "action"
)]
pub enum MailCommand {
    Send {
        from: String,
        to: Vec<String>,
        subject: String,
        text: String,
        #[serde(default)]
        html: Option<String>,
        #[serde(default)]
        attachments: Vec<MailAttachment>,
    },
    Get {
        id: String,
    },
    List {
        #[serde(default = "default_list_limit")]
        limit: usize,
    },
    Deliver {
        id: String,
    },
    Bounce {
        id: String,
        reason: String,
    },
    Reject {
        id: String,
        reason: String,
    },
}

const fn default_list_limit() -> usize {
    100
}

impl MailCommand {
    #[must_use]
    pub const fn action(&self) -> &'static str {
        match self {
            Self::Send { .. } => "send",
            Self::Get { .. } => "get",
            Self::List { .. } => "list",
            Self::Deliver { .. } => "deliver",
            Self::Bounce { .. } => "bounce",
            Self::Reject { .. } => "reject",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result", content = "value")]
pub enum MailResult {
    Message(Option<MailMessage>),
    Messages(Vec<MailMessage>),
    Accepted(MailMessage),
    Updated(MailMessage),
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct MailEngine {
    pub(crate) resources: BTreeMap<String, BTreeMap<String, MailMessage>>,
    pub(crate) next_message: u64,
}

impl MailEngine {
    pub(crate) fn execute(
        &mut self,
        resource: &str,
        context: &CommandContext,
        command: MailCommand,
    ) -> Result<MailResult> {
        match command {
            MailCommand::Send {
                from,
                to,
                subject,
                text,
                html,
                attachments,
            } => {
                if from.is_empty() || to.is_empty() || to.iter().any(String::is_empty) {
                    return Err(ResourceError::new(
                        ErrorCode::InvalidValue,
                        "mail requires a sender and at least one non-empty recipient",
                    ));
                }
                self.next_message = self.next_message.saturating_add(1);
                let message = MailMessage {
                    id: format!("mail_{:016x}", self.next_message),
                    from,
                    to,
                    subject,
                    text,
                    html,
                    attachments,
                    state: MailState::Accepted,
                    accepted_at: context.at,
                    completed_at: None,
                    reason: None,
                };
                self.resources
                    .entry(resource.to_owned())
                    .or_default()
                    .insert(message.id.clone(), message.clone());
                Ok(MailResult::Accepted(message))
            }
            MailCommand::Get { id } => {
                Ok(MailResult::Message(self.message(resource, &id).cloned()))
            }
            MailCommand::List { limit } => Ok(MailResult::Messages(
                self.resources
                    .get(resource)
                    .into_iter()
                    .flat_map(|messages| messages.values())
                    .take(limit)
                    .cloned()
                    .collect(),
            )),
            MailCommand::Deliver { id } => {
                self.transition(resource, &id, context.at, MailState::Delivered, None)
            }
            MailCommand::Bounce { id, reason } => {
                self.transition(resource, &id, context.at, MailState::Bounced, Some(reason))
            }
            MailCommand::Reject { id, reason } => {
                self.transition(resource, &id, context.at, MailState::Rejected, Some(reason))
            }
        }
    }

    fn message(&self, resource: &str, id: &str) -> Option<&MailMessage> {
        self.resources.get(resource)?.get(id)
    }

    fn transition(
        &mut self,
        resource: &str,
        id: &str,
        at: LogicalTime,
        state: MailState,
        reason: Option<String>,
    ) -> Result<MailResult> {
        let message = self
            .resources
            .get_mut(resource)
            .and_then(|messages| messages.get_mut(id))
            .ok_or_else(|| ResourceError::new(ErrorCode::NotFound, "mail message not found"))?;
        if message.state != MailState::Accepted {
            return Err(ResourceError::new(
                ErrorCode::Conflict,
                "a completed mail message cannot transition again",
            ));
        }
        message.state = state;
        message.completed_at = Some(at);
        message.reason = reason;
        Ok(MailResult::Updated(message.clone()))
    }
}
