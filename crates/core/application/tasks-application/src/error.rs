//! Why a use case did not complete.

use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic, Report};

use crate::DeadLetter;

/// Why a use case did not complete. A wrapper of another error
/// ([`AppError::Rejected`], [`AppError::TagRefused`], [`AppError::Failed`],
/// [`AppError::Identity`], [`AppError::Configuration`],
/// [`AppError::TagQuery`], [`AppError::Prune`], [`AppError::Priority`],
/// [`AppError::DeadLettered`]) is that error's diagnostic: its code, args
/// and English.
#[derive(Clone, PartialEq, Debug)]
pub enum AppError {
    /// The domain refused the command: tell the person.
    Rejected(Report),
    /// A tag given to a write was refused before any command was sent
    /// ([`crate::TagRefusal`]): tell the person.
    TagRefused(Report),
    /// The store or the kernel failed in a way a retry will not mend.
    Failed(Report),
    /// The project could not be resolved.
    Identity(Report),
    /// The configuration could not be read, or its tag schema is malformed.
    Configuration(Report),
    /// A malformed tag query ([`crate::query::QueryError`]).
    TagQuery(Report),
    /// A prune refused before it read anything ([`crate::PruneRefusal`]).
    Prune(Report),
    /// Every attempt the retry budget allows failed with a retryable error;
    /// `error` is the last.
    GaveUp { attempts: u32, error: Report },
    /// Every freshly minted task id was already taken.
    HarpsExhausted { attempts: u32 },
    /// A task the store accepted is not there when read back: removed
    /// between the commit and the read-back, so the write did land.
    Vanished { harp_id: String },
    /// A change to an existing task where the work root has no project.
    NoProject,
    /// A status filter's value starting with `@` that names no class.
    UnknownStatusClass { status: String },
    /// A sort taskloom does not know.
    UnknownSort { sort: String },
    /// A value given blank (`field` names it, as a request does: `status`,
    /// `trigger`, `origin_session`, `request_id`, `term`, `tag_query`,
    /// `sort`, or `limit`, given as 0): a value given is never read as one left out.
    Blank { field: &'static str },
    /// A status change giving a trigger and clearing the trigger at once.
    TriggerAndClear,
    /// An add given a request id, refused until fix O pins the harp an add
    /// mints by its key: without it, a retried add adds the task again.
    AddRequestIdNotYet,
    /// Priority could not be derived (`task_priority::PriorityError`): a
    /// formula that does not compile, or a task it cannot score.
    Priority(Report),
    /// Ids asked for that no task holds: every one of them.
    NotFound { harp_ids: Vec<String> },
    /// Ids held in more than one project, read across projects: each with
    /// the projects holding it.
    Ambiguous { held: Vec<(String, Vec<String>)> },
    /// A permit not spelled as one: a namespace (`area:`), a namespace's
    /// key (`area:cli`) or a flat tag (`urgent`), with no value, outside
    /// tagma's namespace, and not a query operator.
    PermitMalformed { permit: String },
    /// A revoke of what is not among the project's permits (a key of an
    /// open namespace among them: its namespace's permit admits it).
    PermitNotDeclared { permit: String },
    /// A revoke of a permit tasks still carry a tag under: every one of
    /// them, by harp id.
    PermitInUse {
        permit: String,
        harp_ids: Vec<String>,
    },
    /// `error`, from a write during which the kernel dead-lettered
    /// `dead_letters` (never none): `error`'s diagnostic, its code, args
    /// and English, with the lines beside it for the driving adapter to
    /// say.
    DeadLettered {
        error: Box<AppError>,
        dead_letters: Vec<DeadLetter>,
    },
}

impl AppError {
    /// The dead letters the kernel wrote while the use case that
    /// failed with this ran: none for most.
    pub fn dead_letters(&self) -> &[DeadLetter] {
        match self {
            AppError::DeadLettered { dead_letters, .. } => dead_letters,
            _ => &[],
        }
    }

    /// This error, carrying `dead_letters` when there are any.
    pub(crate) fn with_dead_letters(self, dead_letters: Vec<DeadLetter>) -> AppError {
        match dead_letters.is_empty() {
            true => self,
            false => AppError::DeadLettered {
                error: Box::new(self),
                dead_letters,
            },
        }
    }
}

/// Each of `ids`, quoted, comma-separated.
fn quoted(ids: &[String]) -> String {
    ids.iter()
        .map(|id| format!("{id:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Each id held twice and the projects that hold it.
fn held_in(held: &[(String, Vec<String>)]) -> String {
    held.iter()
        .map(|(id, projects)| format!("{id:?} in {}", projects.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for AppError {
    fn code(&self) -> &'static str {
        match self {
            AppError::Rejected(cause)
            | AppError::TagRefused(cause)
            | AppError::Failed(cause)
            | AppError::Identity(cause)
            | AppError::Configuration(cause)
            | AppError::TagQuery(cause)
            | AppError::Prune(cause)
            | AppError::Priority(cause) => cause.code(),
            AppError::DeadLettered { error, .. } => error.code(),
            AppError::GaveUp { .. } => "tasks-application-gave-up",
            AppError::HarpsExhausted { .. } => "tasks-application-harps-exhausted",
            AppError::Vanished { .. } => "tasks-application-vanished",
            AppError::NoProject => "tasks-application-no-project",
            AppError::UnknownStatusClass { .. } => "tasks-application-unknown-status-class",
            AppError::UnknownSort { .. } => "tasks-application-unknown-sort",
            AppError::Blank { .. } => "tasks-application-blank",
            AppError::TriggerAndClear => "tasks-application-trigger-and-clear",
            AppError::AddRequestIdNotYet => "tasks-application-add-request-id-not-yet",
            AppError::NotFound { .. } => "tasks-application-not-found",
            AppError::Ambiguous { .. } => "tasks-application-ambiguous",
            AppError::PermitMalformed { .. } => "tasks-application-permit-malformed",
            AppError::PermitNotDeclared { .. } => "tasks-application-permit-not-declared",
            AppError::PermitInUse { .. } => "tasks-application-permit-in-use",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        match self {
            AppError::Rejected(cause)
            | AppError::TagRefused(cause)
            | AppError::Failed(cause)
            | AppError::Identity(cause)
            | AppError::Configuration(cause)
            | AppError::TagQuery(cause)
            | AppError::Prune(cause)
            | AppError::Priority(cause) => cause.args(),
            AppError::DeadLettered { error, .. } => error.args(),
            AppError::GaveUp { attempts, error } => vec![
                ("attempts", (*attempts).into()),
                ("error", Arg::nested(error)),
            ],
            AppError::HarpsExhausted { attempts } => vec![("attempts", (*attempts).into())],
            AppError::Vanished { harp_id } => vec![("harp_id", harp_id.as_str().into())],
            AppError::NoProject | AppError::TriggerAndClear | AppError::AddRequestIdNotYet => {
                Vec::new()
            }
            AppError::UnknownStatusClass { status } => vec![
                ("status", status.as_str().into()),
                ("classes", crate::read::classes().into()),
            ],
            AppError::UnknownSort { sort } => vec![("sort", sort.as_str().into())],
            AppError::Blank { field } => vec![("field", (*field).into())],
            AppError::NotFound { harp_ids } => vec![
                ("harp_ids", quoted(harp_ids).into()),
                ("count", harp_ids.len().into()),
            ],
            AppError::Ambiguous { held } => {
                vec![("held", held_in(held).into()), ("count", held.len().into())]
            }
            AppError::PermitMalformed { permit } | AppError::PermitNotDeclared { permit } => {
                vec![("permit", permit.as_str().into())]
            }
            AppError::PermitInUse { permit, harp_ids } => vec![
                ("permit", permit.as_str().into()),
                ("harp_ids", quoted(harp_ids).into()),
                ("count", harp_ids.len().into()),
            ],
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    "tasks-application",
    include_str!("../locales/en-US/tasks-application.ftl"),
);

/// The en-US catalogs of every diagnostic a use case returns but its
/// adapters': the domain's rejections and this crate's errors.
pub const CATALOGS: [(&str, &str); 3] = [tasks::EN_US, task_priority::EN_US, EN_US];

/// One value of every error this crate defines, of every reason a tag is
/// refused, and of every reason an import gives for leaving a record behind (not the wrappers, whose codes
/// are their causes'), for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let tags = crate::tags::samples().into_iter();
    let mut samples: Vec<Box<dyn Diagnostic>> = vec![
        Box::new(AppError::GaveUp {
            attempts: 10,
            error: Report::new("x-busy"),
        }),
        Box::new(AppError::HarpsExhausted { attempts: 100 }),
        Box::new(AppError::Vanished {
            harp_id: "swift-amber".to_string(),
        }),
        Box::new(AppError::NoProject),
        Box::new(crate::DeadLetter {
            line: "DEAD LETTER (almost certainly a bug): x".to_string(),
        }),
        Box::new(crate::import::NoJsonlLog {
            project: "jolly-mute-magma".to_string(),
        }),
        Box::new(AppError::UnknownStatusClass {
            status: "@done".to_string(),
        }),
        Box::new(AppError::UnknownSort {
            sort: "age".to_string(),
        }),
        Box::new(AppError::Blank { field: "trigger" }),
        Box::new(AppError::TriggerAndClear),
        Box::new(AppError::AddRequestIdNotYet),
        Box::new(AppError::NotFound {
            harp_ids: vec!["swift-amber".to_string(), "calm-otter".to_string()],
        }),
        Box::new(AppError::Ambiguous {
            held: vec![(
                "swift-amber".to_string(),
                vec!["jolly-mute-magma".to_string(), "brisk-tall-oak".to_string()],
            )],
        }),
        Box::new(AppError::PermitMalformed {
            permit: "area:cli=x".to_string(),
        }),
        Box::new(AppError::PermitNotDeclared {
            permit: "sig:".to_string(),
        }),
        Box::new(AppError::PermitInUse {
            permit: "area:cli".to_string(),
            harp_ids: vec!["swift-amber".to_string(), "calm-otter".to_string()],
        }),
        Box::new(crate::Why::Displaced),
        Box::new(crate::Why::HarpTaken),
        Box::new(crate::Why::TaskNotImported),
    ];
    samples.extend(tags.map(|t| Box::new(t) as Box<dyn Diagnostic>));
    samples.extend(crate::store_schema::samples());
    samples.extend(
        crate::lint::samples()
            .into_iter()
            .map(|f| Box::new(f) as Box<dyn Diagnostic>),
    );
    samples.extend(
        crate::prune::samples()
            .into_iter()
            .map(|p| Box::new(p) as Box<dyn Diagnostic>),
    );
    samples.extend(
        crate::priority::samples()
            .into_iter()
            .map(|n| Box::new(n) as Box<dyn Diagnostic>),
    );
    samples.push(Box::new(crate::query::QueryError {
        query: "and".to_string(),
        postfix: "x".to_string(),
    }));
    samples
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
