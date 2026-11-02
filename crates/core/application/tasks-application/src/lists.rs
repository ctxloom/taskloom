//! The read model a service keeps, one per project: the task list, a
//! projector on angzarr-embedded's port, caught up from the project's
//! store whenever it is read (kernel-runner's [`ProjectLists`], as every
//! context's list is).

use angzarr_embedded::{Processed, ReadStore, Registry, Store};
use kernel_runner::ProjectLists;
use task_lists::{TaskList, TaskLists};

use crate::error::AppError;
use crate::{Project, Row, Task};

/// Each project's task list, by project id, as last caught up.
pub(crate) struct Lists {
    tasks: ProjectLists<TaskLists>,
}

impl Default for Lists {
    fn default() -> Self {
        Lists {
            tasks: ProjectLists::new(TaskList::catch_up),
        }
    }
}

impl Lists {
    /// What `read` reads of `project`'s task list, caught up from `store`
    /// (its store, under `registry`, the tasks domain's) now.
    pub(crate) async fn tasks<S: ReadStore + Store + Processed, R>(
        &mut self,
        project: &Project,
        store: &S,
        registry: &Registry,
        read: impl FnOnce(&TaskList) -> R,
    ) -> Result<R, AppError> {
        let lists = self.tasks.caught_up(project, store, registry).await;
        Ok(read(&lists.map_err(AppError::Failed)?.list()))
    }

    /// The live tasks of `project`'s task list, caught up from `store`
    /// now, in the order they were added.
    pub(crate) async fn rows<S: ReadStore + Store + Processed>(
        &mut self,
        project: &Project,
        store: &S,
        registry: &Registry,
    ) -> Result<Vec<Row>, AppError> {
        self.tasks(project, store, registry, |list| rows_in(list, project))
            .await
    }
}

/// The live tasks of `project`'s `list` in the order they were added.
fn rows_in(list: &task_lists::TaskList, project: &Project) -> Vec<Row> {
    let mut tasks: Vec<Task> = list.entries().into_iter().map(Task::of).collect();
    tasks.sort_by(|a, b| {
        let added = |t: &Task| (t.created_at.seconds, t.created_at.nanos);
        added(a)
            .cmp(&added(b))
            .then_with(|| a.harp_id.cmp(&b.harp_id))
    });
    tasks
        .into_iter()
        .map(|task| Row {
            project: project.clone(),
            task,
        })
        .collect()
}
