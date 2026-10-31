//! The plans domain. One plan is one aggregate, keyed by a harp id as a
//! task is: its title, its [`PlanStatus`], the work-root-relative path of
//! its markdown text and that text's content hash, and its typed links,
//! `governs` (plan to task, many) and `supersedes` (plan to plan).
//!
//! RED STUB (docs/plans/plans-aggregate.md): the interface proposed for
//! review, every body `todo!()`. Implemented, the commands, events and
//! state move into `proto/ctxloom/plans/v1/plans.proto` (angzarr-build's
//! markup layer) and [`decide`] and [`apply`] become a `PlanAggregate` on
//! angzarr-embedded, as `TaskAggregate` is; the rejection gets codes, args
//! and an en-US catalog. The rules are the same pure functions either way.
//!
//! What one stream cannot see is not decided here: whether a linked task or
//! plan exists, and whether a supersede closes a cycle, are the
//! application's checks (it reads those streams first), because a domain
//! reads no other stream and no projection.

// RED STUB: the bodies are `todo!()`, so their arguments are unused.
#![allow(unused_variables)]

use std::collections::BTreeSet;

/// A plan's status. A new plan is [`PlanStatus::Draft`] unless the add says
/// [`PlanStatus::Active`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum PlanStatus {
    /// Being written; governs nothing yet in spirit, though links are kept.
    #[default]
    Draft,
    /// The plan in force.
    Active,
    /// Replaced by another plan, which `supersedes` it. Only a supersede
    /// link sets it, never `plan status`.
    Superseded,
    /// Carried out.
    Done,
    /// Dropped without being carried out.
    Abandoned,
}

impl PlanStatus {
    /// Every status, in display order.
    pub const ALL: [PlanStatus; 5] = [
        PlanStatus::Draft,
        PlanStatus::Active,
        PlanStatus::Superseded,
        PlanStatus::Done,
        PlanStatus::Abandoned,
    ];

    /// Whether the plan is finished with (Superseded, Done, Abandoned).
    pub fn terminal(self) -> bool {
        todo!("RED: PlanStatus::terminal")
    }

    /// Whether `plan status` may move a plan from `self` to `to`.
    pub fn may_become(self, to: PlanStatus) -> bool {
        todo!("RED: PlanStatus::may_become")
    }

    /// The name the CLI and JSON use: `draft`, `active`, ...
    pub fn name(self) -> &'static str {
        todo!("RED: PlanStatus::name")
    }

    /// The status `name` spells, any case.
    pub fn parse(name: &str) -> Option<PlanStatus> {
        todo!("RED: PlanStatus::parse")
    }
}

/// A plan as its stream folds it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PlanState {
    /// Whether the plan was added: false for an empty stream.
    pub added: bool,
    pub harp_id: String,
    pub title: String,
    pub status: PlanStatus,
    /// The text's path, relative to the work root (`/`-separated), or, for
    /// a file `plan import` registered from outside it, absolute.
    pub path: String,
    /// The text's sha-256, lowercase hex, as last recorded.
    pub content_hash: String,
    /// The session the plan was added in (CTXLOOM_SESSION_HARP), or empty.
    pub origin_session: String,
    /// The tasks this plan governs, by harp id.
    pub governs: BTreeSet<String>,
    /// The plans this plan supersedes, by harp id.
    pub supersedes: BTreeSet<String>,
    /// The plan that supersedes this one, while it is Superseded.
    pub superseded_by: String,
}

/// What a plan is asked to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PlanCommand {
    Add(AddPlan),
    /// `plan status`: any status but Superseded.
    SetStatus(PlanStatus),
    /// `plan link --governs`: the application has seen the task exists.
    Govern(String),
    /// `plan unlink --governs`.
    Ungovern(String),
    /// `plan link --supersedes`, on the superseding plan: the application
    /// has seen the plan exists and that no cycle closes.
    Supersede(String),
    /// `plan unlink --supersedes`, on the superseding plan.
    Unsupersede(String),
    /// On the superseded plan, sent by the application after a
    /// [`PlanCommand::Supersede`]: it becomes Superseded by the plan named.
    MarkSuperseded(String),
    /// On the superseded plan, after a [`PlanCommand::Unsupersede`]: it is
    /// Active again.
    Reinstate,
    /// `plan check --accept`: the text's hash as it now is.
    Rehash(String),
}

/// `plan add`, as the application sends it: the path already resolved
/// and the hash already taken.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AddPlan {
    pub harp_id: String,
    pub title: String,
    pub status: PlanStatus,
    pub path: String,
    pub content_hash: String,
    pub origin_session: String,
}

/// What a plan's stream records.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PlanEvent {
    PlanAdded(AddPlan),
    StatusChanged { from: PlanStatus, to: PlanStatus },
    Governed(String),
    Ungoverned(String),
    Superseded(String),
    Unsuperseded(String),
    MarkedSuperseded { by: String, from: PlanStatus },
    Reinstated,
    Rehashed(String),
}

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
}

/// What `command` to the plan `state` records, or why it is refused.
/// Pure: no clock, no I/O, no other stream.
pub fn decide(state: &PlanState, command: &PlanCommand) -> Result<Vec<PlanEvent>, PlanRejection> {
    todo!("RED: plans::decide")
}

/// Fold `event` into `state`. Pure.
pub fn apply(state: &mut PlanState, event: &PlanEvent) {
    todo!("RED: plans::apply")
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
