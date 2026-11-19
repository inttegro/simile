use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CommandContext, ErrorCode, LogicalTime, ResourceError, Result};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Fired,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: String,
    pub due_at: LogicalTime,
    pub payload: Value,
    pub state: TaskState,
    pub created_at: LogicalTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<LogicalTime>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "action"
)]
pub enum SchedulerCommand {
    Schedule {
        #[serde(default)]
        id: Option<String>,
        due_at: LogicalTime,
        payload: Value,
    },
    Cancel {
        id: String,
    },
    Get {
        id: String,
    },
    List {
        #[serde(default = "default_list_limit")]
        limit: usize,
    },
    RunDue {
        #[serde(default = "default_list_limit")]
        limit: usize,
    },
}

const fn default_list_limit() -> usize {
    100
}

impl SchedulerCommand {
    #[must_use]
    pub const fn action(&self) -> &'static str {
        match self {
            Self::Schedule { .. } => "schedule",
            Self::Cancel { .. } => "cancel",
            Self::Get { .. } => "get",
            Self::List { .. } => "list",
            Self::RunDue { .. } => "run_due",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result", content = "value")]
pub enum SchedulerResult {
    Scheduled(ScheduledTask),
    Task(Option<ScheduledTask>),
    Tasks(Vec<ScheduledTask>),
    Cancelled(bool),
    Fired(Vec<ScheduledTask>),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct SchedulerEngine {
    resources: BTreeMap<String, BTreeMap<String, ScheduledTask>>,
    next_task: u64,
}

impl SchedulerEngine {
    pub(crate) fn execute(
        &mut self,
        resource: &str,
        context: &CommandContext,
        command: SchedulerCommand,
    ) -> Result<SchedulerResult> {
        match command {
            SchedulerCommand::Schedule {
                id,
                due_at,
                payload,
            } => {
                if due_at < context.at {
                    return Err(ResourceError::new(
                        ErrorCode::InvalidValue,
                        "a task cannot be scheduled in the past",
                    ));
                }
                self.next_task = self.next_task.saturating_add(1);
                let id = id.unwrap_or_else(|| format!("task_{:016x}", self.next_task));
                if id.is_empty()
                    || self
                        .resources
                        .get(resource)
                        .is_some_and(|tasks| tasks.contains_key(&id))
                {
                    return Err(ResourceError::new(
                        ErrorCode::Conflict,
                        "the scheduled task identity is empty or already exists",
                    ));
                }
                let task = ScheduledTask {
                    id: id.clone(),
                    due_at,
                    payload,
                    state: TaskState::Pending,
                    created_at: context.at,
                    completed_at: None,
                };
                self.resources
                    .entry(resource.to_owned())
                    .or_default()
                    .insert(id, task.clone());
                Ok(SchedulerResult::Scheduled(task))
            }
            SchedulerCommand::Cancel { id } => {
                let Some(task) = self
                    .resources
                    .get_mut(resource)
                    .and_then(|tasks| tasks.get_mut(&id))
                else {
                    return Ok(SchedulerResult::Cancelled(false));
                };
                if task.state != TaskState::Pending {
                    return Err(ResourceError::new(
                        ErrorCode::Conflict,
                        "only a pending task can be cancelled",
                    ));
                }
                task.state = TaskState::Cancelled;
                task.completed_at = Some(context.at);
                Ok(SchedulerResult::Cancelled(true))
            }
            SchedulerCommand::Get { id } => Ok(SchedulerResult::Task(
                self.resources
                    .get(resource)
                    .and_then(|tasks| tasks.get(&id))
                    .cloned(),
            )),
            SchedulerCommand::List { limit } => Ok(SchedulerResult::Tasks(
                self.resources
                    .get(resource)
                    .into_iter()
                    .flat_map(|tasks| tasks.values())
                    .take(limit)
                    .cloned()
                    .collect(),
            )),
            SchedulerCommand::RunDue { limit } => Ok(SchedulerResult::Fired(
                self.run_due(resource, context.at, limit),
            )),
        }
    }

    pub(crate) fn run_all_due(
        &mut self,
        now: LogicalTime,
        limit_per_resource: usize,
    ) -> BTreeMap<String, Vec<ScheduledTask>> {
        let names = self.resources.keys().cloned().collect::<Vec<_>>();
        names
            .into_iter()
            .filter_map(|name| {
                let fired = self.run_due(&name, now, limit_per_resource);
                (!fired.is_empty()).then_some((name, fired))
            })
            .collect()
    }

    fn run_due(&mut self, resource: &str, now: LogicalTime, limit: usize) -> Vec<ScheduledTask> {
        let Some(tasks) = self.resources.get_mut(resource) else {
            return Vec::new();
        };
        let mut due = tasks
            .values()
            .filter(|task| task.state == TaskState::Pending && task.due_at <= now)
            .map(|task| (task.due_at, task.id.clone()))
            .collect::<Vec<_>>();
        due.sort();
        let mut fired = Vec::new();
        for (_, id) in due.into_iter().take(limit) {
            if let Some(task) = tasks.get_mut(&id) {
                task.state = TaskState::Fired;
                task.completed_at = Some(now);
                fired.push(task.clone());
            }
        }
        fired
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{SchedulerCommand, SchedulerEngine, SchedulerResult};
    use crate::{CommandContext, CommandId, CommandMetadata, LogicalTime};

    fn context(at: i64) -> CommandContext {
        CommandContext {
            command_id: CommandId(1),
            at: LogicalTime::from_unix_millis(at),
            metadata: CommandMetadata::default(),
        }
    }

    #[test]
    fn due_tasks_follow_logical_time() {
        let mut engine = SchedulerEngine::default();
        let scheduled = engine.execute(
            "jobs",
            &context(0),
            SchedulerCommand::Schedule {
                id: Some("one".to_owned()),
                due_at: LogicalTime::from_unix_millis(10),
                payload: json!({"job": "invoice"}),
            },
        );
        assert!(matches!(scheduled, Ok(SchedulerResult::Scheduled(_))));
        assert!(
            engine
                .run_due("jobs", LogicalTime::from_unix_millis(9), 10)
                .is_empty()
        );
        assert_eq!(
            engine
                .run_due("jobs", LogicalTime::from_unix_millis(10), 10)
                .len(),
            1
        );
    }
}
