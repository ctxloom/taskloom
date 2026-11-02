//! What every change needs: its project ([`Projects`]) and the kernel its
//! commands run through ([`Runner`], kernel-runner's, which retries them
//! and gathers the dead letters they answer).
//!
//! **Each command is decided once, by the kernel.** A command is sent to
//! its plan's stream, and the plan aggregate decides it against the stream
//! as the kernel loads it; its refusal comes back typed, the plan's own
//! [`PlanRejection`]. A write landing between the kernel's load and its
//! write is the kernel's race to run again, on the new head; a store that
//! could not say is the kernel's to retry, under kernel-runner's
//! `KERNEL_RETRY` over the `Delay` and `Jitter` ports, as the tasks'
//! commands are; one it hands back has had its retries, and is given up
//! on. A retry re-sends the same command id, so an attempt that landed
//! after all is answered DUPLICATE.

use angzarr_embedded::v1::CommandBook;
use angzarr_embedded::CommandBookExt;
use kernel_runner::{Refusal, Runner};
use plans::{PlanAggregate, PlanCommand, PlanRejection};
use ports::{Clock, CommandIds, Delay, Jitter, Project, TaskStores};

use crate::project::{failed, plan_cover, Projects};
use crate::PlanError;

/// A change's way to its project and the kernel, borrowed from the
/// service for one call.
pub(crate) struct Writes<'s, I, S, C, D> {
    pub projects: Projects<'s, I, S>,
    pub runner: Runner<'s, C, D>,
}

impl<I, S, C, D> Writes<'_, I, S, C, D>
where
    S: TaskStores,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    /// Run `command` on the plan `harp_id` in `project`'s store.
    pub(crate) async fn commit(
        &mut self,
        project: &Project,
        harp_id: &str,
        command: PlanCommand,
    ) -> Result<(), PlanError> {
        let store = self.projects.stores.open(project).await.map_err(failed)?;
        let book = CommandBook::command_of(
            plan_cover(harp_id),
            &plans::messages::PlanCommand::from(&command),
        );
        let command_id = self.runner.clock.command_id();
        match self
            .runner
            .run::<PlanAggregate, PlanRejection, _>(store, book, &command_id)
            .await?
        {
            Ok(_) => Ok(()),
            // The plan's own rejection, typed; one the router made on its
            // behalf, as reported.
            Err(Refusal {
                rejection: Some(rejection),
                ..
            }) => Err(PlanError::Rejected(rejection)),
            Err(Refusal { report, .. }) => Err(PlanError::Failed(report)),
        }
    }
}
