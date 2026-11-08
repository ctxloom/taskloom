//! The task aggregate on angzarr-embedded's markup: thin handlers over the
//! rules in `lib.rs`.
//!
//! One task is one aggregate: its stream's root is derived from its harp
//! id.
//!
//! - the instance holds the folded [`TaskState`], read by [`TaskAggregate::state`];
//! - one `#[handles_cmd]` per command, each `fn(&self, cmd, ctx: &Context)
//!   -> Result<_, TaskRejection>`, `ctx.command` the router's whole
//!   `CommandContext` (its cover's root, whether the stream has history),
//!   `ctx.recorded_at()` the instant its events are recorded at: no clock;
//! - a status change and a retag return `Option`: a change that changes
//!   nothing records nothing;
//! - every refusal is a typed refusal message of the protos, listed in the
//!   command's `refuses(..)` and refused under the status its message
//!   declares (`refusal.rs`); the rules raise a [`crate::TaskRejection`], a
//!   key (`code!(...)`, under the crate's name) and its args, never English,
//!   which the application reads back from the refusal's type. A stream with
//!   history refuses an add ([`crate::TaskRejection::TaskExists`]), by the
//!   router's `had_prior_events`;
//! - one `#[applies]` per event, assigning what the event carries, an add's
//!   and a status change's instant, and when it deferred the task, its
//!   page's `recorded_at` (or an old event's `at` and `deferred_since`): at
//!   decide time the kernel hands the decision's own
//!   events the metadata it stamps them with, so the task decided is the
//!   task replayed;
//! - no snapshot: a task's stream is short, and every load replays it.

use angzarr_embedded::markup::{Context, Failed, PageContext, Refuses};
use angzarr_embedded_macros::{aggregate, applies, handles_cmd};

use crate::refusal::refuse;
use crate::{
    retagged, status_changed, task_added, task_removed, text_edited, AddTask, Blank, EditText,
    HarpIdRequired, NoTask, RemoveTask, Removed, Retag, Retagged, SetStatus, StatusChanged,
    StatusRequired, StatusReserved, TagRequired, TaskAdded, TaskCommand, TaskEvent, TaskExists,
    TaskRejection, TaskRemoved, TaskState, TextEdited, TextRequired, TriggerRequired,
    TriggerWithStatus, Unresolved,
};

/// The task aggregate: one task, folded from its stream.
#[derive(Clone, Debug, Default)]
pub struct TaskAggregate {
    task: TaskState,
}

impl TaskAggregate {
    /// The folded task.
    pub fn state(&self) -> &TaskState {
        &self.task
    }
}

/// `event`, folded into the task at its page's `recorded_at`
/// ([`crate::apply`]). Every page the kernel hands an applier is stamped,
/// the decision's own events' included; one with no metadata is folded at
/// the epoch.
fn fold(task: &mut TaskState, event: TaskEvent, page: &PageContext) {
    let recorded_at = page.meta.and_then(|meta| meta.recorded_at);
    crate::apply(task, &event, recorded_at.unwrap_or_default());
}

// The domain is TASK_DOMAIN, spelled as the markup takes it, a literal
// (`aggregate.test.rs` holds them to each other).
#[aggregate(domain = "tasks", command = TaskCommand, event = TaskEvent)]
impl TaskAggregate {
    /// Create the task, in the stream its harp id derives (`rooted_by`: an
    /// add sent to another stream is refused before this runs): refused on
    /// a stream with history (the harp id is taken), recorded at
    /// `ctx.recorded_at()`.
    #[handles_cmd(AddTask, rooted_by = harp_id, refuses(TaskExists, HarpIdRequired, TextRequired, Unresolved, Blank, StatusReserved, TriggerWithStatus, TriggerRequired))]
    fn add_task(
        &self,
        cmd: AddTask,
        ctx: &Context,
    ) -> Result<TaskAdded, Failed<Refuses<Self, AddTask>>> {
        let taken = ctx.command.had_prior_events;
        task_added(&self.task, taken, &cmd).map_err(|rejection| {
            refuse!(
                TaskRejection,
                rejection,
                Refuses<Self, AddTask>,
                [TaskExists{harp_id} => TaskExists, HarpIdRequired => HarpIdRequired, TextRequired => TextRequired, Unresolved{field} => Unresolved, Blank{field} => Blank, StatusReserved{status} => StatusReserved, TriggerWithStatus{status, defers} => TriggerWithStatus, TriggerRequired => TriggerRequired]
            )
        })
    }

    /// Change the status, the trigger, or both; `None` when it changes
    /// nothing.
    #[handles_cmd(
        SetStatus,
        refuses(
            NoTask,
            Removed,
            Unresolved,
            Blank,
            StatusReserved,
            StatusRequired,
            TriggerWithStatus,
            TriggerRequired
        )
    )]
    fn set_status(
        &self,
        cmd: SetStatus,
        ctx: &Context,
    ) -> Result<Option<StatusChanged>, Failed<Refuses<Self, SetStatus>>> {
        let _ = ctx;
        status_changed(&self.task, &cmd).map_err(|rejection| {
            refuse!(
                TaskRejection,
                rejection,
                Refuses<Self, SetStatus>,
                [NoTask => NoTask, Removed{harp_id} => Removed, Unresolved{field} => Unresolved, Blank{field} => Blank, StatusReserved{status} => StatusReserved, StatusRequired => StatusRequired, TriggerWithStatus{status, defers} => TriggerWithStatus, TriggerRequired => TriggerRequired]
            )
        })
    }

    /// Replace the text whole.
    #[handles_cmd(EditText, refuses(NoTask, Removed, TextRequired))]
    fn edit_text(
        &self,
        cmd: EditText,
        ctx: &Context,
    ) -> Result<TextEdited, Failed<Refuses<Self, EditText>>> {
        let _ = ctx;
        text_edited(&self.task, &cmd).map_err(|rejection| {
            refuse!(
                TaskRejection,
                rejection,
                Refuses<Self, EditText>,
                [NoTask => NoTask, Removed{harp_id} => Removed, TextRequired => TextRequired]
            )
        })
    }

    /// Add and remove tags; `None` when the tags held do not change.
    #[handles_cmd(Retag, refuses(NoTask, Removed, TagRequired))]
    fn retag(
        &self,
        cmd: Retag,
        ctx: &Context,
    ) -> Result<Option<Retagged>, Failed<Refuses<Self, Retag>>> {
        let _ = ctx;
        retagged(&self.task, &cmd).map_err(|rejection| {
            refuse!(
                TaskRejection,
                rejection,
                Refuses<Self, Retag>,
                [NoTask => NoTask, Removed{harp_id} => Removed, TagRequired => TagRequired]
            )
        })
    }

    /// Remove the task, keeping its stream and its harp id.
    #[handles_cmd(RemoveTask, refuses(NoTask, Removed))]
    fn remove_task(
        &self,
        cmd: RemoveTask,
        ctx: &Context,
    ) -> Result<TaskRemoved, Failed<Refuses<Self, RemoveTask>>> {
        let _ = ctx;
        task_removed(&self.task, &cmd).map_err(|rejection| {
            refuse!(
                TaskRejection,
                rejection,
                Refuses<Self, RemoveTask>,
                [NoTask => NoTask, Removed{harp_id} => Removed]
            )
        })
    }

    #[applies(TaskAdded)]
    fn task_added(&mut self, event: TaskAdded, page: &PageContext) {
        fold(&mut self.task, event.into(), page);
    }

    #[applies(StatusChanged)]
    fn status_changed(&mut self, event: StatusChanged, page: &PageContext) {
        fold(&mut self.task, event.into(), page);
    }

    #[applies(TextEdited)]
    fn text_edited(&mut self, event: TextEdited, page: &PageContext) {
        fold(&mut self.task, event.into(), page);
    }

    #[applies(Retagged)]
    fn retagged(&mut self, event: Retagged, page: &PageContext) {
        fold(&mut self.task, event.into(), page);
    }

    #[applies(TaskRemoved)]
    fn task_removed(&mut self, event: TaskRemoved, page: &PageContext) {
        fold(&mut self.task, event.into(), page);
    }
}

#[cfg(test)]
#[path = "aggregate.test.rs"]
mod tests;
