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
    if let PlanCommand::Add(add) = command {
        return added(state, add).map(|event| vec![event]);
    }
    if !state.added {
        return Err(PlanRejection::NoSuchPlan);
    }
    let event = match command {
        PlanCommand::Add(_) => unreachable!("an add is decided above"),
        PlanCommand::SetStatus(to) => status_set(state, *to)?,
        PlanCommand::Govern(task) => match state.governs.contains(task) {
            true => return Err(PlanRejection::AlreadyGoverns(task.clone())),
            false => PlanEvent::Governed(task.clone()),
        },
        PlanCommand::Ungovern(task) => match state.governs.contains(task) {
            true => PlanEvent::Ungoverned(task.clone()),
            false => return Err(PlanRejection::NotGoverned(task.clone())),
        },
        PlanCommand::Supersede(plan) if *plan == state.harp_id => {
            return Err(PlanRejection::SupersedesItself)
        }
        PlanCommand::Supersede(plan) => match state.supersedes.contains(plan) {
            true => return Err(PlanRejection::AlreadySupersedes(plan.clone())),
            false => PlanEvent::Superseded(plan.clone()),
        },
        PlanCommand::Unsupersede(plan) => match state.supersedes.contains(plan) {
            true => PlanEvent::Unsuperseded(plan.clone()),
            false => return Err(PlanRejection::NotSuperseded(plan.clone())),
        },
        PlanCommand::MarkSuperseded(by) => match state.status {
            PlanStatus::Superseded => {
                return Err(PlanRejection::Transition {
                    from: PlanStatus::Superseded,
                    to: PlanStatus::Superseded,
                })
            }
            from => PlanEvent::MarkedSuperseded {
                by: by.clone(),
                from,
            },
        },
        PlanCommand::Reinstate => match state.status {
            PlanStatus::Superseded => PlanEvent::Reinstated,
            from => {
                return Err(PlanRejection::Transition {
                    from,
                    to: PlanStatus::Active,
                })
            }
        },
        PlanCommand::Rehash(hash) => {
            well_formed(hash)?;
            match *hash == state.content_hash {
                true => return Err(PlanRejection::HashUnchanged),
                false => PlanEvent::Rehashed(hash.clone()),
            }
        }
    };
    Ok(vec![event])
}

/// What an add to `state` records: refused on a stream holding a plan, and
/// for a blank title, a status a plan cannot start in, or a malformed hash.
fn added(state: &PlanState, add: &AddPlan) -> Result<PlanEvent, PlanRejection> {
    if state.added {
        return Err(PlanRejection::AlreadyAdded);
    }
    if add.title.trim().is_empty() {
        return Err(PlanRejection::EmptyTitle);
    }
    if !matches!(add.status, PlanStatus::Draft | PlanStatus::Active) {
        return Err(PlanRejection::InitialStatus(add.status));
    }
    well_formed(&add.content_hash)?;
    Ok(PlanEvent::PlanAdded(add.clone()))
}

/// What `plan status` moving `state` to `to` records.
fn status_set(state: &PlanState, to: PlanStatus) -> Result<PlanEvent, PlanRejection> {
    let from = state.status;
    if to == PlanStatus::Superseded {
        return Err(PlanRejection::SupersededBySupersedeOnly);
    }
    if to == from {
        return Err(PlanRejection::StatusUnchanged(from));
    }
    if !from.may_become(to) {
        return Err(PlanRejection::Transition { from, to });
    }
    Ok(PlanEvent::StatusChanged { from, to })
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
