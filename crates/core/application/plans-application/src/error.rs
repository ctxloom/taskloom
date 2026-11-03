//! Why a plan use case did not complete, and what `check` finds.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic, Report};
use tag_rules::{RulesRefusal, TagRefusal};

use crate::FrontError;

/// Why a plan use case failed. A wrapper of another error (those carrying
/// a [`Report`], and the tag refusals) is that error's diagnostic: its
/// code, args and English.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PlanError {
    /// A status that is none of draft, active, done and abandoned
    /// (superseded is derived, never set).
    UnknownStatus {
        status: String,
    },
    /// `set --status superseded`: a plan is superseded by another plan's
    /// `supersedes:`.
    HandSuperseded,
    /// `set` with neither a title nor a status.
    NoChange,
    /// A title with nothing in it.
    BlankTitle,
    /// A path that does not end `.plan.md`.
    NotAPlanPath {
        path: String,
    },
    /// A tag the store's rules never admit, whatever they are: not
    /// tagma's grammar, an operator word, a reserved namespace, retired.
    Tag(TagRefusal),
    /// A tag the store's rules refuse.
    Rules(RulesRefusal),
    /// `link --supersedes` of the plan itself.
    SupersedeSelf {
        plan: String,
    },
    /// `plan` superseding `target` would close a cycle.
    SupersedeCycle {
        plan: String,
        target: String,
    },
    /// A tag query tagma cannot read.
    TagQuery {
        query: String,
        postfix: String,
    },
    /// The configuration refused (a blank root, a file that does not
    /// load).
    Config(Report),
    NoSuchPlan(String),
    /// A `governs` link to a task the project does not hold.
    NoSuchTask(String),
    /// An id that several files carry: name the file by its path.
    Ambiguous {
        id: String,
        paths: String,
    },
    /// A plan named by a link that has no `id:` yet; `plans set` gives it
    /// one.
    NoId {
        path: String,
    },
    /// A plan file that could not be read.
    Unreadable {
        path: String,
    },
    /// A plan file whose frontmatter cannot be read or edited: nothing is
    /// written to it.
    Frontmatter {
        path: String,
        error: FrontError,
    },
    /// `add` of a file that is already there.
    FileExists {
        path: String,
    },
    /// Every freshly minted plan id was already taken.
    HarpsExhausted {
        attempts: u32,
    },
    /// There is no work root to find plan files in.
    NoWorkRoot(Report),
    /// The plan files could not be written.
    Files(Report),
    /// The project's tasks or rules could not be read.
    Tasks(Report),
}

impl PlanError {
    /// Whether the request is invalid on its own content (the CLI's exit
    /// 2): refused before any file or task was asked about it. The rest
    /// ran against state, and the answer was negative (exit 1).
    pub fn invalid(&self) -> bool {
        matches!(
            self,
            PlanError::UnknownStatus { .. }
                | PlanError::HandSuperseded
                | PlanError::NoChange
                | PlanError::BlankTitle
                | PlanError::NotAPlanPath { .. }
                | PlanError::Tag(_)
                | PlanError::Rules(_)
                | PlanError::SupersedeSelf { .. }
                | PlanError::SupersedeCycle { .. }
                | PlanError::TagQuery { .. }
                | PlanError::Config(_)
        )
    }

    /// The error this one wraps, whose diagnostic it is: none for one of
    /// this crate's own.
    fn wrapped(&self) -> Option<&dyn Diagnostic> {
        match self {
            PlanError::Tag(refusal) => Some(refusal),
            PlanError::Rules(refusal) => Some(refusal),
            PlanError::Config(report)
            | PlanError::NoWorkRoot(report)
            | PlanError::Files(report)
            | PlanError::Tasks(report) => Some(report),
            _ => None,
        }
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for PlanError {
    fn code(&self) -> &str {
        if let Some(wrapped) = self.wrapped() {
            return wrapped.code();
        }
        match self {
            PlanError::UnknownStatus { .. } => code!("unknown-status"),
            PlanError::HandSuperseded => code!("hand-superseded"),
            PlanError::NoChange => code!("no-change"),
            PlanError::BlankTitle => code!("blank-title"),
            PlanError::NotAPlanPath { .. } => code!("not-a-plan-path"),
            PlanError::SupersedeSelf { .. } => code!("supersede-self"),
            PlanError::SupersedeCycle { .. } => code!("supersede-cycle"),
            PlanError::TagQuery { .. } => code!("tag-query"),
            PlanError::NoSuchPlan(_) => code!("no-such-plan"),
            PlanError::NoSuchTask(_) => code!("no-such-task"),
            PlanError::Ambiguous { .. } => code!("ambiguous"),
            PlanError::NoId { .. } => code!("no-id"),
            PlanError::Unreadable { .. } => code!("unreadable"),
            PlanError::Frontmatter { .. } => code!("frontmatter"),
            PlanError::FileExists { .. } => code!("file-exists"),
            PlanError::HarpsExhausted { .. } => code!("harps-exhausted"),
            _ => unreachable!("a wrapper's code is its cause's"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        if let Some(wrapped) = self.wrapped() {
            return wrapped.args();
        }
        let s = |v: &String| Arg::from(v.as_str());
        match self {
            PlanError::UnknownStatus { status } => vec![("status", s(status))],
            PlanError::HandSuperseded | PlanError::NoChange | PlanError::BlankTitle => Vec::new(),
            PlanError::NotAPlanPath { path }
            | PlanError::NoId { path }
            | PlanError::Unreadable { path }
            | PlanError::FileExists { path } => vec![("path", s(path))],
            PlanError::SupersedeSelf { plan } | PlanError::NoSuchPlan(plan) => {
                vec![("plan", s(plan))]
            }
            PlanError::SupersedeCycle { plan, target } => {
                vec![("plan", s(plan)), ("target", s(target))]
            }
            PlanError::TagQuery { query, postfix } => {
                vec![("query", s(query)), ("postfix", s(postfix))]
            }
            PlanError::NoSuchTask(task) => vec![("task", s(task))],
            PlanError::Ambiguous { id, paths } => vec![("id", s(id)), ("paths", s(paths))],
            PlanError::Frontmatter { path, error } => {
                vec![("path", s(path)), ("error", Arg::nested(error))]
            }
            PlanError::HarpsExhausted { attempts } => vec![("attempts", (*attempts).into())],
            _ => unreachable!("a wrapper's args are its cause's"),
        }
    }
}

/// What `plans check` found wrong with a plan file. Each is a diagnostic,
/// so it is said as every diagnostic is, naming its file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Finding {
    /// The file could not be read.
    Unreadable { path: String },
    /// The frontmatter does not parse, or holds a value of the wrong
    /// shape.
    Frontmatter { path: String, error: FrontError },
    /// The file has no `id:`: taskloom gives it one the first time it
    /// writes the file.
    NoId { path: String },
    /// Another file carries the same id (`others` the paths, joined).
    DuplicateId {
        path: String,
        id: String,
        others: String,
    },
    /// A tag the store's rules do not admit.
    Tag {
        path: String,
        tag: String,
        error: Report,
    },
    /// `governs` names a task the project does not hold.
    Governs { path: String, task: String },
    /// `supersedes` names a plan no file is.
    Supersedes { path: String, plan: String },
    /// `supersedes` names the plan itself.
    SupersedesSelf { path: String, plan: String },
    /// A status that is none of the statuses.
    Status { path: String, status: String },
    /// `status: superseded` written by hand: it is derived.
    HandSuperseded { path: String },
}

impl Finding {
    /// The file the finding is about.
    pub fn path(&self) -> &str {
        match self {
            Finding::Unreadable { path }
            | Finding::Frontmatter { path, .. }
            | Finding::NoId { path }
            | Finding::DuplicateId { path, .. }
            | Finding::Tag { path, .. }
            | Finding::Governs { path, .. }
            | Finding::Supersedes { path, .. }
            | Finding::SupersedesSelf { path, .. }
            | Finding::Status { path, .. }
            | Finding::HandSuperseded { path } => path,
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for Finding {
    fn code(&self) -> &str {
        match self {
            Finding::Unreadable { .. } => code!("finding-unreadable"),
            Finding::Frontmatter { .. } => code!("finding-frontmatter"),
            Finding::NoId { .. } => code!("finding-no-id"),
            Finding::DuplicateId { .. } => code!("finding-duplicate-id"),
            Finding::Tag { .. } => code!("finding-tag"),
            Finding::Governs { .. } => code!("finding-governs"),
            Finding::Supersedes { .. } => code!("finding-supersedes"),
            Finding::SupersedesSelf { .. } => code!("finding-supersedes-self"),
            Finding::Status { .. } => code!("finding-status"),
            Finding::HandSuperseded { .. } => code!("finding-hand-superseded"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        let s = |v: &String| Arg::from(v.as_str());
        let path = ("path", Arg::from(self.path()));
        match self {
            Finding::Unreadable { .. } | Finding::NoId { .. } | Finding::HandSuperseded { .. } => {
                vec![path]
            }
            Finding::Frontmatter { error, .. } => vec![path, ("error", Arg::nested(error))],
            Finding::DuplicateId { id, others, .. } => {
                vec![path, ("id", s(id)), ("others", s(others))]
            }
            Finding::Tag { tag, error, .. } => {
                vec![path, ("tag", s(tag)), ("error", Arg::nested(error))]
            }
            Finding::Governs { task, .. } => vec![path, ("task", s(task))],
            Finding::Supersedes { plan, .. } | Finding::SupersedesSelf { plan, .. } => {
                vec![path, ("plan", s(plan))]
            }
            Finding::Status { status, .. } => vec![path, ("status", s(status))],
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!(concat!(env!("OUT_DIR"), "/en-US.ftl")),
);

/// Every catalog the plans use cases' diagnostics come from beyond the
/// tag rules' (which the tasks' list holds): this crate's.
pub const CATALOGS: [(&str, &str); 1] = [EN_US];

/// One value of every error this crate defines (not the wrappers, whose
/// codes are their causes'), of every finding and of every frontmatter
/// error, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let x = || "x".to_string();
    let front = || FrontError::Unclosed;
    vec![
        Box::new(PlanError::UnknownStatus { status: x() }),
        Box::new(PlanError::HandSuperseded),
        Box::new(PlanError::NoChange),
        Box::new(PlanError::BlankTitle),
        Box::new(PlanError::NotAPlanPath { path: x() }),
        Box::new(PlanError::SupersedeSelf { plan: x() }),
        Box::new(PlanError::SupersedeCycle {
            plan: x(),
            target: x(),
        }),
        Box::new(PlanError::TagQuery {
            query: x(),
            postfix: x(),
        }),
        Box::new(PlanError::NoSuchPlan(x())),
        Box::new(PlanError::NoSuchTask(x())),
        Box::new(PlanError::Ambiguous {
            id: x(),
            paths: x(),
        }),
        Box::new(PlanError::NoId { path: x() }),
        Box::new(PlanError::Unreadable { path: x() }),
        Box::new(PlanError::Frontmatter {
            path: x(),
            error: front(),
        }),
        Box::new(PlanError::FileExists { path: x() }),
        Box::new(PlanError::HarpsExhausted { attempts: 100 }),
        Box::new(Finding::Unreadable { path: x() }),
        Box::new(Finding::Frontmatter {
            path: x(),
            error: front(),
        }),
        Box::new(Finding::NoId { path: x() }),
        Box::new(Finding::DuplicateId {
            path: x(),
            id: x(),
            others: x(),
        }),
        Box::new(Finding::Tag {
            path: x(),
            tag: x(),
            error: Report::new("x-refused"),
        }),
        Box::new(Finding::Governs {
            path: x(),
            task: x(),
        }),
        Box::new(Finding::Supersedes {
            path: x(),
            plan: x(),
        }),
        Box::new(Finding::SupersedesSelf {
            path: x(),
            plan: x(),
        }),
        Box::new(Finding::Status {
            path: x(),
            status: x(),
        }),
        Box::new(Finding::HandSuperseded { path: x() }),
        Box::new(FrontError::Unclosed),
        Box::new(FrontError::Yaml { detail: x() }),
        Box::new(FrontError::NotAMapping),
        Box::new(FrontError::Shape { key: x() }),
        Box::new(FrontError::Edit {
            key: x(),
            detail: x(),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
