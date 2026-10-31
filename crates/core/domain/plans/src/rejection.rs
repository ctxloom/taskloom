//! Why a plan refused a command.

use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};

use crate::PlanStatus;

/// Why a plan refused a command.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PlanRejection {
    /// An add to a stream that already holds a plan.
    AlreadyAdded,
    /// Anything but an add to an empty stream.
    NoSuchPlan,
    EmptyTitle,
    /// An add whose status is not Draft or Active.
    InitialStatus(PlanStatus),
    /// The status it already has.
    StatusUnchanged(PlanStatus),
    /// A move [`PlanStatus::may_become`] refuses.
    Transition {
        from: PlanStatus,
        to: PlanStatus,
    },
    /// `plan status superseded`: only a supersede link sets it.
    SupersededBySupersedeOnly,
    AlreadyGoverns(String),
    NotGoverned(String),
    /// A plan naming itself in `supersedes`.
    SupersedesItself,
    AlreadySupersedes(String),
    NotSuperseded(String),
    /// A rehash to the hash already recorded.
    HashUnchanged,
    /// A hash that is not 64 lowercase hex digits.
    BadHash(String),
    /// An add whose harp id is not the one its stream's root derives from.
    NotItsStream(String),
    /// An add whose stream root cannot be derived from its harp id:
    /// `cause` says why.
    NoRoot {
        harp_id: String,
        cause: String,
    },
}

/// Its key and args, never English (angzarr decision 14.37).
impl fmt::Display for PlanRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for PlanRejection {
    fn code(&self) -> &'static str {
        match self {
            PlanRejection::AlreadyAdded => "plans-already-added",
            PlanRejection::NoSuchPlan => "plans-no-such-plan",
            PlanRejection::EmptyTitle => "plans-empty-title",
            PlanRejection::InitialStatus(_) => "plans-initial-status",
            PlanRejection::StatusUnchanged(_) => "plans-status-unchanged",
            PlanRejection::Transition { .. } => "plans-transition",
            PlanRejection::SupersededBySupersedeOnly => "plans-superseded-by-supersede-only",
            PlanRejection::AlreadyGoverns(_) => "plans-already-governs",
            PlanRejection::NotGoverned(_) => "plans-not-governed",
            PlanRejection::SupersedesItself => "plans-supersedes-itself",
            PlanRejection::AlreadySupersedes(_) => "plans-already-supersedes",
            PlanRejection::NotSuperseded(_) => "plans-not-superseded",
            PlanRejection::HashUnchanged => "plans-hash-unchanged",
            PlanRejection::BadHash(_) => "plans-bad-hash",
            PlanRejection::NotItsStream(_) => "plans-not-its-stream",
            PlanRejection::NoRoot { .. } => "plans-no-root",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        let status = |s: &PlanStatus| Arg::from(s.name());
        match self {
            PlanRejection::InitialStatus(s) | PlanRejection::StatusUnchanged(s) => {
                vec![("status", status(s))]
            }
            PlanRejection::Transition { from, to } => {
                vec![("from", status(from)), ("to", status(to))]
            }
            PlanRejection::AlreadyGoverns(task) | PlanRejection::NotGoverned(task) => {
                vec![("task", task.as_str().into())]
            }
            PlanRejection::AlreadySupersedes(plan) | PlanRejection::NotSuperseded(plan) => {
                vec![("plan", plan.as_str().into())]
            }
            PlanRejection::BadHash(hash) => vec![("hash", hash.as_str().into())],
            PlanRejection::NotItsStream(harp_id) => vec![("harp_id", harp_id.as_str().into())],
            PlanRejection::NoRoot { harp_id, cause } => vec![
                ("harp_id", harp_id.as_str().into()),
                ("cause", cause.as_str().into()),
            ],
            PlanRejection::AlreadyAdded
            | PlanRejection::NoSuchPlan
            | PlanRejection::EmptyTitle
            | PlanRejection::SupersededBySupersedeOnly
            | PlanRejection::SupersedesItself
            | PlanRejection::HashUnchanged => Vec::new(),
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = ("plans", include_str!("../locales/en-US/plans.ftl"));

/// One value of every rejection, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let id = || "brisk-cedar-otter".to_string();
    vec![
        Box::new(PlanRejection::AlreadyAdded),
        Box::new(PlanRejection::NoSuchPlan),
        Box::new(PlanRejection::EmptyTitle),
        Box::new(PlanRejection::InitialStatus(PlanStatus::Done)),
        Box::new(PlanRejection::StatusUnchanged(PlanStatus::Active)),
        Box::new(PlanRejection::Transition {
            from: PlanStatus::Draft,
            to: PlanStatus::Done,
        }),
        Box::new(PlanRejection::SupersededBySupersedeOnly),
        Box::new(PlanRejection::AlreadyGoverns("swift-amber".to_string())),
        Box::new(PlanRejection::NotGoverned("swift-amber".to_string())),
        Box::new(PlanRejection::SupersedesItself),
        Box::new(PlanRejection::AlreadySupersedes(id())),
        Box::new(PlanRejection::NotSuperseded(id())),
        Box::new(PlanRejection::HashUnchanged),
        Box::new(PlanRejection::BadHash("abc".to_string())),
        Box::new(PlanRejection::NotItsStream(id())),
        Box::new(PlanRejection::NoRoot {
            harp_id: id(),
            cause: "a domain never contains ':'".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "rejection.test.rs"]
mod tests;
