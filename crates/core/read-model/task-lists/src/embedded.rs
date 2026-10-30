//! A task list on angzarr-embedded (step 3a of
//! taskloom-on-angzarr-embedded.plan.md): the projector of [`crate`]'s
//! annals list, over [`TaskAggregate`]'s streams, beside it until the
//! application moves (step 4), when it replaces it.
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
//! - **No cross-stream number, anywhere** (the owner's ruling D7). A
//!   watcher resumes from a [`ListCursor`]: the list's instance, and for
//!   each stream, by harp id, the sequence of the last event of it the
//!   watcher has seen. [`TaskList::since`] answers what a watcher at a
//!   cursor is missing: each task whose stream moved past the cursor, as it
//!   is now, and each task the cursor holds that has left the list.
//!   Level-based (Kubernetes rule 1): the state now, never each step
//!   between.
//! - **A purge bumps the list's instance** (the owner's ruling on D7): a
//!   stream gone from the store, or started again on its harp, can leave
//!   a sequence a watcher holds meaning another stream, so every cursor of
//!   an earlier instance is resynced whole ([`TaskList::since`] is `None`),
//!   never resumed. A list's instance is the caller's (the application
//!   mints one per list it builds), so a cursor from another list, another
//!   process's, is resynced too.

use std::collections::{BTreeMap, BTreeSet};

use angzarr_embedded::next::v1::{Cover, Uuid};
use angzarr_embedded::next::{
    ids, projection, Committed, KernelError, Payload as _, Projection, ReadStore, ReadUnit,
    Registry, StoreError, Stream, StreamRef, Timestamp,
};
use tasks::{RemovalReason, TaskAggregate, TaskEvent, TaskState, TASK_DOMAIN};

/// What a watcher at a cursor is missing of one task.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// A task entered the list, or a listed task changed: its state now
    /// (boxed: a state is far larger than a removal).
    Upsert(Box<TaskState>),
    /// A task left the list.
    Remove { harp_id: String },
}

/// Which list, as it stood between two purges: the instance the caller
/// gave it, and how many purges it has seen since.
#[derive(Clone, Debug, PartialEq)]
struct Instance {
    id: Uuid,
    purges: u64,
}

/// Where a watcher of a list is: the list's instance it read, and for each
/// task stream it has seen, by harp id, the sequence of that stream's last
/// event it has seen. A vector, one entry per stream: there is no number
/// across streams. An empty cursor ([`ListCursor::default`]) has seen
/// nothing, of any instance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListCursor {
    instance: Option<Instance>,
    seen: BTreeMap<String, u64>,
}

impl ListCursor {
    /// The sequence of the last event of `harp_id`'s stream this cursor
    /// has seen; `None` for a stream it has not.
    pub fn sequence(&self, harp_id: &str) -> Option<u64> {
        self.seen.get(harp_id).copied()
    }

    /// Every stream the cursor has seen, by harp id, with the sequence of
    /// its last event seen, in harp id order.
    pub fn streams(&self) -> Vec<(&str, u64)> {
        self.seen.iter().map(|(h, s)| (h.as_str(), *s)).collect()
    }
}

/// One list of a project's tasks: every live task (none removed).
pub struct TaskList {
    instance: Instance,
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
    /// An empty list, of instance `instance`: the caller's, one per list it
    /// builds, so no cursor of another list is taken for this one's.
    pub fn new(instance: Uuid) -> TaskList {
        TaskList {
            instance: Instance {
                id: instance,
                purges: 0,
            },
            entries: BTreeMap::new(),
            projected: BTreeMap::new(),
            unpurged: BTreeMap::new(),
        }
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

    /// Where a watcher that has seen this list as it is now is: its
    /// instance, and every stream it has projected, by harp id, with its
    /// last projected sequence.
    pub fn cursor(&self) -> ListCursor {
        ListCursor {
            instance: Some(self.instance.clone()),
            seen: self
                .projected
                .iter()
                .map(|(harp, seen)| (harp.clone(), seen.last))
                .collect(),
        }
    }

    /// What a watcher at `cursor` is missing, by harp id: each listed task
    /// whose stream the cursor has not seen, or has seen less of, as it is
    /// now ([`Change::Upsert`]); each task the cursor holds that is not
    /// listed now ([`Change::Remove`]). A stream the cursor has seen
    /// further than the list is not sent backwards. Nothing, at the list's
    /// own cursor. `None` for a cursor of another instance (another list's,
    /// or this one's before a purge): the watcher takes the list whole
    /// ([`TaskList::entries`]).
    pub fn since(&self, cursor: &ListCursor) -> Option<Vec<Change>> {
        if cursor
            .instance
            .as_ref()
            .is_some_and(|seen| *seen != self.instance)
        {
            return None;
        }
        let mut changes: BTreeMap<&str, Change> = BTreeMap::new();
        for (harp, seen) in &self.projected {
            let behind = cursor.seen.get(harp).is_none_or(|&had| had < seen.last);
            if !behind {
                continue;
            }
            match self.entries.get(harp) {
                Some(task) => {
                    changes.insert(harp, Change::Upsert(Box::new(task.clone())));
                }
                None if cursor.seen.contains_key(harp) => {
                    changes.insert(harp, removal(harp));
                }
                None => {}
            }
        }
        for harp in cursor.seen.keys() {
            if !self.projected.contains_key(harp) {
                changes.insert(harp, removal(harp));
            }
        }
        Some(changes.into_values().collect())
    }

    /// Project every task stream `store` holds that is not yet projected,
    /// listed and read in one read-only unit, so what is projected is one
    /// moment of the store: a write or a purge landing meanwhile, through
    /// any handle, is not seen. A task whose stream the store no longer
    /// holds (pruned whole since the last catch-up) leaves the list, its
    /// stream is forgotten (so a task minted on its harp later is seen from
    /// its start), and the purge bumps the list's instance.
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
    /// this list last read. Each such task leaves the list, and the purge
    /// bumps its instance.
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
        if !gone.is_empty() {
            self.purged();
        }
    }

    /// A purge seen: every cursor of the instance before it is resynced.
    fn purged(&mut self) {
        self.instance.purges += 1;
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
        // task's), not skipped as already seen, and the purge it shows
        // bumps the instance.
        let first = (sequence == 0).then(|| Fingerprint::of(committed));
        if let Some(seen) = self.projected.get(&harp) {
            let restarted = first.is_some() && first != seen.first;
            if !restarted && sequence <= seen.last {
                return;
            }
            if restarted {
                self.purged();
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
/// task minted again may be the same). The list's own: a cursor never
/// carries it.
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

fn removal(harp: &str) -> Change {
    Change::Remove {
        harp_id: harp.to_string(),
    }
}

/// Whether `committed` is a prune's removal: the last its stream holds
/// before it is purged. Decoded through the tasks domain's own layer,
/// which reads the same payloads.
fn is_pruned(committed: &Committed<'_, TaskAggregate>) -> bool {
    let any = committed.event().to_any();
    matches!(
        <TaskEvent as annals::next::Payload>::from_any(&any),
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
#[path = "embedded.test.rs"]
mod tests;
