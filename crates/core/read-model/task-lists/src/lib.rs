//! Task lists: read models a projector folds from a project's task streams.
//! A list is purely derived, so it is no aggregate: it projects every task
//! stream in a project's store (the store is the collection, so a list needs
//! no membership metadata on the streams), holding every live task.
//!
//! - [`TaskList`] is a [`Projection`] of [`TaskAggregate`]: the kernel hands
//!   it each committed event live ([`ProjectionRoute`](angzarr_embedded::next::ProjectionRoute)),
//!   and [`TaskList::catch_up`] reads every task stream the store holds in
//!   one read-only unit ([`ReadStore`]: its `covers` lists them, its reads
//!   read them, all at one moment), as a process that did not run the
//!   command does.
//! - A list holds every live task (none removed), and the tasks a prune
//!   removed whose streams are not yet purged ([`TaskList::unpurged`]). A
//!   stream purged and minted again on its freed harp is a new stream, told
//!   from the old by its first event (its cause and when it was recorded),
//!   never by its sequence: each starts at 0.
//! - A list keeps no cursor: the application reads one and drops it (a
//!   listing's, a prune's), and a watch asks the store's change token,
//!   never a list (the owner's ruling on D7, 2026-10-30). A list kept
//!   across catch-ups still projects each stream by its own sequence:
//!   nothing numbers across streams.

pub mod schema;

use std::collections::{BTreeMap, BTreeSet};

use angzarr_embedded::next::v1::{Cover, Uuid};
use angzarr_embedded::next::{
    ids, projection, Committed, KernelError, Payload, Projection, ReadStore, ReadUnit, Registry,
    StoreError, Stream, StreamRef, Timestamp,
};
use tasks::{RemovalReason, TaskAggregate, TaskEvent, TaskState, TASK_DOMAIN};

/// One list of a project's tasks: every live task (none removed).
#[derive(Default)]
pub struct TaskList {
    /// The listed tasks, by harp id.
    entries: BTreeMap<String, TaskState>,
    /// How far each task stream (by harp id) is projected, and which
    /// stream it was.
    projected: BTreeMap<String, Seen>,
    /// The tasks whose stream's last projected event is a prune's removal,
    /// by harp id: removed, and not yet seen purged.
    unpurged: BTreeMap<String, TaskState>,
}

impl TaskList {
    /// An empty list.
    pub fn new() -> TaskList {
        TaskList::default()
    }

    /// The listed tasks, by harp id.
    pub fn entries(&self) -> Vec<&TaskState> {
        self.entries.values().collect()
    }

    /// The tasks a prune removed whose streams were still held when last
    /// read, by harp id: a prune left half done, its purge failed after its
    /// removal landed, for a prune to finish. A purge records no event, so
    /// a list kept across one still names the task until it next catches
    /// up and finds the stream gone: whoever purges checks the stream
    /// first.
    pub fn unpurged(&self) -> Vec<&TaskState> {
        self.unpurged.values().collect()
    }

    /// Project every task stream `store` holds that is not yet projected,
    /// listed and read in one read-only unit, so what is projected is one
    /// moment of the store: a write or a purge landing meanwhile, through
    /// any handle, is not seen. A task whose stream the store no longer
    /// holds (pruned whole since the last catch-up) leaves the list, its
    /// stream is forgotten (so a task minted on its harp later is seen from
    /// its start).
    pub async fn catch_up<S: ReadStore>(
        &mut self,
        store: &S,
        registry: &Registry,
    ) -> Result<(), Box<KernelError>> {
        let mut reads = store.begin_read().await.map_err(store_error)?;
        let covers = reads.covers(TASK_DOMAIN).await.map_err(store_error)?;
        let covers: Vec<Cover> = covers
            .into_iter()
            .filter(|cover| cover.edition.is_none())
            .collect();
        let held: BTreeSet<Vec<u8>> = covers.iter().map(root).collect();
        for cover in covers {
            let stream = StreamRef::try_from(&cover).map_err(store_error)?;
            let slice = reads.read(stream, 0, None).await.map_err(store_error)?;
            let stream = Stream::<TaskAggregate>::replay(cover, &slice.pages, registry)?;
            stream.dispatch(0, registry, &mut [&mut projection(self)])?;
        }
        self.forget_gone(&held);
        Ok(())
    }

    /// Forget every task whose stream the store no longer holds (`held`
    /// are the streams it does, by root): a prune that landed whole since
    /// this list last read. Each such task leaves the list.
    fn forget_gone(&mut self, held: &BTreeSet<Vec<u8>>) {
        let gone: Vec<String> = self
            .projected
            .keys()
            .filter(|harp| !held.contains(&root_of(harp)))
            .cloned()
            .collect();
        for harp in &gone {
            self.projected.remove(harp);
            self.entries.remove(harp);
            self.unpurged.remove(harp);
        }
    }
}

impl Projection<TaskAggregate> for TaskList {
    fn project(&mut self, committed: &Committed<'_, TaskAggregate>) {
        // Lists show the main timeline; a what-if edition is not the project.
        if committed.cover().edition.is_some() {
            return;
        }
        let task = committed.state.state();
        let harp = task.harp_id.clone();
        let sequence = committed.sequence();
        // A pruned stream is purged, and a task minted on the freed harp
        // starts it again from sequence 0, perhaps all between two
        // catch-ups: a first event not the one seen first is a new stream,
        // projected from its start (its first state replaces the old
        // task's), not skipped as already seen.
        let first = (sequence == 0).then(|| Fingerprint::of(committed));
        if let Some(seen) = self.projected.get(&harp) {
            let restarted = first.is_some() && first != seen.first;
            if !restarted && sequence <= seen.last {
                return;
            }
        }
        let first = first.or_else(|| self.projected.get(&harp)?.first.clone());
        self.projected.insert(
            harp.clone(),
            Seen {
                first,
                last: sequence,
            },
        );
        if is_pruned(committed) {
            self.unpurged.insert(harp.clone(), task.clone());
        } else {
            self.unpurged.remove(&harp);
        }
        if task.removed {
            self.entries.remove(&harp);
        } else {
            self.entries.insert(harp, task.clone());
        }
    }
}

/// How far one task stream is projected.
struct Seen {
    /// Its first event's fingerprint; none when the list began projecting
    /// it past its start (live, never caught up), so its first event, if
    /// it comes, is projected again.
    first: Option<Fingerprint>,
    /// The last sequence projected.
    last: u64,
}

/// What tells one stream on a harp from another: its first event's cause
/// (the command that minted the task) and when it was recorded. The
/// sequence cannot tell them apart (each starts at 0), nor the payload (a
/// task minted again may be the same).
#[derive(Clone, PartialEq, Debug)]
struct Fingerprint {
    cause: Uuid,
    recorded_at: Timestamp,
}

impl Fingerprint {
    fn of(committed: &Committed<'_, TaskAggregate>) -> Fingerprint {
        let envelope = committed.envelope();
        Fingerprint {
            cause: envelope.command_id().clone(),
            recorded_at: envelope.recorded_at(),
        }
    }
}

/// Whether `committed` is a prune's removal: the last its stream holds
/// before it is purged, read as the tasks domain's event.
fn is_pruned(committed: &Committed<'_, TaskAggregate>) -> bool {
    let any = committed.event().to_any();
    matches!(
        TaskEvent::from_any(&any),
        Ok(TaskEvent::TaskRemoved(removed)) if removed.reason == RemovalReason::Pruned
    )
}

/// A task stream's root, as the list compares streams by.
fn root(cover: &Cover) -> Vec<u8> {
    cover
        .root
        .as_ref()
        .map(|r| r.value.to_vec())
        .unwrap_or_default()
}

/// The root of the stream of the task `harp_id` names.
fn root_of(harp_id: &str) -> Vec<u8> {
    ids::compute_root(TASK_DOMAIN, harp_id)
        .map(|r| r.value.to_vec())
        .unwrap_or_default()
}

fn store_error(error: StoreError) -> Box<KernelError> {
    Box::new(KernelError::Store(error))
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
