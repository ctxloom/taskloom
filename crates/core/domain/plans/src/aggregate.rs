//! The plan aggregate on angzarr-embedded's markup: each command decided
//! by its own handler, against the plan the stream folds to.
//!
//! - the instance holds the folded [`PlanState`], read by
//!   [`PlanAggregate::state`];
//! - one `#[handles_cmd]` per command, deciding it: the event it records,
//!   or why not. Every command but an add needs a plan on the stream;
//! - every refusal is message-only, a [`PlanRejection`] as a
//!   `Failed::Rejected` (FAILED_PRECONDITION): a key (`plans-*`) and its
//!   args, never English. An add whose harp id is not the one its stream
//!   derives from is refused (`plans-not-its-stream`);
//! - one `#[applies]` per event, folding it with [`apply`](crate::apply);
//! - no snapshot: a plan's stream is short, and every load replays it.

use angzarr_embedded::ids;
use angzarr_embedded::markup::{Context, Failed};
use angzarr_embedded_macros::{aggregate, applies, handles_cmd};

use crate::generated::{
    AddPlan, Govern, Governed, MarkSuperseded, MarkedSuperseded, PlanAdded, PlanEvent,
    PlanStatusChanged, Rehash, Rehashed, Reinstate, Reinstated, SetPlanStatus, Supersede,
    Superseded, Ungovern, Ungoverned, Unsupersede, Unsuperseded,
};
use crate::wire::event_of;
use crate::{carried, status_set, well_formed, PlanRejection, PlanState, PlanStatus, PLAN_DOMAIN};

/// The plan aggregate: one plan, folded from its stream.
#[derive(Clone, Debug, Default)]
pub struct PlanAggregate {
    plan: PlanState,
}

impl PlanAggregate {
    /// The folded plan.
    pub fn state(&self) -> &PlanState {
        &self.plan
    }

    /// The plan, if the stream holds one: every command but an add is
    /// refused on an empty stream.
    fn existing(&self) -> Result<&PlanState, PlanRejection> {
        match self.plan.added {
            true => Ok(&self.plan),
            false => Err(PlanRejection::NoSuchPlan),
        }
    }

    fn fold(&mut self, event: PlanEvent) {
        crate::apply(&mut self.plan, &event_of(event));
    }
}

/// Whether the plan `harp_id` belongs in the stream rooted at `root`: the
/// stream is addressed by the root its harp id derives, so the harp id the
/// add records must be that one, or the plan takes another's.
fn belongs(harp_id: &str, root: Option<&[u8]>) -> Result<(), PlanRejection> {
    let derived = ids::compute_root(PLAN_DOMAIN, harp_id).map_err(|e| PlanRejection::NoRoot {
        harp_id: harp_id.to_string(),
        cause: e.to_string(),
    })?;
    match root == Some(&derived.value[..]) {
        true => Ok(()),
        false => Err(PlanRejection::NotItsStream(harp_id.to_string())),
    }
}

// The domain is PLAN_DOMAIN, spelled as the markup takes it, a literal
// (`aggregate.test.rs` holds them to each other).
#[aggregate(domain = "plans")]
impl PlanAggregate {
    /// Create the plan, in the stream its harp id derives: refused on a
    /// stream holding a plan, and for a blank title, a status a plan
    /// cannot start in, or a malformed hash.
    #[handles_cmd(AddPlan)]
    fn add_plan(&self, cmd: AddPlan, ctx: &Context) -> Result<PlanAdded, Failed<PlanRejection>> {
        // An add to a stream holding a plan is refused as that first.
        if self.plan.added {
            return Err(PlanRejection::AlreadyAdded.into());
        }
        let root = ctx
            .command
            .cover
            .as_ref()
            .and_then(|cover| cover.root.as_ref())
            .map(|root| &root.value[..]);
        belongs(&cmd.harp_id, root)?;
        let add = crate::AddPlan::from(cmd);
        carried(&add)?;
        Ok(add.into())
    }

    /// `plan status`: any status but Superseded, by the transition table.
    #[handles_cmd(SetPlanStatus)]
    fn set_plan_status(
        &self,
        cmd: SetPlanStatus,
        ctx: &Context,
    ) -> Result<PlanStatusChanged, Failed<PlanRejection>> {
        let _ = ctx;
        let from = self.existing()?.status;
        let to = status_set(from, cmd.status.into())?;
        Ok(PlanStatusChanged {
            from_status: from.into(),
            to_status: to.into(),
        })
    }

    /// `plan link --governs`: the application has seen the task exists.
    #[handles_cmd(Govern)]
    fn govern(&self, cmd: Govern, ctx: &Context) -> Result<Governed, Failed<PlanRejection>> {
        let _ = ctx;
        match self.existing()?.governs.contains(&cmd.task) {
            true => Err(PlanRejection::AlreadyGoverns(cmd.task).into()),
            false => Ok(Governed { task: cmd.task }),
        }
    }

    /// `plan unlink --governs`.
    #[handles_cmd(Ungovern)]
    fn ungovern(&self, cmd: Ungovern, ctx: &Context) -> Result<Ungoverned, Failed<PlanRejection>> {
        let _ = ctx;
        match self.existing()?.governs.contains(&cmd.task) {
            true => Ok(Ungoverned { task: cmd.task }),
            false => Err(PlanRejection::NotGoverned(cmd.task).into()),
        }
    }

    /// `plan link --supersedes`, on the superseding plan: the application
    /// has seen the plan exists and that no cycle closes.
    #[handles_cmd(Supersede)]
    fn supersede(
        &self,
        cmd: Supersede,
        ctx: &Context,
    ) -> Result<Superseded, Failed<PlanRejection>> {
        let _ = ctx;
        let plan = self.existing()?;
        if cmd.plan == plan.harp_id {
            return Err(PlanRejection::SupersedesItself.into());
        }
        match plan.supersedes.contains(&cmd.plan) {
            true => Err(PlanRejection::AlreadySupersedes(cmd.plan).into()),
            false => Ok(Superseded { plan: cmd.plan }),
        }
    }

    /// `plan unlink --supersedes`, on the superseding plan.
    #[handles_cmd(Unsupersede)]
    fn unsupersede(
        &self,
        cmd: Unsupersede,
        ctx: &Context,
    ) -> Result<Unsuperseded, Failed<PlanRejection>> {
        let _ = ctx;
        match self.existing()?.supersedes.contains(&cmd.plan) {
            true => Ok(Unsuperseded { plan: cmd.plan }),
            false => Err(PlanRejection::NotSuperseded(cmd.plan).into()),
        }
    }

    /// On the superseded plan, sent by the application after a supersede:
    /// it becomes Superseded by the plan named.
    #[handles_cmd(MarkSuperseded)]
    fn mark_superseded(
        &self,
        cmd: MarkSuperseded,
        ctx: &Context,
    ) -> Result<MarkedSuperseded, Failed<PlanRejection>> {
        let _ = ctx;
        match self.existing()?.status {
            PlanStatus::Superseded => Err(PlanRejection::Transition {
                from: PlanStatus::Superseded,
                to: PlanStatus::Superseded,
            }
            .into()),
            from => Ok(MarkedSuperseded {
                by: cmd.by,
                from_status: from.into(),
            }),
        }
    }

    /// On the superseded plan, after an unsupersede: it is Active again.
    #[handles_cmd(Reinstate)]
    fn reinstate(
        &self,
        cmd: Reinstate,
        ctx: &Context,
    ) -> Result<Reinstated, Failed<PlanRejection>> {
        let (Reinstate {}, _) = (cmd, ctx);
        match self.existing()?.status {
            PlanStatus::Superseded => Ok(Reinstated {}),
            from => Err(PlanRejection::Transition {
                from,
                to: PlanStatus::Active,
            }
            .into()),
        }
    }

    /// The text's hash as it now is, recorded when a read finds the file
    /// changed: refused malformed, or unchanged.
    #[handles_cmd(Rehash)]
    fn rehash(&self, cmd: Rehash, ctx: &Context) -> Result<Rehashed, Failed<PlanRejection>> {
        let _ = ctx;
        let plan = self.existing()?;
        well_formed(&cmd.content_hash)?;
        match cmd.content_hash == plan.content_hash {
            true => Err(PlanRejection::HashUnchanged.into()),
            false => Ok(Rehashed {
                content_hash: cmd.content_hash,
            }),
        }
    }

    #[applies(PlanAdded)]
    fn plan_added(&mut self, event: PlanAdded) {
        self.fold(event.into());
    }

    #[applies(PlanStatusChanged)]
    fn plan_status_changed(&mut self, event: PlanStatusChanged) {
        self.fold(event.into());
    }

    #[applies(Governed)]
    fn governed(&mut self, event: Governed) {
        self.fold(event.into());
    }

    #[applies(Ungoverned)]
    fn ungoverned(&mut self, event: Ungoverned) {
        self.fold(event.into());
    }

    #[applies(Superseded)]
    fn superseded(&mut self, event: Superseded) {
        self.fold(event.into());
    }

    #[applies(Unsuperseded)]
    fn unsuperseded(&mut self, event: Unsuperseded) {
        self.fold(event.into());
    }

    #[applies(MarkedSuperseded)]
    fn marked_superseded(&mut self, event: MarkedSuperseded) {
        self.fold(event.into());
    }

    #[applies(Reinstated)]
    fn reinstated(&mut self, event: Reinstated) {
        self.fold(event.into());
    }

    #[applies(Rehashed)]
    fn rehashed(&mut self, event: Rehashed) {
        self.fold(event.into());
    }
}

// The kernel harness the plan's rules are tested through (`lib.test.rs`
// too).
#[cfg(test)]
#[path = "aggregate.test.rs"]
pub(crate) mod tests;
