//! Task lists: read models a projector folds from a project's task streams.
//! A list is purely derived, so it is no aggregate: it projects every task
//! stream in a project's store (the store is the collection, so a list needs
//! no membership metadata on the streams), holding every live task.
//!
//! - [`TaskList`] is the read model, folded one stream's book at a time
//!   ([`TaskList::project`]); [`TaskLists`] is it on angzarr-embedded's
//!   projector port, which a [`CatchUp`] catches up from the store
//!   ([`TaskList::catch_up`]): every main-timeline task stream handed what
//!   it is owed, from where the list is on it, read in one read-only unit
//!   (one moment of the store: a write or a purge landing meanwhile,
//!   through any handle, is the next step's), as a process that did not
//!   run the command does.
//! - A list holds every live task (none removed), and the tasks a prune
//!   removed whose streams are not yet purged ([`TaskList::unpurged`]). A
//!   purge keeps the stream and appends a tombstone, which the catch-up
//!   hands like any event: the list resets the stream at it, so a stream
//!   purged and minted again on its freed harp continues from there, and
//!   one purged and not minted again holds no task.
//! - A list keeps no cursor across streams: the catch-up keeps where it is
//!   on each stream, by that stream's own sequence, and a watch asks the
//!   store's change token, never a list (the owner's ruling on D7,
//!   2026-10-30). Nothing numbers across streams.

pub mod schema;

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use angzarr_embedded::angzarr_ports::tombstone;
use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
use angzarr_embedded::v1::event_page::Body;
use angzarr_embedded::v1::{Cover, EventBook};
use angzarr_embedded::{CatchUp, Domain, PageContext, Payload, PayloadError, ProjectorHandler};
use tasks::{RemovalReason, TaskAggregate, TaskEvent, TaskState, TASK_DOMAIN};

/// One list of a project's tasks: every live task (none removed).
#[derive(Default)]
pub struct TaskList {
    /// Every task stream projected, by root: the task as its events leave
    /// it.
    streams: BTreeMap<Vec<u8>, TaskAggregate>,
    /// The listed tasks, by harp id.
    entries: BTreeMap<String, TaskState>,
    /// The tasks whose stream's last projected event is a prune's removal,
    /// by harp id: removed, and not yet seen purged.
    unpurged: BTreeMap<String, TaskState>,
}

impl TaskList {
    /// An empty list.
    pub fn new() -> TaskList {
        TaskList::default()
    }

    /// An empty list on the projector port, to catch up from a store: its
    /// positions held in memory, so its first step folds every task stream
    /// from its start, and each later one only what was written since.
    pub fn catch_up() -> CatchUp<TaskLists> {
        CatchUp::new(&[TASK_DOMAIN], TaskLists::default())
            .expect("a task list reads the task domain")
    }

    /// The listed tasks, by harp id.
    pub fn entries(&self) -> Vec<&TaskState> {
        self.entries.values().collect()
    }

    /// The tasks a prune removed whose streams were still held when last
    /// read, by harp id: a prune left half done, its purge failed after its
    /// removal landed, for a prune to finish. A list kept across a purge
    /// still names the task until it next catches up and reads the
    /// tombstone: whoever purges checks the stream first.
    pub fn unpurged(&self) -> Vec<&TaskState> {
        self.unpurged.values().collect()
    }

    /// Fold `book`, one task stream's events after the last projected of
    /// it, as the projector port hands it. A tombstone (a purge's
    /// `StreamClosed`) resets the stream: its task, whatever it was,
    /// leaves the list and the unpurged, and the stream is the default
    /// again for what follows (a task minted on the freed harp). An event
    /// the tasks domain cannot read folds nothing and is the fold's error.
    pub fn project(&mut self, book: &EventBook) -> Result<(), PayloadError> {
        let root = root(book.cover.as_ref());
        if book.pages.is_empty() {
            return Ok(());
        }
        let mut task = self.streams.remove(&root).unwrap_or_default();
        let mut pruned = false;
        let mut closed = false;
        for page in &book.pages {
            if tombstone::is(page) {
                self.unlist(&task);
                task = TaskAggregate::default();
                (pruned, closed) = (false, true);
                continue;
            }
            let Some(Body::Event(event)) = page.body.clone() else {
                continue;
            };
            let at = PageContext {
                cover: book.cover.as_ref(),
                sequence: page.sequence,
                meta: page.meta.as_ref(),
            };
            TaskAggregate::apply(&mut task, &TaskEvent::from_any(&event)?, &at);
            (pruned, closed) = (is_pruned(&event), false);
        }
        if !closed {
            self.list(&task, pruned);
        }
        self.streams.insert(root, task);
        Ok(())
    }

    /// List `task` as its stream's last event leaves it: removed, it
    /// leaves the list, and is unpurged if that event is a prune's.
    fn list(&mut self, task: &TaskAggregate, pruned: bool) {
        let task = task.state();
        let harp = task.harp_id.clone();
        match pruned {
            true => self.unpurged.insert(harp.clone(), task.clone()),
            false => self.unpurged.remove(&harp),
        };
        match task.removed {
            true => self.entries.remove(&harp),
            false => self.entries.insert(harp, task.clone()),
        };
    }

    /// Take `task` out of the list: a purge closed its stream.
    fn unlist(&mut self, task: &TaskAggregate) {
        let harp = &task.state().harp_id;
        self.entries.remove(harp);
        self.unpurged.remove(harp);
    }
}

/// A [`TaskList`] on angzarr-embedded's projector port: what a
/// [`CatchUp`] hands each task stream's book.
#[derive(Default)]
pub struct TaskLists(Mutex<TaskList>);

impl TaskLists {
    /// The list as projected so far.
    pub fn list(&self) -> MutexGuard<'_, TaskList> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ProjectorHandler for TaskLists {
    fn name(&self) -> &str {
        "task-list"
    }

    async fn dispatch(&self, request: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        if let Some(events) = &request.events {
            self.list().project(events).map_err(|error| bug(&error))?;
        }
        Ok(ProjectResponse { projection: None })
    }
}

/// `error`, an event the list cannot read, as the projector port's bug:
/// its code, and its text args (the type URL it names).
fn bug(error: &PayloadError) -> CodedError {
    use angzarr_embedded::coded_diagnostic::{Arg, Diagnostic as _};
    let args = error
        .args()
        .into_iter()
        .filter_map(|(name, arg)| match arg {
            Arg::Str(value) => Some((name.to_string(), value)),
            _ => None,
        });
    CodedError::bug(error.code(), args)
}

/// Whether `event` is a prune's removal: the last its stream holds before
/// it is purged, read as the tasks domain's event.
fn is_pruned(event: &angzarr_embedded::Any) -> bool {
    matches!(
        TaskEvent::from_any(event),
        Ok(TaskEvent::TaskRemoved(removed)) if removed.reason == RemovalReason::Pruned
    )
}

/// A task stream's root, as the list tells streams apart by.
fn root(cover: Option<&Cover>) -> Vec<u8> {
    cover
        .and_then(|cover| cover.root.as_ref())
        .map(|r| r.value.to_vec())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
