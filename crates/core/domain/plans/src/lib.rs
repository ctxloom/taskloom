//! The plans domain. One plan is one aggregate, keyed by a harp id as a
//! task is: its title, its [`PlanStatus`], the work-root-relative path of
//! its markdown text and that text's content hash, and its typed links,
//! `governs` (plan to task, many) and `supersedes` (plan to plan).
//!
//! [`PlanAggregate`] is the aggregate angzarr-embedded runs, its commands
//! and events the messages of `proto/ctxloom/plans/v1/plans.proto`
//! (angzarr-build's markup layer): each of its handlers decides its command
//! against the plan the stream folds to, and [`apply`] folds each event
//! into a [`PlanState`]. A refusal is a [`PlanRejection`]: a code and its
//! args, its English in the en-US catalog ([`EN_US`]). [`check`] runs, for
//! a caller, the rules a command must keep whatever its stream holds.
//!
//! What one stream cannot see is not decided here: whether a linked task or
//! plan exists, and whether a supersede closes a cycle, are the
//! application's checks (it reads those streams first), because a domain
//! reads no other stream and no projection.

mod generated {
    // The generated layer's command and event sets (`PlanCommand`,
    // `PlanEvent`) are the plan's payloads (`messages`); this crate's own
    // `PlanCommand` and `PlanEvent` below are converted to and from them
    // (`wire.rs`).
    #![allow(dead_code, unused_imports)]
    include!(concat!(env!("OUT_DIR"), "/ctxloom.plans.v1.domain.rs"));
}

/// The plan's commands and events as the proto's messages: the generated
/// layer's enums, each a `Payload`, which a book carries
/// (`CommandBook::command_of(cover, &messages::PlanCommand::from(&command))`)
/// and a read model decodes (`PlanEvent::from(messages::PlanEvent::from_any(any)?)`).
pub mod messages {
    pub use crate::generated::{PlanCommand, PlanEvent};
}
mod aggregate;
mod rejection;
mod wire;

use std::collections::BTreeSet;

pub use aggregate::PlanAggregate;
#[cfg(any(test, feature = "test-support"))]
pub use rejection::diagnostic_samples;
pub use rejection::{PlanRejection, EN_US};

/// The plans domain's name: every plan stream's `Cover.domain`.
pub const PLAN_DOMAIN: &str = "plans";

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
        matches!(
            self,
            PlanStatus::Superseded | PlanStatus::Done | PlanStatus::Abandoned
        )
    }

    /// Whether `plan status` may move a plan from `self` to `to`.
    pub fn may_become(self, to: PlanStatus) -> bool {
        use PlanStatus::{Abandoned, Active, Done, Draft};
        matches!(
            (self, to),
            (Draft, Active | Abandoned) | (Active, Done | Abandoned) | (Done | Abandoned, Active)
        )
    }

    /// The name the CLI and JSON use: `draft`, `active`, ...
    pub fn name(self) -> &'static str {
        match self {
            PlanStatus::Draft => "draft",
            PlanStatus::Active => "active",
            PlanStatus::Superseded => "superseded",
            PlanStatus::Done => "done",
            PlanStatus::Abandoned => "abandoned",
        }
    }

    /// The status `name` spells, any case.
    pub fn parse(name: &str) -> Option<PlanStatus> {
        PlanStatus::ALL
            .into_iter()
            .find(|status| status.name().eq_ignore_ascii_case(name))
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
    /// The text's path, relative to the work root (`/`-separated): a plan
    /// `plan import` found outside it is copied in, and this is the copy's.
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
    /// The text's hash as it now is, recorded when a read finds the file
    /// changed.
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

/// Whether `command` keeps the rules the aggregate holds it to by what it
/// carries alone, whatever its stream holds: an add's title, initial status
/// and hash, a rehash's hash, a status change's not naming Superseded. The
/// aggregate holds every command to these same rules; a caller runs them
/// first, so a command they refuse is refused before any file is written
/// for it.
pub fn check(command: &PlanCommand) -> Result<(), PlanRejection> {
    match command {
        PlanCommand::Add(add) => carried(add),
        PlanCommand::Rehash(hash) => well_formed(hash),
        PlanCommand::SetStatus(PlanStatus::Superseded) => {
            Err(PlanRejection::SupersededBySupersedeOnly)
        }
        _ => Ok(()),
    }
}

/// An add's own fields: a title, a status a plan starts in, a sha-256.
pub(crate) fn carried(add: &AddPlan) -> Result<(), PlanRejection> {
    if add.title.trim().is_empty() {
        return Err(PlanRejection::EmptyTitle);
    }
    if !matches!(add.status, PlanStatus::Draft | PlanStatus::Active) {
        return Err(PlanRejection::InitialStatus(add.status));
    }
    well_formed(&add.content_hash)
}

/// What `plan status` moving a plan `from` its status to `to` moves it to:
/// never Superseded (only a supersede link sets it), never the status it
/// holds, and only as [`PlanStatus::may_become`] allows.
pub(crate) fn status_set(from: PlanStatus, to: PlanStatus) -> Result<PlanStatus, PlanRejection> {
    if to == PlanStatus::Superseded {
        return Err(PlanRejection::SupersededBySupersedeOnly);
    }
    if to == from {
        return Err(PlanRejection::StatusUnchanged(from));
    }
    if !from.may_become(to) {
        return Err(PlanRejection::Transition { from, to });
    }
    Ok(to)
}

/// Whether `hash` is a sha-256 as the store records one: 64 lowercase hex
/// digits.
pub fn well_formed(hash: &str) -> Result<(), PlanRejection> {
    let hex = |c: char| c.is_ascii_digit() || ('a'..='f').contains(&c);
    match hash.len() == 64 && hash.chars().all(hex) {
        true => Ok(()),
        false => Err(PlanRejection::BadHash(hash.to_string())),
    }
}

/// Fold `event` into `state`. Pure.
pub fn apply(state: &mut PlanState, event: &PlanEvent) {
    match event {
        PlanEvent::PlanAdded(add) => {
            state.added = true;
            state.harp_id = add.harp_id.clone();
            state.title = add.title.clone();
            state.status = add.status;
            state.path = add.path.clone();
            state.content_hash = add.content_hash.clone();
            state.origin_session = add.origin_session.clone();
        }
        PlanEvent::StatusChanged { to, .. } => state.status = *to,
        PlanEvent::Governed(task) => {
            state.governs.insert(task.clone());
        }
        PlanEvent::Ungoverned(task) => {
            state.governs.remove(task);
        }
        PlanEvent::Superseded(plan) => {
            state.supersedes.insert(plan.clone());
        }
        PlanEvent::Unsuperseded(plan) => {
            state.supersedes.remove(plan);
        }
        PlanEvent::MarkedSuperseded { by, .. } => {
            state.status = PlanStatus::Superseded;
            state.superseded_by = by.clone();
        }
        PlanEvent::Reinstated => {
            state.status = PlanStatus::Active;
            state.superseded_by.clear();
        }
        PlanEvent::Rehashed(hash) => state.content_hash = hash.clone(),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
