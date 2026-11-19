use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::{ResourceKind, ResourceRef};

/// Provenance of a compiled consequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectOrigin {
    /// A provider wrote the effect in HCL.
    ProviderAuthored,
    /// Simile derived a conservative default from the public contract.
    ConventionInferred,
}

/// A value used to build one resource command after an operation commits.
///
/// Effect values are deliberately smaller than a general-purpose expression
/// language. They can contain literals and can project public values from the
/// validated request or committed response. That keeps compiled effects
/// portable, deterministic, and safe to publish.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EffectValue {
    Literal { value: Value },
    Request { pointer: String },
    Response { pointer: String },
    Array { items: Vec<Self> },
    Object { fields: BTreeMap<String, Self> },
}

/// The typed command vocabulary available to provider-authored effects.
///
/// These are mutation-oriented commands only. Reads remain runtime concerns;
/// an effect describes consequences of a committed operation rather than an
/// alternate query API for Stack.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectActionKind {
    DbPut,
    DbDelete,
    CacheSet,
    CacheDelete,
    CacheIncrement,
    CacheCompareAndSet,
    QueuePublish,
    MailSend,
    ObjectStorePut,
    ObjectStoreDelete,
    EventBusPublish,
    SchedulerSchedule,
    SchedulerCancel,
    HttpRegister,
    HttpCall,
}

impl EffectActionKind {
    /// Resolve the compact HCL pair in `action "RESOURCE" "COMMAND"`.
    #[must_use]
    pub fn from_resource_and_name(resource: ResourceKind, name: &str) -> Option<Self> {
        Some(match (resource, name) {
            (ResourceKind::Db, "put") => Self::DbPut,
            (ResourceKind::Db, "delete") => Self::DbDelete,
            (ResourceKind::Cache, "set") => Self::CacheSet,
            (ResourceKind::Cache, "delete") => Self::CacheDelete,
            (ResourceKind::Cache, "increment") => Self::CacheIncrement,
            (ResourceKind::Cache, "compare_and_set") => Self::CacheCompareAndSet,
            (ResourceKind::Queue, "publish") => Self::QueuePublish,
            (ResourceKind::Mail, "send") => Self::MailSend,
            (ResourceKind::ObjectStore, "put") => Self::ObjectStorePut,
            (ResourceKind::ObjectStore, "delete") => Self::ObjectStoreDelete,
            (ResourceKind::EventBus, "publish") => Self::EventBusPublish,
            (ResourceKind::Scheduler, "schedule") => Self::SchedulerSchedule,
            (ResourceKind::Scheduler, "cancel") => Self::SchedulerCancel,
            (ResourceKind::Http, "register") => Self::HttpRegister,
            (ResourceKind::Http, "call") => Self::HttpCall,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn resource_kind(self) -> ResourceKind {
        match self {
            Self::DbPut | Self::DbDelete => ResourceKind::Db,
            Self::CacheSet
            | Self::CacheDelete
            | Self::CacheIncrement
            | Self::CacheCompareAndSet => ResourceKind::Cache,
            Self::QueuePublish => ResourceKind::Queue,
            Self::MailSend => ResourceKind::Mail,
            Self::ObjectStorePut | Self::ObjectStoreDelete => ResourceKind::ObjectStore,
            Self::EventBusPublish => ResourceKind::EventBus,
            Self::SchedulerSchedule | Self::SchedulerCancel => ResourceKind::Scheduler,
            Self::HttpRegister | Self::HttpCall => ResourceKind::Http,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::DbPut | Self::ObjectStorePut => "put",
            Self::DbDelete | Self::CacheDelete | Self::ObjectStoreDelete => "delete",
            Self::CacheSet => "set",
            Self::CacheIncrement => "increment",
            Self::CacheCompareAndSet => "compare_and_set",
            Self::QueuePublish | Self::EventBusPublish => "publish",
            Self::MailSend => "send",
            Self::SchedulerSchedule => "schedule",
            Self::SchedulerCancel => "cancel",
            Self::HttpRegister => "register",
            Self::HttpCall => "call",
        }
    }

    #[must_use]
    pub const fn required_arguments(self) -> &'static [&'static str] {
        match self {
            Self::DbPut => &["collection", "key", "value"],
            Self::DbDelete => &["collection", "key"],
            Self::CacheSet => &["key", "value"],
            Self::CacheDelete | Self::CacheIncrement => &["key"],
            Self::CacheCompareAndSet => &["key", "expected_version", "value"],
            Self::QueuePublish => &["body"],
            Self::MailSend => &["from", "to", "subject", "text"],
            Self::ObjectStorePut => &["key", "bytes", "content_type"],
            Self::ObjectStoreDelete => &["key"],
            Self::EventBusPublish => &["topic", "body"],
            Self::SchedulerSchedule => &["due_at", "payload"],
            Self::SchedulerCancel => &["id"],
            Self::HttpRegister => &["expectation"],
            Self::HttpCall => &["request"],
        }
    }

    #[must_use]
    pub const fn optional_arguments(self) -> &'static [&'static str] {
        match self {
            Self::DbPut | Self::DbDelete | Self::ObjectStoreDelete => &["condition"],
            Self::CacheSet => &["ttl_ms"],
            Self::CacheDelete | Self::SchedulerCancel | Self::HttpRegister | Self::HttpCall => &[],
            Self::CacheIncrement => &["by", "ttl_ms"],
            Self::CacheCompareAndSet => &["ttl_ms"],
            Self::QueuePublish => &["attributes", "delay_ms"],
            Self::MailSend => &["html", "attachments"],
            Self::ObjectStorePut => &["metadata", "condition"],
            Self::EventBusPublish => &["attributes"],
            Self::SchedulerSchedule => &["id"],
        }
    }

    #[must_use]
    pub fn accepts_argument(self, name: &str) -> bool {
        self.required_arguments().contains(&name) || self.optional_arguments().contains(&name)
    }
}

/// One typed resource mutation template.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectAction {
    pub resource: ResourceRef,
    pub kind: EffectActionKind,
    pub arguments: BTreeMap<String, EffectValue>,
}

/// A provider-authored consequence attached to one or more public operations.
///
/// Inline actions execute after the operation commits and before its response
/// is delivered. Named async operations are dispatched outside the
/// response-critical path and do not contribute to response latency.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub name: String,
    pub origin: EffectOrigin,
    pub operations: Vec<String>,
    pub actions: Vec<EffectAction>,
    pub async_operations: Vec<String>,
}
