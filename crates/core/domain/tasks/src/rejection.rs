//! Why a command to a task was refused.

use std::fmt;

use annals_diagnostic::{Arg, Diagnostic};

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

impl fmt::Display for TaskRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskRejection::HarpIdRequired => f.write_str("a task needs a harp id"),
            TaskRejection::NotItsStream { harp_id } => {
                write!(f, "task {harp_id} does not belong in this stream")
            }
            TaskRejection::NoRoot { harp_id, cause } => write!(
                f,
                "task {harp_id} has no stream to belong in: its root cannot be derived ({cause})"
            ),
            TaskRejection::TaskExists { harp_id } => write!(f, "task {harp_id} already exists"),
            TaskRejection::NoTask => f.write_str("there is no such task"),
            TaskRejection::Removed { harp_id } => write!(f, "task {harp_id} was removed"),
            TaskRejection::TextRequired => f.write_str("a task needs text"),
            TaskRejection::StatusRequired => f.write_str("a status change needs a status, a trigger, or the trigger cleared"),
            TaskRejection::StatusReserved { status } => write!(
                f,
                "the status {status} is not allowed: '@' is reserved for status classes"
            ),
            TaskRejection::TriggerRequired => {
                f.write_str("a Deferred task needs a trigger: the condition that should revive it")
            }
            TaskRejection::TagRequired => f.write_str("a retag needs at least one tag"),
            TaskRejection::Blank { field } => {
                write!(f, "the {field} given is blank: give one, or leave it out")
            }
            TaskRejection::TriggerWithStatus { status, defers } => write!(
                f,
                "a trigger defers the task to {defers}, so it cannot be given with the status {status}: give the trigger alone, or the status alone"
            ),
            TaskRejection::Unresolved { field } => write!(
                f,
                "the command carries no {field} status: the application resolves it from the configured statuses"
            ),
        }
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

/// The same key and args, as angzarr-embedded's markup refuses with them
/// (`TaskAggregate`): a key and its args, never English (decision 14.37).
impl angzarr_embedded_diagnostic::Diagnostic for TaskRejection {
    fn code(&self) -> &'static str {
        Diagnostic::code(self)
    }

    fn args(&self) -> Vec<(&'static str, angzarr_embedded_diagnostic::Arg)> {
        use angzarr_embedded_diagnostic::Arg as Embedded;
        Diagnostic::args(self)
            .into_iter()
            .map(|(name, arg)| {
                let arg = match arg {
                    Arg::Int(n) => Embedded::Int(n),
                    Arg::Uint(n) => Embedded::Uint(n),
                    Arg::Str(s) => Embedded::Str(s),
                    Arg::Nested(cause) => Embedded::Str(cause.code().to_string()),
                };
                (name, arg)
            })
            .collect()
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = ("tasks", include_str!("../locales/en-US/tasks.ftl"));

/// One value of every rejection, for the catalog test.
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let harp_id = || "swift-amber".to_string();
    vec![
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
    ]
}
