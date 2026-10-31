//! Why a command to a task was refused.

use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TaskRejection {
    /// An add without a harp id.
    HarpIdRequired,
    /// An add whose harp id is not the one its stream's root derives from.
    NotItsStream { harp_id: String },
    /// An add whose stream root cannot be derived from its harp id:
    /// `cause` says why.
    NoRoot { harp_id: String, cause: String },
    /// An add to a stream that already holds a task: the harp id is taken.
    TaskExists { harp_id: String },
    /// A command to a stream holding no task.
    NoTask,
    /// A command to a removed task.
    Removed { harp_id: String },
    /// Text that is empty once trimmed.
    TextRequired,
    /// A status change giving neither a status nor a trigger.
    StatusRequired,
    /// A status starting with the status-class sigil.
    StatusReserved { status: String },
    /// Deferred without a revive trigger, given or already held.
    TriggerRequired,
    /// A retag with no tag once trimmed.
    TagRequired,
    /// A field given blank (`field` names it as the wire does: `status`,
    /// `trigger`, `origin_session`): a value given is never read as left
    /// out. A status change's trigger given empty is no such field: it
    /// clears the trigger.
    Blank { field: String },
    /// A trigger given with a status other than `defers`, the status a
    /// trigger defers a task to: the two would say different statuses.
    TriggerWithStatus { status: String, defers: String },
    /// A command missing a status the application resolves for it from the
    /// configured taxonomy (`field`: `defers` or `initial`), empty once
    /// trimmed: decide names no status of its own to fall back on.
    Unresolved { field: String },
}

/// Its key and args, never English (angzarr decision 14.37).
impl fmt::Display for TaskRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for TaskRejection {
    fn code(&self) -> &'static str {
        match self {
            TaskRejection::HarpIdRequired => "tasks-harp-id-required",
            TaskRejection::NotItsStream { .. } => "tasks-not-its-stream",
            TaskRejection::NoRoot { .. } => "tasks-no-root",
            TaskRejection::TaskExists { .. } => "tasks-task-exists",
            TaskRejection::NoTask => "tasks-no-task",
            TaskRejection::Removed { .. } => "tasks-removed",
            TaskRejection::TextRequired => "tasks-text-required",
            TaskRejection::StatusRequired => "tasks-status-required",
            TaskRejection::StatusReserved { .. } => "tasks-status-reserved",
            TaskRejection::TriggerRequired => "tasks-trigger-required",
            TaskRejection::TagRequired => "tasks-tag-required",
            TaskRejection::Blank { .. } => "tasks-blank",
            TaskRejection::TriggerWithStatus { .. } => "tasks-trigger-with-status",
            TaskRejection::Unresolved { .. } => "tasks-unresolved",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        match self {
            TaskRejection::NotItsStream { harp_id }
            | TaskRejection::TaskExists { harp_id }
            | TaskRejection::Removed { harp_id } => {
                vec![("harp_id", harp_id.as_str().into())]
            }
            TaskRejection::NoRoot { harp_id, cause } => vec![
                ("harp_id", harp_id.as_str().into()),
                ("cause", cause.as_str().into()),
            ],
            TaskRejection::StatusReserved { status } => vec![("status", status.as_str().into())],
            TaskRejection::Blank { field } | TaskRejection::Unresolved { field } => {
                vec![("field", field.as_str().into())]
            }
            TaskRejection::TriggerWithStatus { status, defers } => vec![
                ("status", status.as_str().into()),
                ("defers", defers.as_str().into()),
            ],
            _ => Vec::new(),
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = ("tasks", include_str!("../locales/en-US/tasks.ftl"));

/// One value of every rejection, a task's and the schema's, for the
/// catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let harp_id = || "swift-amber".to_string();
    let tasks: Vec<Box<dyn Diagnostic>> = vec![
        Box::new(TaskRejection::HarpIdRequired),
        Box::new(TaskRejection::NotItsStream { harp_id: harp_id() }),
        Box::new(TaskRejection::NoRoot {
            harp_id: harp_id(),
            cause: "a domain never contains ':'".to_string(),
        }),
        Box::new(TaskRejection::TaskExists { harp_id: harp_id() }),
        Box::new(TaskRejection::NoTask),
        Box::new(TaskRejection::Removed { harp_id: harp_id() }),
        Box::new(TaskRejection::TextRequired),
        Box::new(TaskRejection::StatusRequired),
        Box::new(TaskRejection::StatusReserved {
            status: "@open".to_string(),
        }),
        Box::new(TaskRejection::TriggerRequired),
        Box::new(TaskRejection::TagRequired),
        Box::new(TaskRejection::Blank {
            field: "trigger".to_string(),
        }),
        Box::new(TaskRejection::TriggerWithStatus {
            status: "In Progress".to_string(),
            defers: "Deferred".to_string(),
        }),
        Box::new(TaskRejection::Unresolved {
            field: "defers".to_string(),
        }),
    ];
    tasks.into_iter().chain(crate::schema::samples()).collect()
}
