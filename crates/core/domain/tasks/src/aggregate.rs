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
//! - every refusal is message-only, a [`TaskRejection`] as a
//!   `Failed::Rejected` (FAILED_PRECONDITION): a key (`tasks-*`) and its
//!   args, never English, business-fatal. A stream with history refuses an
//!   add (`tasks-task-exists`), by the router's `had_prior_events`;
//! - one `#[applies]` per event, assigning what the event carries, an add's
//!   and a status change's instant, and when it deferred the task, its
//!   page's `recorded_at` (or an old event's `at` and `deferred_since`): at
//!   decide time the kernel hands the decision's own
//!   events the metadata it stamps them with, so the task decided is the
//!   task replayed;
//! - no snapshot: a task's stream is short, and every load replays it.

use angzarr_embedded::markup::{Context, Failed, PageContext};
use angzarr_embedded_macros::{aggregate, applies, handles_cmd};

use crate::{
    retagged, status_changed, task_added, task_removed, text_edited, AddTask, EditText, RemoveTask,
    Retag, Retagged, SetStatus, StatusChanged, TaskAdded, TaskCommand, TaskEvent, TaskRejection,
    TaskRemoved, TaskState, TextEdited,
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
    #[handles_cmd(AddTask, rooted_by = harp_id)]
    fn add_task(&self, cmd: AddTask, ctx: &Context) -> Result<TaskAdded, Failed<TaskRejection>> {
        let taken = ctx.command.had_prior_events;
        Ok(task_added(&self.task, taken, &cmd)?)
    }

    /// Change the status, the trigger, or both; `None` when it changes
    /// nothing.
    #[handles_cmd(SetStatus)]
    fn set_status(
        &self,
        cmd: SetStatus,
        ctx: &Context,
    ) -> Result<Option<StatusChanged>, Failed<TaskRejection>> {
        let _ = ctx;
        Ok(status_changed(&self.task, &cmd)?)
    }

    /// Replace the text whole.
    #[handles_cmd(EditText)]
    fn edit_text(&self, cmd: EditText, ctx: &Context) -> Result<TextEdited, Failed<TaskRejection>> {
        let _ = ctx;
        Ok(text_edited(&self.task, &cmd)?)
    }

    /// Add and remove tags; `None` when the tags held do not change.
    #[handles_cmd(Retag)]
    fn retag(&self, cmd: Retag, ctx: &Context) -> Result<Option<Retagged>, Failed<TaskRejection>> {
        let _ = ctx;
        Ok(retagged(&self.task, &cmd)?)
    }

    /// Remove the task, keeping its stream and its harp id.
    #[handles_cmd(RemoveTask)]
    fn remove_task(
        &self,
        cmd: RemoveTask,
        ctx: &Context,
    ) -> Result<TaskRemoved, Failed<TaskRejection>> {
        let _ = ctx;
        Ok(task_removed(&self.task, &cmd)?)
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
