//! `plan status`, `link` and `unlink`: a change to a plan that exists
//! ([`Changing`]).
//!
//! **A supersede is two writes**, one on each plan's stream: `Supersede` on
//! the superseding plan, then `MarkSuperseded` on its target (an unlink,
//! `Unsupersede` then `Reinstate`). No order crosses streams. Each half is
//! checked against what the other stream holds, so a link or unlink left
//! half done is finished by running it again. Two supersedes of one target
//! racing can both pass their check (the design's flag): the second
//! target's write is refused, and its first half is left for an unlink.

use plans::{PlanCommand, PlanRejection, PlanStatus};
use ports::{
    Clock, CommandIds, Delay, Jitter, PlanFiles, Project, ProjectIdentity, TaskStores, Tasks,
};

use crate::tree::Tree;
use crate::{PlanError, PlanView};

/// A typed link, as `plan link` and `plan unlink` name it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Link {
    Governs(String),
    Supersedes(String),
}

/// A change over the ports it needs: the project, the kernel and the work
/// tree ([`Tree`]), and the tasks a `governs` link must name. Borrowed
/// from the service for one call.
pub(crate) struct Changing<'s, I, S, F, T, C, D> {
    pub tree: Tree<'s, I, S, F, C, D>,
    pub tasks: &'s mut T,
}

impl<I, S, F, T, C, D> Changing<'_, I, S, F, T, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    F: PlanFiles,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    pub(crate) async fn set_status(
        &mut self,
        plan: &str,
        status: PlanStatus,
    ) -> Result<PlanView, PlanError> {
        let project = self.tree.writes.projects.holding(plan).await?;
        self.tree
            .writes
            .commit(&project, plan, PlanCommand::SetStatus(status))
            .await?;
        self.tree.view(&project, plan).await
    }

    /// `plan` supersedes `target`: both exist, the link closes no cycle,
    /// and no other plan supersedes `target`. Two writes, the second on the
    /// target; a link left half done (the first landed) is finished.
    async fn superseding(
        &mut self,
        project: &Project,
        plan: &str,
        target: String,
    ) -> Result<(), PlanError> {
        if target == plan {
            return Err(PlanError::Rejected(PlanRejection::SupersedesItself));
        }
        let writes = &mut self.tree.writes;
        let list = writes.projects.plan_list(project).await?;
        let (Some(superseding), Some(superseded)) = (list.get(plan), list.get(&target)) else {
            return Err(PlanError::NoSuchPlan(target));
        };
        if list.supersedes_reaches(&target, plan) {
            return Err(PlanError::SupersedeCycle {
                plan: plan.to_string(),
                target,
            });
        }
        let linked = superseding.supersedes.contains(&target);
        let marked_by = superseded.superseded_by.clone();
        match (linked, marked_by.as_str()) {
            (true, by) if by == plan => {
                return Err(PlanError::Rejected(PlanRejection::AlreadySupersedes(
                    target,
                )))
            }
            (_, "") => {}
            (_, by) if by != plan => {
                return Err(PlanError::AlreadySuperseded {
                    plan: target,
                    by: by.to_string(),
                })
            }
            _ => {}
        }
        if !linked {
            writes
                .commit(project, plan, PlanCommand::Supersede(target.clone()))
                .await?;
        }
        if marked_by.is_empty() {
            writes
                .commit(
                    project,
                    &target,
                    PlanCommand::MarkSuperseded(plan.to_string()),
                )
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn unlink(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        let writes = &mut self.tree.writes;
        let project = writes.projects.holding(plan).await?;
        match link {
            Link::Governs(task) => {
                writes
                    .commit(&project, plan, PlanCommand::Ungovern(task))
                    .await?;
            }
            Link::Supersedes(target) => {
                let linked = writes
                    .projects
                    .state(&project, plan)
                    .await?
                    .is_some_and(|p| p.supersedes.contains(&target));
                let marked = writes
                    .projects
                    .state(&project, &target)
                    .await?
                    .is_some_and(|t| t.superseded_by == plan);
                if !linked && !marked {
                    return Err(PlanError::Rejected(PlanRejection::NotSuperseded(target)));
                }
                if linked {
                    writes
                        .commit(&project, plan, PlanCommand::Unsupersede(target.clone()))
                        .await?;
                }
                if marked {
                    writes
                        .commit(&project, &target, PlanCommand::Reinstate)
                        .await?;
                }
            }
        }
        self.tree.view(&project, plan).await
    }
}

impl<I, S, F, T, C, D> Changing<'_, I, S, F, T, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    F: PlanFiles,
    T: Tasks,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    pub(crate) async fn link(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        let project = self.tree.writes.projects.holding(plan).await?;
        match link {
            Link::Governs(task) => {
                if !self.tasks.exists(&task).await.map_err(PlanError::Tasks)? {
                    return Err(PlanError::NoSuchTask(task));
                }
                self.tree
                    .writes
                    .commit(&project, plan, PlanCommand::Govern(task))
                    .await?;
            }
            Link::Supersedes(target) => self.superseding(&project, plan, target).await?,
        }
        self.tree.view(&project, plan).await
    }
}
