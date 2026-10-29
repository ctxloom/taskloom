//! The task aggregate on angzarr-embedded's markup: RED, step 1 of
//! taskloom-on-angzarr-embedded.plan.md. Every handler and applier is a
//! `todo!()` stub, so the tests beside it (`aggregate.test.rs`) compile and
//! fail at run time; step 2 writes the bodies over the rules in `lib.rs`.
//! The annals domain ([`crate::Tasks`]) stays, and the rest of the
//! workspace on it, until step 3.
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
//! - every refusal is message-only, a [`TaskRejection`]: a key (`tasks-*`)
//!   and its args, never English, refused as business-fatal;
//! - one `#[applies]` per event, assigning what the event carries;
//! - no snapshot: a task's stream is short, and every load replays it.

use angzarr_embedded::next::markup::Context;
use angzarr_embedded_macros::{aggregate, applies, handles_cmd};

use crate::{
    AddTask, EditText, RemoveTask, Retag, Retagged, SetStatus, StatusChanged, TaskAdded,
    TaskRejection, TaskRemoved, TaskState, TextEdited,
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

// The domain is TASK_DOMAIN, spelled as the markup takes it, a literal
// (`aggregate.test.rs` holds them to each other).
#[aggregate(domain = "tasks")]
impl TaskAggregate {
    /// Create the task: refused on a stream with history (the harp id is
    /// taken), recorded at `ctx.recorded_at()`.
    #[handles_cmd(AddTask)]
    fn add_task(&self, cmd: AddTask, ctx: &Context) -> Result<TaskAdded, TaskRejection> {
        let _ = (cmd, ctx);
        todo!("step 2: the add, over lib.rs's rules")
    }

    /// Change the status, the trigger, or both; `None` when it changes
    /// nothing.
    #[handles_cmd(SetStatus)]
    fn set_status(
        &self,
        cmd: SetStatus,
        ctx: &Context,
    ) -> Result<Option<StatusChanged>, TaskRejection> {
        let _ = (cmd, ctx);
        todo!("step 2: the status change, over lib.rs's rules")
    }

    /// Replace the text whole.
    #[handles_cmd(EditText)]
    fn edit_text(&self, cmd: EditText, ctx: &Context) -> Result<TextEdited, TaskRejection> {
        let _ = (cmd, ctx);
        todo!("step 2: the edit")
    }

    /// Add and remove tags; `None` when the tags held do not change.
    #[handles_cmd(Retag)]
    fn retag(&self, cmd: Retag, ctx: &Context) -> Result<Option<Retagged>, TaskRejection> {
        let _ = (cmd, ctx);
        todo!("step 2: the retag")
    }

    /// Remove the task, keeping its stream and its harp id.
    #[handles_cmd(RemoveTask)]
    fn remove_task(&self, cmd: RemoveTask, ctx: &Context) -> Result<TaskRemoved, TaskRejection> {
        let _ = (cmd, ctx);
        todo!("step 2: the removal")
    }

    #[applies(TaskAdded)]
    fn task_added(&mut self, event: TaskAdded) {
        let _ = event;
        todo!("step 2: apply TaskAdded")
    }

    #[applies(StatusChanged)]
    fn status_changed(&mut self, event: StatusChanged) {
        let _ = event;
        todo!("step 2: apply StatusChanged")
    }

    #[applies(TextEdited)]
    fn text_edited(&mut self, event: TextEdited) {
        let _ = event;
        todo!("step 2: apply TextEdited")
    }

    #[applies(Retagged)]
    fn retagged(&mut self, event: Retagged) {
        let _ = event;
        todo!("step 2: apply Retagged")
    }

    #[applies(TaskRemoved)]
    fn task_removed(&mut self, event: TaskRemoved) {
        let _ = event;
        todo!("step 2: apply TaskRemoved")
    }
}

#[cfg(test)]
#[path = "aggregate.test.rs"]
mod tests;
