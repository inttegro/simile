use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use simile_spec::{EffectActionKind, ResourceKind};

use crate::{
    BusCommand, BusResult, CacheCommand, CacheResult, DbCommand, DbResult, ErrorCode, HttpCommand,
    HttpResult, LogicalTime, MailCommand, MailResult, ObjectCommand, ObjectResult, QueueCommand,
    QueueResult, ResourceError, SchedulerCommand, SchedulerResult,
};

pub const PROTOCOL_VERSION: u16 = 1;

/// A declared resource such as `queue.orders` or `db.customers`.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceAddress {
    pub kind: ResourceKind,
    pub name: String,
}

impl ResourceAddress {
    pub fn new(kind: ResourceKind, name: impl Into<String>) -> crate::Result<Self> {
        let name = name.into();
        if name.is_empty() || name.contains('.') {
            return Err(ResourceError::new(
                ErrorCode::InvalidValue,
                "a resource name must be non-empty and cannot contain `.`",
            ));
        }
        Ok(Self { kind, name })
    }
}

impl fmt::Display for ResourceAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.kind, self.name)
    }
}

impl FromStr for ResourceAddress {
    type Err = ResourceError;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let (kind, name) = value.split_once('.').ok_or_else(|| {
            ResourceError::new(
                ErrorCode::InvalidValue,
                "a resource address must have the form `kind.name`",
            )
        })?;
        let kind = kind.parse::<ResourceKind>().map_err(|()| {
            ResourceError::new(
                ErrorCode::InvalidValue,
                format!("`{kind}` is not a supported resource kind"),
            )
        })?;
        Self::new(kind, name)
    }
}

/// Caller-supplied causal metadata for one resource command.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// A monotonically increasing identity assigned by one stack instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CommandId(pub u64);

/// Complete execution context after the supervisor has assigned time and identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandContext {
    pub command_id: CommandId,
    pub at: LogicalTime,
    pub metadata: CommandMetadata,
}

/// The exhaustive command vocabulary understood by Stack protocol version 1.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "component", content = "command", rename_all = "snake_case")]
pub enum StackCommand {
    Db(DbCommand),
    Cache(CacheCommand),
    Queue(QueueCommand),
    Mail(MailCommand),
    ObjectStore(ObjectCommand),
    EventBus(BusCommand),
    Scheduler(SchedulerCommand),
    Http(HttpCommand),
}

impl StackCommand {
    #[must_use]
    pub const fn kind(&self) -> ResourceKind {
        match self {
            Self::Db(_) => ResourceKind::Db,
            Self::Cache(_) => ResourceKind::Cache,
            Self::Queue(_) => ResourceKind::Queue,
            Self::Mail(_) => ResourceKind::Mail,
            Self::ObjectStore(_) => ResourceKind::ObjectStore,
            Self::EventBus(_) => ResourceKind::EventBus,
            Self::Scheduler(_) => ResourceKind::Scheduler,
            Self::Http(_) => ResourceKind::Http,
        }
    }

    #[must_use]
    pub const fn action(&self) -> &'static str {
        match self {
            Self::Db(command) => command.action(),
            Self::Cache(command) => command.action(),
            Self::Queue(command) => command.action(),
            Self::Mail(command) => command.action(),
            Self::ObjectStore(command) => command.action(),
            Self::EventBus(command) => command.action(),
            Self::Scheduler(command) => command.action(),
            Self::Http(command) => command.action(),
        }
    }

    /// Decode a rendered provider-authored effect into the typed Stack protocol.
    ///
    /// HCL and artifacts retain command arguments as public data templates.
    /// This adapter is the single seam that turns those values into component
    /// commands, so the operation runtime never depends on Stack internals.
    pub fn from_effect(
        kind: EffectActionKind,
        arguments: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> crate::Result<Self> {
        let mut fields = arguments
            .iter()
            .map(|(name, value)| (protocol_field_name(name), value.clone()))
            .collect::<serde_json::Map<_, _>>();
        fields.insert(
            "action".to_owned(),
            serde_json::Value::String(kind.name().to_owned()),
        );
        let value = serde_json::Value::Object(fields);
        let invalid = |error: serde_json::Error| {
            ResourceError::new(
                ErrorCode::InvalidCommand,
                format!(
                    "invalid {}.{} effect action: {error}",
                    kind.resource_kind(),
                    kind.name()
                ),
            )
        };
        Ok(match kind.resource_kind() {
            ResourceKind::Db => Self::Db(serde_json::from_value(value).map_err(invalid)?),
            ResourceKind::Cache => Self::Cache(serde_json::from_value(value).map_err(invalid)?),
            ResourceKind::Queue => Self::Queue(serde_json::from_value(value).map_err(invalid)?),
            ResourceKind::Mail => Self::Mail(serde_json::from_value(value).map_err(invalid)?),
            ResourceKind::ObjectStore => {
                Self::ObjectStore(serde_json::from_value(value).map_err(invalid)?)
            }
            ResourceKind::EventBus => {
                Self::EventBus(serde_json::from_value(value).map_err(invalid)?)
            }
            ResourceKind::Scheduler => {
                Self::Scheduler(serde_json::from_value(value).map_err(invalid)?)
            }
            ResourceKind::Http => Self::Http(serde_json::from_value(value).map_err(invalid)?),
        })
    }
}

/// Translate the provider-facing effect vocabulary into the Stack wire vocabulary.
///
/// Effect arguments are deliberately written as HCL-friendly `snake_case`, while
/// the versioned Stack protocol uses `camelCase`. Keeping that translation here
/// makes this the only seam at which the two public representations meet.
fn protocol_field_name(name: &str) -> String {
    let mut parts = name.split('_');
    let mut converted = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let mut characters = part.chars();
        if let Some(first) = characters.next() {
            converted.extend(first.to_uppercase());
            converted.extend(characters);
        }
    }
    converted
}

/// The exhaustive result vocabulary returned by Stack protocol version 1.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "component", content = "result", rename_all = "snake_case")]
pub enum StackResult {
    Db(Box<DbResult>),
    Cache(Box<CacheResult>),
    Queue(Box<QueueResult>),
    Mail(Box<MailResult>),
    ObjectStore(Box<ObjectResult>),
    EventBus(Box<BusResult>),
    Scheduler(Box<SchedulerResult>),
    Http(Box<HttpResult>),
}

/// One internal process command. The supervisor is authoritative for `context.at`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandEnvelope {
    pub protocol_version: u16,
    pub resource: ResourceAddress,
    pub context: CommandContext,
    pub command: StackCommand,
}

/// A successful command result carrying the exact logical time and identity used.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandReceipt {
    pub command_id: CommandId,
    pub at: LogicalTime,
    pub replayed: bool,
    pub result: StackResult,
}

/// Discovery metadata returned by every component process.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentManifest {
    pub protocol_version: u16,
    pub kind: ResourceKind,
    pub capabilities: Vec<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::str::FromStr;

    use serde_json::json;
    use simile_spec::{EffectActionKind, ResourceKind};

    use super::{ResourceAddress, StackCommand};
    use crate::{LogicalTime, QueueCommand, SchedulerCommand};

    #[test]
    fn resource_addresses_use_the_public_kind_dot_name_form() {
        let address = ResourceAddress::from_str("queue.orders");
        assert_eq!(
            address,
            Ok(ResourceAddress {
                kind: ResourceKind::Queue,
                name: "orders".to_owned(),
            })
        );
    }

    #[test]
    fn wire_commands_use_stable_snake_case_actions_and_camel_case_fields()
    -> Result<(), serde_json::Error> {
        let command = StackCommand::Queue(QueueCommand::Publish {
            body: json!({"orderId": "or_123"}),
            attributes: BTreeMap::new(),
            delay_ms: 250,
        });
        let value = serde_json::to_value(command)?;
        assert_eq!(value.pointer("/component"), Some(&json!("queue")));
        assert_eq!(value.pointer("/command/action"), Some(&json!("publish")));
        assert_eq!(value.pointer("/command/delayMs"), Some(&json!(250)));
        assert_eq!(value.pointer("/command/delay_ms"), None);
        Ok(())
    }

    #[test]
    fn rendered_effects_cross_one_typed_protocol_boundary() {
        let arguments = BTreeMap::from([
            ("collection".to_owned(), json!("orders")),
            ("key".to_owned(), json!("or_123")),
            ("value".to_owned(), json!({"id": "or_123"})),
        ]);
        let command = StackCommand::from_effect(EffectActionKind::DbPut, &arguments)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            command,
            StackCommand::Db(crate::DbCommand::Put { collection, key, .. })
                if collection == "orders" && key == "or_123"
        ));
    }

    #[test]
    fn rendered_effects_translate_public_snake_case_fields() {
        let arguments = BTreeMap::from([
            ("due_at".to_owned(), json!("2030-01-01T12:00:00Z")),
            ("payload".to_owned(), json!({"kind": "settlement"})),
        ]);
        let command = StackCommand::from_effect(EffectActionKind::SchedulerSchedule, &arguments)
            .unwrap_or_else(|error| panic!("{error}"));

        assert!(matches!(
            command,
            StackCommand::Scheduler(SchedulerCommand::Schedule { due_at, .. })
                if due_at == LogicalTime::from_unix_millis(1_893_499_200_000)
        ));
    }
}
