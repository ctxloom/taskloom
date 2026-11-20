//! Each project's list, caught up from the project's store whenever it is
//! read, as every taskloom context reads one (the tasks' task list, the
//! plans' plan list): the shared half of a read, which no context owns.
//!
//! A list is a projector on angzarr-embedded's port, run as a [`CatchUp`]
//! whose positions are held in memory, with the list: the first read of a
//! project in a process folds every stream of its store from its start
//! (the lists are not persisted, so there is nothing to resume from), and
//! every later read in the same process only what was written since (a
//! long-lived service's: a watch's, or ctxloom's). A catch-up's step reads
//! one moment of the store, its read unit, purges included: a stream
//! purged, or purged and written again, is told by the catch-up from the
//! covers that same read listed, so a purge landing mid-step is the next
//! step's, as every other write landing then is.

use std::collections::BTreeMap;

use angzarr_diagnostic::Report;
use angzarr_embedded::{CatchUp, Processed, ProjectorHandler, ReadStore, Registry, Store};
use ports::Project;

/// Each project's list `P`, by project id, as last caught up.
pub struct ProjectLists<P> {
    /// A list's catch-up from nothing: a project's first read starts one.
    start: fn() -> CatchUp<P>,
    held: BTreeMap<String, CatchUp<P>>,
}

impl<P> ProjectLists<P> {
    /// No project's list yet; each is started by `start` (the read model's
    /// `catch_up`) on its project's first read.
    pub fn new(start: fn() -> CatchUp<P>) -> Self {
        ProjectLists {
            start,
            held: BTreeMap::new(),
        }
    }
}

impl<P: ProjectorHandler> ProjectLists<P> {
    /// `project`'s list, caught up from `store` (its store, under
    /// `registry`, the list's domain's) now: every stream handed what it
    /// is owed. Or why not: the store's failure, or the first stream the
    /// list failed (held where it was, so the next read hands it again).
    pub async fn caught_up<S: ReadStore + Store + Processed>(
        &mut self,
        project: &Project,
        store: &S,
        registry: &Registry,
    ) -> Result<&P, Report> {
        let start = self.start;
        let catch_up = self.held.entry(project.id.clone()).or_insert_with(start);
        let caught = catch_up
            .step(store, registry.catalog())
            .await
            .map_err(|error| Report::of(&error))?;
        if let Some((_, error)) = caught.failures().first() {
            return Err(Report::of(error));
        }
        Ok(catch_up.projector())
    }
}

#[cfg(test)]
#[path = "lists.test.rs"]
mod tests;
