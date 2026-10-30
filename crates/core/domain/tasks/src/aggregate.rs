//! The task aggregate on angzarr-embedded's markup (step 2 of
//! taskloom-on-angzarr-embedded.plan.md): thin handlers over the rules in
//! `lib.rs`, which the annals domain ([`crate::Tasks`]) decides by too,
//! until the rest of the workspace leaves annals (step 3).
//!
//! One task is one aggregate, as before: its stream's root is derived from
//! its harp id. The proposed shape:
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
//! - one `#[applies]` per event, assigning what the event carries;
//! - no snapshot: a task's stream is short, and every load replays it.

use angzarr_embedded::next::markup::{Context, Failed};
use angzarr_embedded_macros::{aggregate, applies, handles_cmd};

use crate::{
    retagged, status_changed, task_added, task_removed, text_edited, AddTask, EditText, RemoveTask,
    Retag, Retagged, SetStatus, StatusChanged, TaskAdded, TaskEvent, TaskRejection, TaskRemoved,
    TaskState, Tasks, TextEdited,
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

/// `event`, folded into the task as the annals domain folds it: the same
/// assignments, one place.
fn fold(task: &mut TaskState, event: TaskEvent) {
    <Tasks as annals::next::Domain>::apply(task, &event, &());
}

// The domain is TASK_DOMAIN, spelled as the markup takes it, a literal
// (`aggregate.test.rs` holds them to each other).
#[aggregate(domain = "tasks")]
impl TaskAggregate {
    /// Create the task: refused on a stream with history (the harp id is
    /// taken), recorded at `ctx.recorded_at()`.
    #[handles_cmd(AddTask)]
    fn add_task(&self, cmd: AddTask, ctx: &Context) -> Result<TaskAdded, Failed<TaskRejection>> {
        let root = ctx
            .command
            .cover
            .as_ref()
            .and_then(|cover| cover.root.as_ref());
        let root = root.map(|root| &root.value[..]);
        let taken = ctx.command.had_prior_events;
        Ok(task_added(
            &self.task,
            taken,
            &cmd,
            root,
            ctx.recorded_at(),
        )?)
    }

    /// Change the status, the trigger, or both; `None` when it changes
    /// nothing.
    #[handles_cmd(SetStatus)]
    fn set_status(
        &self,
        cmd: SetStatus,
        ctx: &Context,
    ) -> Result<Option<StatusChanged>, Failed<TaskRejection>> {
        Ok(status_changed(&self.task, &cmd, ctx.recorded_at())?)
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
    fn task_added(&mut self, event: TaskAdded) {
        fold(&mut self.task, event.into());
    }

    #[applies(StatusChanged)]
    fn status_changed(&mut self, event: StatusChanged) {
        fold(&mut self.task, event.into());
    }

    #[applies(TextEdited)]
    fn text_edited(&mut self, event: TextEdited) {
        fold(&mut self.task, event.into());
    }

    #[applies(Retagged)]
    fn retagged(&mut self, event: Retagged) {
        fold(&mut self.task, event.into());
    }

    #[applies(TaskRemoved)]
    fn task_removed(&mut self, event: TaskRemoved) {
        fold(&mut self.task, event.into());
    }
}

#[cfg(test)]
#[path = "aggregate.test.rs"]
mod tests;
