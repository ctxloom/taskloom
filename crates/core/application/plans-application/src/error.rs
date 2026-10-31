//! Why a plan use case did not complete.

use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic, Report};

use crate::PlanRejection;

/// Why a path cannot be a plan's.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PathError {
    /// It leaves the work root (`..` past it, or absolute elsewhere).
    OutsideWorkRoot(String),
    /// It does not end `.plan.md`.
    NotAPlanFile(String),
    Empty,
}

/// Why a plan use case failed. A wrapper of another error
/// ([`PlanError::Rejected`], [`PlanError::Path`], and those carrying a
/// [`Report`]) is that error's diagnostic: its code, args and English.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PlanError {
    /// The aggregate refused.
    Rejected(PlanRejection),
    NoSuchPlan(String),
    /// A `governs` link to a task the project does not hold.
    NoSuchTask(String),
    /// `plan` superseding `target` would close a cycle.
    SupersedeCycle {
        plan: String,
        target: String,
    },
    /// A supersede of `plan`, which `by` already supersedes.
    AlreadySuperseded {
        plan: String,
        by: String,
    },
    Path(PathError),
    /// `plan add --path` naming a file that is not there.
    FileMissing(String),
    /// `plan add` with no path, and the default path's file already exists.
    FileExists(String),
    /// A path another plan's text already is.
    AlreadyRegistered {
        path: String,
        plan: String,
    },
    /// Every freshly minted plan id was already taken.
    HarpsExhausted {
        attempts: u32,
    },
    /// There is no work root to find plan files in.
    NoWorkRoot(Report),
    /// The project could not be resolved.
    Identity(Report),
    /// The work tree's files could not be written.
    Files(Report),
    /// The project's tasks could not be read or retagged.
    Tasks(Report),
    /// The store or the kernel failed in a way a retry will not mend.
    Failed(Report),
    /// Every attempt the retry budget allows failed with a retryable
    /// error; `error` is the last.
    GaveUp {
        attempts: u32,
        error: Report,
    },
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for PathError {
    fn code(&self) -> &'static str {
        match self {
            PathError::OutsideWorkRoot(_) => "plans-application-outside-work-root",
            PathError::NotAPlanFile(_) => "plans-application-not-a-plan-file",
            PathError::Empty => "plans-application-empty-path",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        match self {
            PathError::OutsideWorkRoot(path) | PathError::NotAPlanFile(path) => {
                vec![("path", path.as_str().into())]
            }
            PathError::Empty => Vec::new(),
        }
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl PlanError {
    /// The error this one wraps, whose diagnostic it is: none for one of
    /// this crate's own.
    fn wrapped(&self) -> Option<&dyn Diagnostic> {
        match self {
            PlanError::Rejected(rejection) => Some(rejection),
            PlanError::Path(path) => Some(path),
            PlanError::NoWorkRoot(report)
            | PlanError::Identity(report)
            | PlanError::Files(report)
            | PlanError::Tasks(report)
            | PlanError::Failed(report) => Some(report),
            _ => None,
        }
    }
}

impl Diagnostic for PlanError {
    fn code(&self) -> &'static str {
        if let Some(wrapped) = self.wrapped() {
            return wrapped.code();
        }
        match self {
            PlanError::NoSuchPlan(_) => "plans-application-no-such-plan",
            PlanError::NoSuchTask(_) => "plans-application-no-such-task",
            PlanError::SupersedeCycle { .. } => "plans-application-supersede-cycle",
            PlanError::AlreadySuperseded { .. } => "plans-application-already-superseded",
            PlanError::FileMissing(_) => "plans-application-file-missing",
            PlanError::FileExists(_) => "plans-application-file-exists",
            PlanError::AlreadyRegistered { .. } => "plans-application-already-registered",
            PlanError::HarpsExhausted { .. } => "plans-application-harps-exhausted",
            PlanError::GaveUp { .. } => "plans-application-gave-up",
            _ => unreachable!("a wrapper's code is its cause's"),
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        if let Some(wrapped) = self.wrapped() {
            return wrapped.args();
        }
        let s = |v: &String| Arg::from(v.as_str());
        match self {
            PlanError::NoSuchPlan(plan) => vec![("plan", s(plan))],
            PlanError::NoSuchTask(task) => vec![("task", s(task))],
            PlanError::SupersedeCycle { plan, target } => {
                vec![("plan", s(plan)), ("target", s(target))]
            }
            PlanError::AlreadySuperseded { plan, by } => vec![("plan", s(plan)), ("by", s(by))],
            PlanError::FileMissing(path) | PlanError::FileExists(path) => {
                vec![("path", s(path))]
            }
            PlanError::AlreadyRegistered { path, plan } => {
                vec![("path", s(path)), ("plan", s(plan))]
            }
            PlanError::HarpsExhausted { attempts } => vec![("attempts", (*attempts).into())],
            PlanError::GaveUp { attempts, error } => vec![
                ("attempts", (*attempts).into()),
                ("error", Arg::nested(error)),
            ],
            _ => unreachable!("a wrapper's args are its cause's"),
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    "plans-application",
    include_str!("../locales/en-US/plans-application.ftl"),
);

/// Every catalog the plans use cases' diagnostics come from: the domain's
/// and this crate's.
pub const CATALOGS: [(&str, &str); 2] = [plans::EN_US, EN_US];

/// One value of every error this crate defines (not the wrappers, whose
/// codes are their causes'), for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let x = || "x".to_string();
    vec![
        Box::new(PathError::OutsideWorkRoot(x())),
        Box::new(PathError::NotAPlanFile(x())),
        Box::new(PathError::Empty),
        Box::new(PlanError::NoSuchPlan(x())),
        Box::new(PlanError::NoSuchTask(x())),
        Box::new(PlanError::SupersedeCycle {
            plan: x(),
            target: x(),
        }),
        Box::new(PlanError::AlreadySuperseded { plan: x(), by: x() }),
        Box::new(PlanError::FileMissing(x())),
        Box::new(PlanError::FileExists(x())),
        Box::new(PlanError::AlreadyRegistered {
            path: x(),
            plan: x(),
        }),
        Box::new(PlanError::HarpsExhausted { attempts: 100 }),
        Box::new(PlanError::GaveUp {
            attempts: 10,
            error: Report::new("x-busy"),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
