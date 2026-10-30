//! The task, folded from its events, and its wire form.

use angzarr_embedded::next::{Timestamp, WireError};

use crate::Tag;

/// The tasks domain's name: every task stream's `Cover.domain`.
pub const TASK_DOMAIN: &str = "tasks";

/// The task, folded from its events. Derived values (whether the status is
/// terminal, the text hash, a priority) are computed when read, never
/// stored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskState {
    pub harp_id: String,
    pub text: String,
    pub status: String,
    pub trigger: String,
    pub origin_session: String,
    pub tags: Vec<Tag>,
    pub created_at: Timestamp,
    pub status_set_at: Timestamp,
    pub deferred_since: Option<Timestamp>,
    pub removed: bool,
    pub prior_status: String,
}

impl From<TaskState> for tasks_proto::v1::TaskState {
    fn from(value: TaskState) -> Self {
        tasks_proto::v1::TaskState {
            harp_id: value.harp_id,
            text: value.text,
            status: value.status,
            trigger: value.trigger,
            origin_session: value.origin_session,
            tags: value
                .tags
                .into_iter()
                .map(tasks_proto::v1::Tag::from)
                .collect(),
            created_at: Some(value.created_at),
            status_set_at: Some(value.status_set_at),
            deferred_since: value.deferred_since,
            removed: value.removed,
            prior_status: value.prior_status,
        }
    }
}

impl TryFrom<tasks_proto::v1::TaskState> for TaskState {
    type Error = WireError;

    fn try_from(value: tasks_proto::v1::TaskState) -> Result<Self, WireError> {
        let missing = |field| WireError::Missing {
            message: "ctxloom.tasks.v1.TaskState",
            field,
        };
        Ok(TaskState {
            harp_id: value.harp_id,
            text: value.text,
            status: value.status,
            trigger: value.trigger,
            origin_session: value.origin_session,
            tags: value
                .tags
                .into_iter()
                .map(Tag::try_from)
                .collect::<Result<Vec<_>, _>>()?,
            created_at: value.created_at.ok_or_else(|| missing("created_at"))?,
            status_set_at: value
                .status_set_at
                .ok_or_else(|| missing("status_set_at"))?,
            deferred_since: value.deferred_since,
            removed: value.removed,
            prior_status: value.prior_status,
        })
    }
}
