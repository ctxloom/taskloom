//! What every write needs: its project ([`Projects`]) and the kernel its
//! commands run through ([`Runner`], kernel-runner's, which retries them
//! and gathers the dead letters they answer). [`Writes`] adds the steps
//! the writes share: the store's schema stream seeded before a write is
//! judged by its rules, a declaration on it, a command to a task's stream
//! answered with the task it left, and the command id a request sends.

use angzarr_embedded::v1::{CommandBook, Uuid};
use angzarr_embedded::CommandBookExt;
use kernel_runner::Runner;
use ports::{Clock, CommandIds, Delay, Jitter, Project, TaskStores};
use tasks::schema::Declare;
use tasks::{TaskAggregate, TaskCommand};

use crate::project::Projects;
use crate::rules::{Finding, StoreRules};
use crate::schema::{self, Declared};
use crate::task::{task_after, task_cover};
use crate::{failed, request_command_id, AppError, Tag, Task, Written};

/// A write's way to its project and the kernel, borrowed from the service
/// for one call.
pub(crate) struct Writes<'s, I, S, G, C, D> {
    pub projects: Projects<'s, I, S, G>,
    pub runner: Runner<'s, C, D>,
}

impl<I, S, G, C, D> Writes<'_, I, S, G, C, D>
where
    S: TaskStores,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    /// `project`'s store's rules after seeding its schema stream, where it
    /// has no history ([`schema::seed_in`]).
    pub(crate) async fn seeded(&mut self, project: &Project) -> Result<StoreRules, AppError> {
        let store = self.projects.stores.open(project).await.map_err(failed)?;
        schema::seed_in(&mut self.runner, store).await
    }

    /// `declare` on `project`'s store's schema stream, seeded first: the
    /// tags after it, and whether they changed.
    pub(crate) async fn declaring(
        &mut self,
        project: &Project,
        declare: Declare,
    ) -> Result<Declared, AppError> {
        tasks::schema::check(&declare).map_err(|r| crate::rejected(&r))?;
        let before = self.seeded(project).await?;
        let store = self.projects.stores.open(project).await.map_err(failed)?;
        schema::declare_in(&mut self.runner, store, declare, before).await
    }

    /// What `declare` would leave on `project`'s store's schema stream,
    /// sent nowhere: the tags after it, and whether they would change.
    pub(crate) async fn previewing(
        &mut self,
        project: &Project,
        declare: Declare,
    ) -> Result<Declared, AppError> {
        tasks::schema::check(&declare).map_err(|r| crate::rejected(&r))?;
        let before = self.seeded(project).await?;
        let removed: Vec<tagma_core::Tag> = declare.remove.iter().map(Tag::to_core).collect();
        let kept = before
            .tags()
            .iter()
            .filter(|t| !removed.contains(t))
            .cloned();
        let added = declare.add.iter().map(Tag::to_core);
        let after = StoreRules::of_tags(kept.chain(added));
        Ok(Declared {
            changed: after != before,
            tags: after.export(),
        })
    }

    /// The command id `operation` sends: derived from the request's key
    /// when one is given ([`request_command_id`]), so a retry sends the
    /// same; a fresh one when none is.
    pub(crate) fn command_id(&mut self, operation: &str, request_id: Option<&str>) -> Uuid {
        match request_id {
            Some(key) => request_command_id(operation, key),
            None => self.runner.clock.command_id(),
        }
    }

    /// Run `command` on the task `harp_id` in `project`'s store, under
    /// `command_id`, retried by the kernel ([`Runner`]); the task after it.
    /// A command already applied under `command_id` is answered DUPLICATE
    /// and applies nothing: the task is answered as it stands.
    pub(crate) async fn commit(
        &mut self,
        project: &Project,
        harp_id: &str,
        command: TaskCommand,
        command_id: Uuid,
    ) -> Result<Task, AppError> {
        let store = self.projects.stores.open(project).await.map_err(failed)?;
        let book = CommandBook::command_of(task_cover(harp_id), &command);
        let handled = self
            .runner
            .run::<TaskAggregate, _, _>(store, book, &command_id)
            .await??;
        task_after(store, self.runner.registry, harp_id, &handled).await
    }
}

impl<I, S, G, C, D> Writes<'_, I, S, G, C, D> {
    /// What a write answers with: `task`, written in `project` under
    /// `rules` (which say what a person is shown), the warnings it carries,
    /// and every dead letter the call's kernel calls answered, taken.
    pub(crate) fn written(
        &mut self,
        project: Project,
        task: Task,
        rules: &StoreRules,
        warnings: Vec<Finding>,
    ) -> Written {
        Written {
            project,
            task,
            shown: rules.shown(),
            dead_letters: std::mem::take(self.runner.dead_letters),
            warnings,
        }
    }
}
