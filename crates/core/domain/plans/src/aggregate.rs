//! The plan aggregate on angzarr-embedded's markup: thin handlers over
//! [`decide`] and [`apply`](crate::apply).
//!
//! - the instance holds the folded [`PlanState`], read by
//!   [`PlanAggregate::state`];
//! - one `#[handles_cmd]` per command: the proto's command converted to a
//!   [`PlanCommand`], decided, and its one event converted back;
//! - every refusal is message-only, a [`PlanRejection`] as a
//!   `Failed::Rejected` (FAILED_PRECONDITION): a key (`plans-*`) and its
//!   args, never English. An add whose harp id is not the one its stream
//!   derives from is refused (`plans-not-its-stream`);
//! - one `#[applies]` per event, folding it with [`apply`](crate::apply);
//! - no snapshot: a plan's stream is short, and every load replays it.

use angzarr_embedded::next::ids;
use angzarr_embedded::next::markup::{Context, Failed};
use angzarr_embedded_macros::{aggregate, applies, handles_cmd};

use crate::generated::{
    AddPlan, Govern, Governed, MarkSuperseded, MarkedSuperseded, PlanAdded, PlanCommand, PlanEvent,
    PlanStatusChanged, Rehash, Rehashed, Reinstate, Reinstated, SetPlanStatus, Supersede,
    Superseded, Ungovern, Ungoverned, Unsupersede, Unsuperseded,
};
use crate::wire::{command_of, event_message, event_of};
use crate::{decide, PlanRejection, PlanState, PLAN_DOMAIN};

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

    /// The one event `command` records, as the proto's message.
    fn decided(&self, command: PlanCommand) -> Result<PlanEvent, PlanRejection> {
        let events = decide(&self.plan, &command_of(command))?;
        match events.as_slice() {
            [event] => Ok(event_message(event)),
            _ => unreachable!("decide records one event per command"),
        }
    }

    fn fold(&mut self, event: PlanEvent) {
        crate::apply(&mut self.plan, &event_of(event));
    }
}

/// The event a command's decision records, as the variant it must be.
macro_rules! recorded {
    ($decided:expr, $variant:ident) => {
        match $decided {
            Ok(PlanEvent::$variant(event)) => Ok(event),
            Ok(other) => unreachable!(
                "decide records {other:?} for a command recording {}",
                stringify!($variant)
            ),
            Err(rejection) => Err(Failed::rejected(rejection)),
        }
    };
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
    /// Create the plan, in the stream its harp id derives.
    #[handles_cmd(AddPlan)]
    fn add_plan(&self, cmd: AddPlan, ctx: &Context) -> Result<PlanAdded, Failed<PlanRejection>> {
        let root = ctx
            .command
            .cover
            .as_ref()
            .and_then(|cover| cover.root.as_ref())
            .map(|root| &root.value[..]);
        // An add to a stream holding a plan is refused as that first.
        if !self.plan.added {
            belongs(&cmd.harp_id, root)?;
        }
        recorded!(self.decided(PlanCommand::AddPlan(cmd)), PlanAdded)
    }

    #[handles_cmd(SetPlanStatus)]
    fn set_plan_status(
        &self,
        cmd: SetPlanStatus,
        ctx: &Context,
    ) -> Result<PlanStatusChanged, Failed<PlanRejection>> {
        let _ = ctx;
        recorded!(
            self.decided(PlanCommand::SetPlanStatus(cmd)),
            PlanStatusChanged
        )
    }

    #[handles_cmd(Govern)]
    fn govern(&self, cmd: Govern, ctx: &Context) -> Result<Governed, Failed<PlanRejection>> {
        let _ = ctx;
        recorded!(self.decided(PlanCommand::Govern(cmd)), Governed)
    }

    #[handles_cmd(Ungovern)]
    fn ungovern(&self, cmd: Ungovern, ctx: &Context) -> Result<Ungoverned, Failed<PlanRejection>> {
        let _ = ctx;
        recorded!(self.decided(PlanCommand::Ungovern(cmd)), Ungoverned)
    }

    #[handles_cmd(Supersede)]
    fn supersede(
        &self,
        cmd: Supersede,
        ctx: &Context,
    ) -> Result<Superseded, Failed<PlanRejection>> {
        let _ = ctx;
        recorded!(self.decided(PlanCommand::Supersede(cmd)), Superseded)
    }

    #[handles_cmd(Unsupersede)]
    fn unsupersede(
        &self,
        cmd: Unsupersede,
        ctx: &Context,
    ) -> Result<Unsuperseded, Failed<PlanRejection>> {
        let _ = ctx;
        recorded!(self.decided(PlanCommand::Unsupersede(cmd)), Unsuperseded)
    }

    #[handles_cmd(MarkSuperseded)]
    fn mark_superseded(
        &self,
        cmd: MarkSuperseded,
        ctx: &Context,
    ) -> Result<MarkedSuperseded, Failed<PlanRejection>> {
        let _ = ctx;
        recorded!(
            self.decided(PlanCommand::MarkSuperseded(cmd)),
            MarkedSuperseded
        )
    }

    #[handles_cmd(Reinstate)]
    fn reinstate(
        &self,
        cmd: Reinstate,
        ctx: &Context,
    ) -> Result<Reinstated, Failed<PlanRejection>> {
        let _ = ctx;
        recorded!(self.decided(PlanCommand::Reinstate(cmd)), Reinstated)
    }

    #[handles_cmd(Rehash)]
    fn rehash(&self, cmd: Rehash, ctx: &Context) -> Result<Rehashed, Failed<PlanRejection>> {
        let _ = ctx;
        recorded!(self.decided(PlanCommand::Rehash(cmd)), Rehashed)
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

#[cfg(test)]
#[path = "aggregate.test.rs"]
mod tests;
