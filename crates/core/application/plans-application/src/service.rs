//! [`PlanService`]: the ports the use cases run over, owned once, and the
//! composition of the use-case units behind the traits a driving adapter
//! calls ([`Plans`], [`Governing`]). Each call lends its unit only the
//! ports it uses ([`Parts`]); the unit is the use case.

use std::collections::BTreeMap;
use std::future::Future;

use angzarr_embedded::{DeadLetter, OnRetryable, Registry};
use kernel_runner::{ProjectLists, Runner, KERNEL_RETRY};
use plan_lists::{PlanList, PlanLists};
use plans::{PlanAggregate, PlanStatus};
use ports::{
    Clock, CommandIds, Delay, Jitter, Mint, PlanFiles, Project, ProjectIdentity, TaskStores, Tasks,
};

use crate::add::Adding;
use crate::change::Changing;
use crate::check::Checking;
use crate::import::Importing;
use crate::project::Projects;
use crate::read::Reading;
use crate::tree::Tree;
use crate::write::Writes;
use crate::{CheckReport, ImportReport, Link, NewPlan, PlanError, PlanQuery, PlanText, PlanView};

/// The plans use cases, as a driving adapter reaches them. Their futures
/// are not promised `Send` (see [`ports::Tasks`]).
pub trait Plans {
    fn add(&mut self, new: NewPlan) -> impl Future<Output = Result<PlanView, PlanError>>;
    fn set_status(
        &mut self,
        plan: &str,
        status: PlanStatus,
    ) -> impl Future<Output = Result<PlanView, PlanError>>;
    fn link(&mut self, plan: &str, link: Link)
        -> impl Future<Output = Result<PlanView, PlanError>>;
    fn unlink(
        &mut self,
        plan: &str,
        link: Link,
    ) -> impl Future<Output = Result<PlanView, PlanError>>;
    /// The plans `query` selects, by harp id; each file read, and its new
    /// hash recorded where it changed.
    fn list(&mut self, query: &PlanQuery)
        -> impl Future<Output = Result<Vec<PlanView>, PlanError>>;
    /// The plan `id_or_path` names: a harp id, or a path (relative, or
    /// absolute as `plan list` gives it), and its text.
    fn show(&mut self, id_or_path: &str) -> impl Future<Output = Result<PlanText, PlanError>>;
    /// Every recorded file read (its new hash recorded where it changed),
    /// and what is missing or unregistered reported.
    fn check(&mut self) -> impl Future<Output = Result<CheckReport, PlanError>>;
    /// Every task's `ctxloom:plan` tags into plans and governs links, the
    /// migrated tags retired. `dry_run` reports and changes nothing.
    /// Running it again adds nothing new.
    fn import(&mut self, dry_run: bool) -> impl Future<Output = Result<ImportReport, PlanError>>;
    /// The dead letters the kernel's calls answered since the last call:
    /// almost certainly bugs, for the driving adapter to say loudly.
    // Equivalent: `vec![]` is the empty list this returns.
    #[cfg_attr(test, mutants::skip)]
    fn dead_letters(&mut self) -> Vec<DeadLetter> {
        Vec::new()
    }
}

/// The plans governing tasks, as a frontend showing a task names them
/// (`taskloom show`). The tasks know no plan: plans are downstream of the
/// tasks, whose ids their links name, so which plans govern a task is the
/// plans' to answer, and the frontend puts the two together.
pub trait Governing {
    /// The plans governing each of `tasks` (the project a task is in, and
    /// its harp id), by harp id, in `tasks`' order: each project's plan
    /// list caught up once, from the store its tasks are in; none where
    /// the project has no store.
    fn governing(
        &mut self,
        tasks: &[(Project, String)],
    ) -> impl Future<Output = Result<Vec<Vec<String>>, PlanError>>;
}

/// The registry the plans domain runs under: the plan aggregate.
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    registry
        .aggregate::<PlanAggregate>(PlanAggregate::default)
        .expect("the plan domain, once");
    registry
}

/// The ports the use cases run over, owned once, and what they share
/// across calls: `identity` resolves the project (the one `project` pins,
/// if any), `stores` opens its store, `files` reaches the work tree,
/// `tasks` the project's tasks and `mint` new ids; `clock` stamps commands
/// and mints their ids; `delay` waits, with jitter, between retries. Each
/// call lends its use case only the ports it uses.
pub struct PlanService<I, S, F, T, M, C, D> {
    pub identity: I,
    pub stores: S,
    pub files: F,
    pub tasks: T,
    pub mint: M,
    pub clock: C,
    pub delay: D,
    /// How the kernel retries a command ([`KERNEL_RETRY`]).
    pub retry: OnRetryable,
    /// `--project`: the project to act on, overriding the work root's.
    pub project: Option<String>,
    registry: Registry,
    /// Each project's plan list, kept caught up across the service's
    /// reads (kernel-runner's [`ProjectLists`]).
    lists: ProjectLists<PlanLists>,
    /// The dead letters the use cases' kernel calls answered, kept until
    /// taken ([`Plans::dead_letters`]).
    dead_letters: Vec<DeadLetter>,
}

impl<I, S, F, T, M, C, D> PlanService<I, S, F, T, M, C, D> {
    pub fn new(identity: I, stores: S, files: F, tasks: T, mint: M, clock: C, delay: D) -> Self {
        PlanService {
            identity,
            stores,
            files,
            tasks,
            mint,
            clock,
            delay,
            retry: KERNEL_RETRY,
            project: None,
            registry: registry(),
            lists: ProjectLists::new(PlanList::catch_up),
            dead_letters: Vec::new(),
        }
    }

    /// The same service, acting on `project` when one is given.
    pub fn pinned(self, project: Option<String>) -> Self {
        PlanService { project, ..self }
    }

    /// The ports and shared state, each lent apart, for one call.
    fn parts(&mut self) -> Parts<'_, I, S, F, T, M, C, D> {
        let projects = Projects {
            identity: &mut self.identity,
            stores: &mut self.stores,
            pinned: self.project.as_deref(),
            registry: &self.registry,
            lists: &mut self.lists,
        };
        let runner = Runner {
            registry: &self.registry,
            clock: &mut self.clock,
            delay: &mut self.delay,
            retry: self.retry,
            dead_letters: &mut self.dead_letters,
        };
        Parts {
            tree: Tree {
                writes: Writes { projects, runner },
                files: &self.files,
            },
            tasks: &mut self.tasks,
            mint: &self.mint,
        }
    }

    fn adding(&mut self) -> Adding<'_, I, S, F, T, M, C, D> {
        let Parts { tree, tasks, mint } = self.parts();
        Adding { tree, tasks, mint }
    }

    fn changing(&mut self) -> Changing<'_, I, S, F, T, C, D> {
        let Parts { tree, tasks, .. } = self.parts();
        Changing { tree, tasks }
    }

    fn reading(&mut self) -> Reading<'_, I, S, F, C, D> {
        Reading {
            tree: self.parts().tree,
        }
    }

    fn checking(&mut self) -> Checking<'_, I, S, F, T, C, D> {
        let Parts { tree, tasks, .. } = self.parts();
        Checking { tree, tasks }
    }

    fn importing(&mut self) -> Importing<'_, I, S, F, T, M, C, D> {
        let Parts { tree, tasks, mint } = self.parts();
        Importing { tree, tasks, mint }
    }
}

/// A [`PlanService`]'s ports and shared state, each borrowed apart, from
/// which a call's use case takes what it uses.
struct Parts<'s, I, S, F, T, M, C, D> {
    tree: Tree<'s, I, S, F, C, D>,
    tasks: &'s mut T,
    mint: &'s M,
}

impl<I, S, F, T, M, C, D> Plans for PlanService<I, S, F, T, M, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    F: PlanFiles,
    T: Tasks,
    M: Mint,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    async fn add(&mut self, new: NewPlan) -> Result<PlanView, PlanError> {
        self.adding().add(new).await
    }

    async fn set_status(&mut self, plan: &str, status: PlanStatus) -> Result<PlanView, PlanError> {
        self.changing().set_status(plan, status).await
    }

    async fn link(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.changing().link(plan, link).await
    }

    async fn unlink(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.changing().unlink(plan, link).await
    }

    async fn list(&mut self, query: &PlanQuery) -> Result<Vec<PlanView>, PlanError> {
        self.reading().list(query).await
    }

    async fn show(&mut self, id_or_path: &str) -> Result<PlanText, PlanError> {
        self.reading().show(id_or_path).await
    }

    async fn check(&mut self) -> Result<CheckReport, PlanError> {
        self.checking().check().await
    }

    async fn import(&mut self, dry_run: bool) -> Result<ImportReport, PlanError> {
        self.importing().import(dry_run).await
    }

    fn dead_letters(&mut self) -> Vec<DeadLetter> {
        let mut letters = std::mem::take(&mut self.dead_letters);
        letters.extend(self.tasks.dead_letters());
        letters
    }
}

/// Which plans govern a task reads the plan list alone: the store each
/// task's project is in, under the plans' registry.
impl<I, S, F, T, M, C, D> Governing for PlanService<I, S, F, T, M, C, D>
where
    S: TaskStores,
{
    async fn governing(
        &mut self,
        tasks: &[(Project, String)],
    ) -> Result<Vec<Vec<String>>, PlanError> {
        let projects = &mut self.parts().tree.writes.projects;
        let mut by_project: BTreeMap<String, PlanList> = BTreeMap::new();
        let mut governing = Vec::new();
        for (project, harp_id) in tasks {
            if !by_project.contains_key(&project.id) {
                let list = projects.plan_list(project).await?;
                by_project.insert(project.id.clone(), list);
            }
            let plans = by_project[&project.id]
                .governing(harp_id)
                .into_iter()
                .map(|plan| plan.harp_id.clone())
                .collect();
            governing.push(plans);
        }
        Ok(governing)
    }
}

#[cfg(test)]
#[path = "service.test.rs"]
mod tests;
