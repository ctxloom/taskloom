//! Each project's list, caught up from the project's store whenever it is
//! read, as every taskloom context reads one (the tasks' task list, the
//! plans' plan list): the shared half of a read, which no context owns.
//!
//! A list is a projector on angzarr-embedded's port, run as a [`CatchUp`]
//! whose positions are held in memory, with the list: the first read of a
//! project in a process folds every stream of its store from its start
//! (the lists are not persisted, so there is nothing to resume from), and
//! every later read in the same process only what was written since (a
//! long-lived service's: a watch's, or ctxloom's). A catch-up reads one
//! moment of the store, per stream only; a stream purged, or purged and
//! written again, is told by the catch-up.

use std::collections::BTreeMap;

use angzarr_diagnostic::Report;
use angzarr_embedded::{CatchUp, ListPositions, ProjectorHandler, ReadStore, Registry, Store};
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
    pub async fn caught_up<S: ReadStore + Store + ListPositions>(
        &mut self,
        project: &Project,
        store: &S,
        registry: &Registry,
    ) -> Result<&P, Report> {
        let _ = (self.start, &self.held, project, store, registry);
        todo!("item 4: the shared catch-up")
    }
}

#[cfg(test)]
#[path = "lists.test.rs"]
mod tests;
