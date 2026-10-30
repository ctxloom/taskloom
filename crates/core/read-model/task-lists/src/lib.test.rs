//! The task list, projected live and caught up: each command runs through
//! angzarr-embedded's kernel over angzarr-ports' in-memory store with the
//! list attached as a projection, so the list sees each committed event with
//! the task just after it; a fresh list catches up from the store, as every
//! read of another process does.

use std::future::Future;
use std::marker::PhantomData;

use angzarr_embedded::next::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::next::v1::{command_page, Command, CommandBook, CommandPage, Cover, Uuid};
use angzarr_embedded::next::{
    ids, Any, EventStore, Kernel, MemoryDeadLetters, OnRetryable, Payload, ProjectionRoute, Purge,
    ReadStore, ReadUnit, Registry, Sampled, Slice, StoreError, StreamRef, Timestamp,
};
use tasks::{AddTask, RemovalReason, RemoveTask, SetStatus, TaskAggregate, TaskCommand};

use super::TaskList;

const FIRST: &str = "swift-amber";
const SECOND: &str = "calm-otter";

/// Poll `future` once: the in-memory store never waits.
fn ready<F: Future>(future: F) -> F::Output {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match std::pin::pin!(future).poll(&mut cx) {
        std::task::Poll::Ready(out) => out,
        std::task::Poll::Pending => panic!("the in-memory store never waits"),
    }
}

/// One project's store, and the clock its commands are sampled at.
struct Project {
    store: MemoryStore,
    registry: Registry,
    seconds: i64,
}

impl Project {
    fn new() -> Project {
        let mut registry = Registry::new();
        registry.aggregate::<TaskAggregate>(TaskAggregate::default);
        Project {
            store: MemoryStore::new(),
            registry,
            seconds: 100,
        }
    }

    /// Run `command` on task `harp`'s stream, projecting into `list`.
    fn run(&mut self, list: &mut TaskList, harp: &str, command: impl Into<TaskCommand>) {
        self.seconds += 1;
        let sampled = Sampled {
            now: Timestamp {
                seconds: self.seconds,
                nanos: 0,
            },
            command_id: Uuid::from_bytes([self.seconds as u8; 16]),
        };
        let dead_letters = MemoryDeadLetters::new();
        let mut route = ProjectionRoute {
            name: "list",
            projection: list,
            domain: PhantomData::<fn() -> TaskAggregate>,
        };
        ready(
            Kernel::new(&self.registry, &dead_letters)
                .on_retryable(OnRetryable {
                    max_retries: 0,
                    ..OnRetryable::default()
                })
                .handle::<TaskAggregate, _, _>(
                    &self.store,
                    book(harp, command.into().to_any()),
                    &sampled,
                    &mut route,
                ),
        )
        .unwrap_or_else(|failure| panic!("{failure:?}"));
    }

    /// Purge task `harp`'s stream: what a prune does after its removal.
    fn purge(&self, harp: &str) -> u64 {
        let cover = cover(harp);
        let stream = StreamRef::try_from(&cover).unwrap();
        let head = ready(EventStore::head(&self.store, stream)).unwrap();
        ready(Purge::purge(&self.store, stream, head.next_sequence)).unwrap()
    }

    /// Catch `list` up from the store.
    fn catch_up(&self, list: &mut TaskList) {
        ready(list.catch_up(&self.store, &self.registry)).unwrap();
    }

    /// A list built afresh from the store: another process's.
    fn rebuilt(&self) -> TaskList {
        let mut list = TaskList::new();
        self.catch_up(&mut list);
        list
    }
}

fn cover(harp: &str) -> Cover {
    Cover {
        domain: tasks::TASK_DOMAIN.to_string(),
        root: Some(ids::compute_root(tasks::TASK_DOMAIN, harp).unwrap()),
        edition: None,
    }
}

fn book(harp: &str, payload: Any) -> CommandBook {
    CommandBook {
        cover: Some(cover(harp)),
        page: Some(CommandPage {
            body: Some(command_page::Body::Command(Command {
                payload: Some(payload),
            })),
            ..CommandPage::default()
        }),
        handling: None,
    }
}

fn add(harp: &str, text: &str) -> AddTask {
    AddTask {
        harp_id: harp.to_string(),
        text: text.to_string(),
        status: None,
        trigger: None,
        tags: Vec::new(),
        origin_session: None,
        defers: tasks::DEFERRED.to_string(),
        initial: tasks::TO_DO.to_string(),
    }
}

fn status(status: &str, trigger: &str) -> SetStatus {
    let given = |s: &str| (!s.is_empty()).then(|| s.to_string());
    SetStatus {
        status: given(status),
        trigger: given(trigger),
        defers: tasks::DEFERRED.to_string(),
        initial: tasks::TO_DO.to_string(),
    }
}

fn pruned() -> RemoveTask {
    RemoveTask {
        reason: RemovalReason::Pruned,
    }
}

fn removed() -> RemoveTask {
    RemoveTask {
        reason: RemovalReason::Removed,
    }
}

fn every_task() -> TaskList {
    TaskList::new()
}

fn harps(list: &TaskList) -> Vec<&str> {
    list.entries().iter().map(|t| t.harp_id.as_str()).collect()
}

fn texts(list: &TaskList) -> Vec<&str> {
    list.entries().iter().map(|t| t.text.as_str()).collect()
}

fn unpurged(list: &TaskList) -> Vec<&str> {
    list.unpurged().iter().map(|t| t.harp_id.as_str()).collect()
}

// ------------------------------------------------------------ the list

#[test]
fn test_a_list_holds_each_live_task() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    assert_eq!(harps(&list), [SECOND, FIRST], "by harp id");
}

#[test]
fn test_a_listed_task_is_held_as_it_is_after_its_last_event() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, status("In Progress", ""));
    assert_eq!(list.entries()[0].status, "In Progress");
}

#[test]
fn test_a_removed_task_leaves_the_list() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, removed());
    assert!(list.entries().is_empty());
}

// ------------------------------------------------------------ catch-up

#[test]
fn test_a_list_caught_up_from_the_store_holds_what_the_live_list_holds() {
    let mut project = Project::new();
    let mut live = every_task();
    project.run(&mut live, FIRST, add(FIRST, "First"));
    project.run(&mut live, SECOND, add(SECOND, "Second"));
    project.run(&mut live, FIRST, removed());

    let rebuilt = project.rebuilt();
    assert_eq!(rebuilt.entries(), live.entries());
    assert_eq!(harps(&rebuilt), [SECOND]);
}

#[test]
fn test_a_redelivered_event_changes_nothing() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    project.catch_up(&mut list);
    assert_eq!(
        texts(&list),
        ["Second", "First"],
        "the store's events were already projected"
    );
}

#[test]
fn test_a_redelivered_event_past_the_first_changes_nothing() {
    // Read back, the stream's middle event's state is not the task's now:
    // projected again, it would list the task as it was.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, status("In Progress", ""));
    project.run(&mut list, FIRST, status("Done", ""));
    project.catch_up(&mut list);
    assert_eq!(list.entries()[0].status, "Done");
}

#[test]
fn test_an_append_seen_at_a_catch_up_is_no_restart() {
    // The stream read back from its start: its first event is the one
    // already seen, so only the new event is projected.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    let mut elsewhere = every_task();
    project.run(&mut elsewhere, FIRST, status("In Progress", ""));
    project.catch_up(&mut list);
    assert_eq!(texts(&list), ["First"]);
    assert_eq!(list.entries()[0].status, "In Progress");
}

// ------------------------------------------------------------ pruning

#[test]
fn test_a_task_minted_on_a_pruned_harp_is_listed_though_its_stream_restarted() {
    // Prune records the removal, then purges the stream; a new task minted
    // on the freed harp starts the stream again from its first sequence,
    // which the list must not skip as already projected.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, status("Done", ""));
    project.run(&mut list, FIRST, pruned());
    assert!(list.entries().is_empty(), "the removal drops it at once");
    assert_eq!(project.purge(FIRST), 3);

    project.run(&mut list, FIRST, add(FIRST, "Again"));
    assert_eq!(texts(&list), ["Again"], "the new task, not the pruned one");
}

#[test]
fn test_a_list_caught_up_after_a_prune_holds_the_task_minted_since() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, pruned());
    project.purge(FIRST);
    let mut other = every_task();
    project.run(&mut other, FIRST, add(FIRST, "Again"));
    project.catch_up(&mut list);
    assert_eq!(texts(&list), ["Again"]);
}

#[test]
fn test_a_task_pruned_and_purged_unseen_leaves_the_list_at_the_next_catch_up() {
    // A long-lived list (a watch's) catching up after a prune landed whole
    // between two of its reads: it never saw the removal, and the stream
    // is gone, so only the stream's absence says the task went.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    let mut elsewhere = every_task();
    project.run(&mut elsewhere, FIRST, pruned());
    project.purge(FIRST);

    project.catch_up(&mut list);
    assert_eq!(harps(&list), [SECOND]);
    assert!(unpurged(&list).is_empty(), "its stream is gone");
    project.catch_up(&mut list);
    assert_eq!(harps(&list), [SECOND], "gone once");

    // The freed harp minted again: its stream starts over, and is seen.
    project.run(&mut elsewhere, FIRST, add(FIRST, "Again"));
    project.catch_up(&mut list);
    assert_eq!(texts(&list), ["Second", "Again"], "by harp");
}

#[test]
fn test_a_task_the_list_does_not_hold_leaves_no_entry_when_its_stream_goes() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, pruned());
    project.purge(FIRST);
    project.catch_up(&mut list);
    assert!(list.entries().is_empty(), "nothing listed left");
    assert!(unpurged(&list).is_empty(), "its stream forgotten");
}

#[test]
fn test_a_task_pruned_but_not_yet_purged_is_held_as_unpurged() {
    // A prune whose purge failed after its removal landed leaves the
    // stream behind, its harp taken: the list names it, for a prune to
    // finish.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, status("Done", ""));
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    assert!(unpurged(&list).is_empty(), "nothing pruned yet");
    project.run(&mut list, FIRST, pruned());
    project.run(&mut list, SECOND, removed());
    assert_eq!(unpurged(&list), [FIRST], "a removal by name is no prune's");
    assert!(list.entries().is_empty());
    assert_eq!(list.unpurged()[0].status, "Done", "as it was pruned");

    let mut fresh = project.rebuilt();
    assert_eq!(unpurged(&fresh), [FIRST], "read back from the store");
    project.catch_up(&mut fresh);
    assert_eq!(unpurged(&fresh), [FIRST], "read again, still once");
}

#[test]
fn test_a_pruned_stream_read_again_before_its_purge_changes_nothing() {
    // A long-lived list that saw the prune's removal reads the stream
    // again before the purge lands: the same stream, already projected.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, pruned());
    project.catch_up(&mut list);
    assert!(list.entries().is_empty(), "nothing projected again");
    assert_eq!(unpurged(&list), [FIRST], "still pruned, not purged");
}

#[test]
fn test_a_harp_minted_again_after_its_purge_is_no_longer_unpurged() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, pruned());
    project.purge(FIRST);
    project.run(&mut list, FIRST, add(FIRST, "Again"));
    assert!(unpurged(&list).is_empty(), "the stream holds a new task");
    assert_eq!(harps(&list), [FIRST]);
}

/// A store whose every stream reads back empty: what a stream purged
/// between a listing's `covers` and its read can look like.
struct Hollow(MemoryStore);

/// Hollow's read-only unit: the streams listed, each read empty.
struct HollowRead<'s>(&'s MemoryStore);

impl ReadStore for Hollow {
    type Reads<'s> = HollowRead<'s>;

    async fn begin_read(&self) -> Result<HollowRead<'_>, StoreError> {
        Ok(HollowRead(&self.0))
    }
}

impl ReadUnit for HollowRead<'_> {
    async fn covers(&mut self, domain: &str) -> Result<Vec<Cover>, StoreError> {
        EventStore::covers(self.0, domain).await
    }

    async fn read(
        &mut self,
        _: StreamRef<'_>,
        _: u64,
        _: Option<u64>,
    ) -> Result<Slice, StoreError> {
        Ok(Slice::default())
    }

    async fn read_snapshot(
        &mut self,
        _: StreamRef<'_>,
    ) -> Result<Option<angzarr_embedded::next::v1::Snapshot>, StoreError> {
        Ok(None)
    }
}

#[test]
fn test_a_stream_that_replays_to_no_events_is_never_a_blank_task() {
    let mut project = Project::new();
    let mut live = every_task();
    project.run(&mut live, FIRST, add(FIRST, "First"));
    let store = Hollow(std::mem::take(&mut project.store));

    let mut list = every_task();
    ready(list.catch_up(&store, &project.registry)).unwrap();
    assert!(list.entries().is_empty(), "{:?}", list.entries());
    assert!(unpurged(&list).is_empty(), "no stream seen");
}

// ------------------------------------------------------------ restarted streams

/// A prune's removal, its purge and a task minted again on the freed harp,
/// all by another hand, in the store: what a long-lived list (a watch's)
/// can miss whole between two catch-ups.
fn prune_purge_and_mint_again(project: &mut Project, harp: &str, text: &str) {
    let mut elsewhere = every_task();
    project.run(&mut elsewhere, harp, pruned());
    project.purge(harp);
    project.run(&mut elsewhere, harp, add(harp, text));
}

#[test]
fn test_a_stream_pruned_purged_and_minted_again_between_catch_ups_lists_the_new_task() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, status("Done", ""));
    project.catch_up(&mut list);

    // The new stream is shorter than the old: its first event's sequence
    // is one the list has already passed.
    prune_purge_and_mint_again(&mut project, FIRST, "Again");
    project.catch_up(&mut list);
    assert_eq!(texts(&list), ["Again"], "the new task, not the pruned one");
    assert_eq!(list.entries()[0].status, "To Do", "not the old Done");
    project.catch_up(&mut list);
    assert_eq!(
        texts(&list),
        ["Again"],
        "the new stream is now the one seen"
    );
}

#[test]
fn test_a_restart_unseen_beside_another_streams_change_lists_both_as_they_are() {
    // The old stream and the new are each seen at sequence 0, so the
    // sequence alone cannot tell them apart: the first event does.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    prune_purge_and_mint_again(&mut project, FIRST, "Again");
    project.run(&mut list, SECOND, status("Done", ""));
    project.catch_up(&mut list);

    let whole: Vec<(&str, &str, &str)> = list
        .entries()
        .iter()
        .map(|t| (t.harp_id.as_str(), t.text.as_str(), t.status.as_str()))
        .collect();
    assert_eq!(
        whole,
        [(SECOND, "Second", "Done"), (FIRST, "Again", "To Do")],
        "by harp id"
    );
}

#[test]
fn test_a_restarted_stream_read_by_another_process_is_the_new_task() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    prune_purge_and_mint_again(&mut project, FIRST, "Again");
    let elsewhere = project.rebuilt();
    assert_eq!(texts(&elsewhere), ["Again"]);
}
