//! `taskloom tasks set`: a task's text and/or status in one request, built
//! from the two commands that already exist ([`TaskService::edit`], then
//! [`TaskService::set_status`]), each under a request id derived from the
//! one given (`<id>:text`, `<id>:status`). Not atomic: a failure between
//! the two leaves the text changed and the status not; replayed under the
//! same request id, the step already done is answered as a duplicate and
//! the other is applied.

use crate::{AppError, StatusChange, TaskService, TextEdit, Written};

/// A task's text, status and trigger set together; at least one of them.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TaskSet {
    pub project: Option<String>,
    pub harp_id: String,
    /// The new text, whole.
    pub text: Option<String>,
    /// The new status.
    pub status: Option<String>,
    /// A revive trigger, as [`StatusChange::trigger`].
    pub trigger: Option<String>,
    /// Clear the trigger, as [`StatusChange::clear_trigger`].
    pub clear_trigger: bool,
    /// The request's key: the text and status steps send `<key>:text` and
    /// `<key>:status`. Given blank, refused ([`AppError::Blank`]) before
    /// anything is sent.
    pub request_id: Option<String>,
}

/// The step's request id, derived from the request's.
fn derived(request_id: &Option<String>, step: &str) -> Option<String> {
    request_id.as_ref().map(|key| format!("{key}:{step}"))
}

/// Set `change` on the task, the text first: the task as the last step
/// left it, with every step's dead letters.
pub async fn set<S: TaskService>(service: &mut S, change: TaskSet) -> Result<Written, AppError> {
    if change
        .request_id
        .as_deref()
        .is_some_and(|k| k.trim().is_empty())
    {
        return Err(AppError::Blank {
            field: "request_id",
        });
    }
    if change.trigger.is_some() && change.clear_trigger {
        return Err(AppError::TriggerAndClear);
    }
    let moves = change.status.is_some() || change.trigger.is_some() || change.clear_trigger;
    if change.text.is_none() && !moves {
        return Err(AppError::Blank { field: "text" });
    }
    let mut dead_letters = Vec::new();
    let mut last = None;
    if let Some(text) = change.text {
        let edit = TextEdit {
            project: change.project.clone(),
            harp_id: change.harp_id.clone(),
            text,
            request_id: derived(&change.request_id, "text"),
        };
        let mut written = service.edit(edit).await?;
        dead_letters.append(&mut written.dead_letters);
        last = Some(written);
    }
    if moves {
        let status = StatusChange {
            project: change.project,
            harp_id: change.harp_id,
            status: change.status,
            trigger: change.trigger,
            clear_trigger: change.clear_trigger,
            request_id: derived(&change.request_id, "status"),
        };
        let mut written = service.set_status(status).await?;
        dead_letters.append(&mut written.dead_letters);
        last = Some(written);
    }
    let mut written = last.expect("a text edit or a status change was made");
    written.dead_letters = dead_letters;
    Ok(written)
}
