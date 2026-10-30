//! A task list on angzarr-embedded (step 3a of
//! taskloom-on-angzarr-embedded.plan.md): the projector of [`crate`]'s
//! annals list, over [`TaskAggregate`]'s streams, beside it until the
//! application moves (step 4), when it replaces it.
//!
//! RED: every body is `todo!()`. The proposed shape:
//!
//! - [`TaskList`] is a [`Projection`] of [`TaskAggregate`]: the kernel hands
//!   it each committed event live ([`ProjectionRoute`](angzarr_embedded::next::ProjectionRoute)),
//!   and [`TaskList::catch_up`] reads every task stream the store holds in
//!   one read-only unit ([`ReadStore`]: its `covers` lists them, its reads
//!   read them, all at one moment), as a process that did not run the
//!   command does. What wakes a catch-up ([`Watch`](angzarr_embedded::next::Watch)'s
//!   change token) is the application's to wire (step 4).
//! - A list holds every live task (none removed), and the tasks a prune
//!   removed whose streams are not yet purged ([`TaskList::unpurged`]). A
//!   stream purged and minted again on its freed harp is a new stream, told
//!   from the old by its first event (its cause and when it was recorded),
//!   never by its sequence: each starts at 0.
//! - **No cross-stream number, anywhere** (the owner's ruling D7). A list
//!   numbers nothing; a watcher resumes from a [`ListCursor`], a vector:
//!   for each stream, by harp id, the sequence of the last event of that
//!   stream it has seen, and which stream it was (its first event).
//!   [`TaskList::since`] answers what a watcher at a cursor is missing: each
//!   task whose stream moved past the cursor or is not the one the cursor
//!   saw, as it is now, and each task the cursor holds that has left the
//!   list. Level-based (Kubernetes rule 1): the state now, never each step
//!   between.

use angzarr_embedded::next::{Committed, KernelError, Projection, ReadStore, Registry};
use tasks::{TaskAggregate, TaskState};

/// What a watcher at a cursor is missing of one task.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// A task entered the list, or a listed task changed: its state now
    /// (boxed: a state is far larger than a removal).
    Upsert(Box<TaskState>),
    /// A task left the list.
    Remove { harp_id: String },
}

/// Where a watcher of a list is: for each task stream it has seen, by harp
/// id, the sequence of that stream's last event it has seen, and which
/// stream it was (its first event's cause and `recorded_at`, so a stream
/// purged and minted again on the harp is not taken for the one seen). A
/// vector, one entry per stream: there is no number across streams. An
/// empty cursor ([`ListCursor::default`]) has seen nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListCursor {}

impl ListCursor {
    /// The sequence of the last event of `harp_id`'s stream this cursor
    /// has seen; `None` for a stream it has not.
    pub fn sequence(&self, harp_id: &str) -> Option<u64> {
        let _ = harp_id;
        todo!("3a: the cursor's sequence for one stream")
    }

    /// Every stream the cursor has seen, by harp id, with the sequence of
    /// its last event seen, in harp id order.
    pub fn streams(&self) -> Vec<(&str, u64)> {
        todo!("3a: the cursor's streams")
    }
}

/// One list of a project's tasks: every live task (none removed). A new one
/// ([`TaskList::default`]) is empty.
#[derive(Default)]
pub struct TaskList {}

impl TaskList {
    /// The listed tasks, by harp id.
    pub fn entries(&self) -> Vec<&TaskState> {
        todo!("3a: the listed tasks")
    }

    /// The tasks a prune removed whose streams were still held when last
    /// read, by harp id: a prune left half done, for a prune to finish.
    pub fn unpurged(&self) -> Vec<&TaskState> {
        todo!("3a: the pruned tasks not yet purged")
    }

    /// Where a watcher that has seen this list as it is now is: every
    /// stream the list has projected, by harp id, with its last projected
    /// sequence and which stream it was.
    pub fn cursor(&self) -> ListCursor {
        todo!("3a: the list's cursor")
    }

    /// What a watcher at `cursor` is missing, by harp id: each listed task
    /// whose stream the cursor has not seen, or has seen less of, or saw as
    /// another stream on the same harp, as it is now ([`Change::Upsert`]);
    /// each task the cursor holds that is not listed now
    /// ([`Change::Remove`]). A stream the cursor has seen further than the
    /// list is not sent backwards. Nothing, at the list's own cursor.
    pub fn since(&self, cursor: &ListCursor) -> Vec<Change> {
        let _ = cursor;
        todo!("3a: the changes after a cursor")
    }

    /// Project every task stream `store` holds that is not yet projected,
    /// listed and read in one read-only unit, so what is projected is one
    /// moment of the store. A task whose stream the store no longer holds
    /// (pruned whole since the last catch-up) leaves the list, and its
    /// stream is forgotten, so a task minted on its harp later is seen from
    /// its start.
    pub async fn catch_up<S: ReadStore>(
        &mut self,
        store: &S,
        registry: &Registry,
    ) -> Result<(), Box<KernelError>> {
        let _ = (store, registry);
        todo!("3a: catch up from the store")
    }
}

impl Projection<TaskAggregate> for TaskList {
    fn project(&mut self, committed: &Committed<'_, TaskAggregate>) {
        let _ = committed;
        todo!("3a: project one committed event")
    }
}

#[cfg(test)]
#[path = "embedded.test.rs"]
mod tests;
