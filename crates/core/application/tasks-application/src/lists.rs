//! The read model a service keeps, one per project: the task list, a
//! projector on angzarr-embedded's port, caught up from the project's
//! store whenever it is read.
//!
//! A list's positions are held in memory, with the list: the first read
//! of a project in a process folds every stream of its store from its
//! start (the lists are not persisted, so there is nothing to resume
//! from), and every later read in the same process only what was written
//! since (a long-lived service's: a watch's, or ctxloom's). A catch-up reads
//! one moment of the store, per stream only; a stream purged, or purged
//! and written again, is told by the catch-up.

use angzarr_diagnostic::Report;
use angzarr_embedded::{CatchUp, ListPositions, ProjectorHandler, ReadStore, Registry, Store};
use std::collections::BTreeMap;
use task_lists::{TaskList, TaskLists};

use crate::error::AppError;
use crate::Project;

/// Each project's task list, by project id, as last caught up.
#[derive(Default)]
pub(crate) struct Lists {
    tasks: BTreeMap<String, CatchUp<TaskLists>>,
}

impl Lists {
    /// What `read` reads of `project`'s task list, caught up from `store`
    /// (its store, under `registry`, the tasks domain's) now.
    pub(crate) async fn tasks<S: ReadStore + Store + ListPositions, R>(
        &mut self,
        project: &Project,
        store: &S,
        registry: &Registry,
        read: impl FnOnce(&TaskList) -> R,
    ) -> Result<R, AppError> {
        let held = self.tasks.entry(project.id.clone());
        let catch_up = held.or_insert_with(TaskList::catch_up);
        caught_up(catch_up, store, registry).await?;
        Ok(read(&catch_up.projector().list()))
    }
}

/// One step of `catch_up` over `store`: every stream handed what it is
/// owed, or the first stream that failed as the read's failure (held, so
/// the next read hands it again).
async fn caught_up<P: ProjectorHandler, S: ReadStore + Store + ListPositions>(
    catch_up: &mut CatchUp<P>,
    store: &S,
    registry: &Registry,
) -> Result<(), AppError> {
    let failed = |error: &dyn angzarr_diagnostic::Diagnostic| AppError::Failed(Report::of(error));
    let caught = catch_up
        .step(store, registry.catalog())
        .await
        .map_err(|e| failed(&e))?;
    match caught.failures().first() {
        Some((_, error)) => Err(failed(error)),
        None => Ok(()),
    }
}
