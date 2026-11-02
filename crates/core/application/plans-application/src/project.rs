//! How a use case reaches its project and what the project's store holds
//! of its plans: the project resolved ([`ProjectIdentity`], the one
//! `--project` pins, if any), its store opened ([`TaskStores`]: a
//! project's plans live in the store its tasks do), a plan's stream as the
//! kernel loads it, and the plan list caught up (kernel-runner's
//! [`ProjectLists`]). Every use case reaches its project this way
//! ([`Projects`]); a change goes on to run its commands through the kernel
//! ([`crate::write::Writes`]).

use angzarr_diagnostic::Report;
use angzarr_embedded::v1::Cover;
use angzarr_embedded::{CoverExt, Kernel, KernelError, Registry, StoreError};
use kernel_runner::ProjectLists;
use plan_lists::{PlanList, PlanLists};
use plans::{PlanAggregate, PlanState, PLAN_DOMAIN};
use ports::{Mint, Project, ProjectIdentity, TaskStores};

use crate::PlanError;

/// How many plan ids an add mints before giving up, each refused because a
/// plan already holds it.
pub const HARP_ATTEMPTS: u32 = 100;

/// The stream of the plan `harp_id` names: its harp is its natural key.
pub(crate) fn plan_cover(harp_id: &str) -> Cover {
    Cover::natural(PLAN_DOMAIN, harp_id)
}

/// `error`, the store's, as a use case reports it.
pub(crate) fn failed(error: StoreError) -> PlanError {
    PlanError::Failed(Report::of(&KernelError::Store(error)))
}

/// The ports a project is reached through, the registry its plan streams
/// are read under, and each project's plan list kept caught up across
/// the service's reads: borrowed from the service for one call.
pub(crate) struct Projects<'s, I, S> {
    pub identity: &'s mut I,
    pub stores: &'s mut S,
    /// `--project`: the project to act on, overriding the work root's.
    pub pinned: Option<&'s str>,
    pub registry: &'s Registry,
    pub lists: &'s mut ProjectLists<PlanLists>,
}

impl<I: ProjectIdentity, S> Projects<'_, I, S> {
    /// The project a write goes to: pinned, bound, or minted and bound.
    pub(crate) fn for_write(&mut self) -> Result<Project, PlanError> {
        self.identity
            .for_write(self.pinned)
            .map_err(PlanError::Identity)
    }
}

impl<I, S: TaskStores> Projects<'_, I, S> {
    /// Every plan of `project`'s store, caught up now; none where it has
    /// no store.
    pub(crate) async fn plan_list(&mut self, project: &Project) -> Result<PlanList, PlanError> {
        let Some(store) = self.stores.open_existing(project).await.map_err(failed)? else {
            return Ok(PlanList::new());
        };
        let lists = self.lists.caught_up(project, store, self.registry).await;
        Ok(lists.map_err(PlanError::Failed)?.list().clone())
    }

    /// The plan `harp_id` as its stream holds it now; `None` when it holds
    /// none.
    pub(crate) async fn state(
        &mut self,
        project: &Project,
        harp_id: &str,
    ) -> Result<Option<PlanState>, PlanError> {
        let Some(store) = self.stores.open_existing(project).await.map_err(failed)? else {
            return Ok(None);
        };
        let (plan, _) = Kernel::new(self.registry)
            .load::<PlanAggregate, _>(store, &plan_cover(harp_id))
            .await
            .map_err(|e| PlanError::Failed(Report::of(&e)))?;
        let state = plan.state().state();
        Ok(state.added.then(|| state.clone()))
    }

    /// A fresh plan id from `mint`: one no plan stream of `project` holds.
    pub(crate) async fn fresh_id(
        &mut self,
        mint: &impl Mint,
        project: &Project,
    ) -> Result<String, PlanError> {
        for _ in 0..HARP_ATTEMPTS {
            let harp_id = mint.mint();
            if self.state(project, &harp_id).await?.is_none() {
                return Ok(harp_id);
            }
        }
        Err(PlanError::HarpsExhausted {
            attempts: HARP_ATTEMPTS,
        })
    }
}

impl<I: ProjectIdentity, S: TaskStores> Projects<'_, I, S> {
    /// The project a read is about: `None` where there is none yet, or it
    /// has no store, so no plan.
    pub(crate) async fn for_read(&mut self) -> Result<Option<Project>, PlanError> {
        let Some(project) = self
            .identity
            .for_read(self.pinned)
            .map_err(PlanError::Identity)?
        else {
            return Ok(None);
        };
        let store = self.stores.open_existing(&project).await.map_err(failed)?;
        Ok(store.is_some().then_some(project))
    }

    /// The project holding the plan `plan`, which must exist.
    pub(crate) async fn holding(&mut self, plan: &str) -> Result<Project, PlanError> {
        let missing = || PlanError::NoSuchPlan(plan.to_string());
        let project = self.for_read().await?.ok_or_else(missing)?;
        match self.state(&project, plan).await? {
            Some(_) => Ok(project),
            None => Err(missing()),
        }
    }
}
