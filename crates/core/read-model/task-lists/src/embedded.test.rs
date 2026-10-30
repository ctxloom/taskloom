//! RED, step 3a of taskloom-on-angzarr-embedded.plan.md: the task list on
//! angzarr-embedded. Each command runs through angzarr-embedded's kernel
//! over angzarr-ports' in-memory store with the list attached as a
//! projection, so the list sees each committed event with the task just
//! after it; a fresh list catches up from the store, as every read and
//! watch of another process does.
//!
//! `lib.test.rs`'s tests, ported: the per-stream ones (a redelivered event
//! changes nothing; a removal by name keeps its stream's place) and the
//! fingerprint ones (a stream pruned, purged and minted again is a new
//! stream, though it restarts at 0). Where they counted the list's own
//! sequence they now ask a [`ListCursor`] what is new: under the owner's
//! ruling D7 a list numbers nothing, and a watcher resumes from a vector,
//! each stream's sequence by harp id. Not ported: the two `since` tests of
//! a list's own sequence (a cursor past it, and the changes after one),
//! which the cursor's tests replace, and the sync catch-up's twins: the
//! ports are async only, so there is one catch-up.

use std::future::Future;
use std::marker::PhantomData;

use angzarr_embedded::next::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::next::v1::{command_page, Command, CommandBook, CommandPage, Cover, Uuid};
use angzarr_embedded::next::{
    ids, Any, EventStore, Kernel, MemoryDeadLetters, OnRetryable, ProjectionRoute, Purge,
    ReadStore, ReadUnit, Registry, Sampled, Slice, StoreError, StreamRef, Timestamp,
};
use annals::next::Payload;
use tasks::{AddTask, RemovalReason, RemoveTask, SetStatus, TaskAggregate, TaskCommand};

use super::{Change, ListCursor, TaskList};

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
        ready(Purge::purge(
            &self.store,
            StreamRef::try_from(&cover).unwrap(),
        ))
        .unwrap()
    }

    /// Catch `list` up from the store.
    fn catch_up(&self, list: &mut TaskList) {
        ready(list.catch_up(&self.store, &self.registry)).unwrap();
    }

    /// A list built afresh from the store: another process's.
    fn rebuilt(&self) -> TaskList {
        let mut list = every_task();
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
    TaskList::default()
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

/// Each change as (harp, text, status), a removal as (harp, "removed", "").
fn said(changes: &[Change]) -> Vec<(&str, &str, &str)> {
    changes
        .iter()
        .map(|change| match change {
            Change::Upsert(t) => (t.harp_id.as_str(), t.text.as_str(), t.status.as_str()),
            Change::Remove { harp_id } => (harp_id.as_str(), "removed", ""),
        })
        .collect()
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

// ------------------------------------------------------------ the cursor (D7)

#[test]
fn test_a_cursor_holds_each_streams_last_sequence_by_harp_and_nothing_across_them() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, status("In Progress", ""));
    project.run(&mut list, FIRST, status("Done", ""));
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    let cursor = list.cursor();
    assert_eq!(
        cursor.streams(),
        [(SECOND, 0), (FIRST, 2)],
        "one entry per stream, its own sequence: no number of the list's"
    );
    assert_eq!(cursor.sequence(FIRST), Some(2));
    assert_eq!(cursor.sequence("tpyo"), None, "a stream it never saw");
}

#[test]
fn test_a_stream_removed_by_name_keeps_its_place_in_the_cursor() {
    // Its stream is held, so a watcher has seen it, though it is not listed.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, removed());
    assert_eq!(list.cursor().streams(), [(FIRST, 1)]);
}

#[test]
fn test_an_empty_cursor_resumes_to_the_whole_list() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    assert_eq!(
        said(&list.since(&ListCursor::default())),
        [(SECOND, "Second", "To Do"), (FIRST, "First", "To Do")],
        "every listed task, by harp id"
    );
}

#[test]
fn test_the_lists_own_cursor_is_missing_nothing() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, removed());
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    assert_eq!(list.since(&list.cursor()), []);
}

#[test]
fn test_resuming_from_a_cursor_replays_only_the_streams_that_moved() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    let cursor = list.cursor();
    project.run(&mut list, FIRST, status("In Progress", ""));
    project.run(&mut list, FIRST, status("Done", ""));
    assert_eq!(
        said(&list.since(&cursor)),
        [(FIRST, "First", "Done")],
        "the moved stream once, as it is now; the still one not at all"
    );
}

#[test]
fn test_resuming_from_a_cursor_shows_a_stream_it_has_not_seen() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    let cursor = list.cursor();
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    assert_eq!(said(&list.since(&cursor)), [(SECOND, "Second", "To Do")]);
}

#[test]
fn test_a_task_removed_since_the_cursor_is_a_removal() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    let cursor = list.cursor();
    project.run(&mut list, FIRST, removed());
    assert_eq!(said(&list.since(&cursor)), [(FIRST, "removed", "")]);
}

#[test]
fn test_a_cursor_that_never_saw_a_removed_task_is_told_nothing_of_it() {
    let mut project = Project::new();
    let mut list = every_task();
    let cursor = list.cursor();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, removed());
    assert_eq!(list.since(&cursor), [], "nothing it holds left");
}

#[test]
fn test_a_cursor_resumes_against_a_list_another_process_rebuilt_from_the_store() {
    // The cursor is the store's streams' own sequences, not a number one
    // list made up: a list rebuilt elsewhere resumes it to what is new.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    let cursor = list.cursor();
    project.run(&mut list, SECOND, status("Done", ""));

    let elsewhere = project.rebuilt();
    assert_eq!(
        elsewhere.cursor(),
        list.cursor(),
        "the same streams, seen as far"
    );
    assert_eq!(
        said(&elsewhere.since(&cursor)),
        [(SECOND, "Second", "Done")]
    );
}

#[test]
fn test_a_stream_the_cursor_has_seen_further_than_the_list_is_not_sent_backwards() {
    // A watcher's cursor from a list ahead of this one (live, while this
    // one last caught up earlier): this list's older task is not sent.
    let mut project = Project::new();
    let mut live = every_task();
    project.run(&mut live, FIRST, add(FIRST, "First"));
    let behind = project.rebuilt();
    project.run(&mut live, FIRST, status("Done", ""));
    assert_eq!(behind.since(&live.cursor()), []);
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
    assert_eq!(rebuilt.cursor(), live.cursor());
}

#[test]
fn test_a_redelivered_event_changes_nothing() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    let cursor = list.cursor();
    project.catch_up(&mut list);
    assert_eq!(
        list.since(&cursor),
        [],
        "the store's events were already projected"
    );
    assert_eq!(list.cursor(), cursor);
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
    assert_eq!(list.cursor().sequence(FIRST), Some(2));
}

#[test]
fn test_an_append_seen_at_a_catch_up_is_no_restart() {
    // The stream read back from its start: its first event is the one
    // already seen, so only the new event is projected.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    let cursor = list.cursor();
    let mut elsewhere = every_task();
    project.run(&mut elsewhere, FIRST, status("In Progress", ""));
    project.catch_up(&mut list);
    assert_eq!(
        said(&list.since(&cursor)),
        [(FIRST, "First", "In Progress")]
    );
    let caught_up = list.cursor();
    project.catch_up(&mut list);
    assert_eq!(list.since(&caught_up), [], "nothing new");
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
    assert_eq!(list.cursor().sequence(FIRST), Some(0), "the new stream's");
}

#[test]
fn test_a_task_removed_by_name_keeps_its_streams_place() {
    // Only a prune frees a stream; a removal by name keeps it, so its
    // events redelivered are still seen as projected.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, removed());
    let cursor = list.cursor();
    project.catch_up(&mut list);
    assert!(list.entries().is_empty());
    assert_eq!(
        list.cursor(),
        cursor,
        "nothing redelivered was projected again"
    );
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
    let cursor = list.cursor();
    let mut elsewhere = every_task();
    project.run(&mut elsewhere, FIRST, pruned());
    project.purge(FIRST);

    project.catch_up(&mut list);
    assert_eq!(harps(&list), [SECOND]);
    assert_eq!(said(&list.since(&cursor)), [(FIRST, "removed", "")]);
    assert!(unpurged(&list).is_empty(), "its stream is gone");
    assert_eq!(
        list.cursor().streams(),
        [(SECOND, 0)],
        "its stream forgotten"
    );
    let gone = list.cursor();
    project.catch_up(&mut list);
    assert_eq!(list.since(&gone), [], "gone once");

    // The freed harp minted again: its stream starts over, and is seen.
    project.run(&mut elsewhere, FIRST, add(FIRST, "Again"));
    project.catch_up(&mut list);
    assert_eq!(texts(&list), ["Second", "Again"], "by harp");
}

#[test]
fn test_a_task_the_list_does_not_hold_leaves_no_change_when_its_stream_goes() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, FIRST, pruned());
    let cursor = list.cursor();
    project.purge(FIRST);
    project.catch_up(&mut list);
    assert_eq!(list.since(&cursor), [], "nothing listed left");
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
    let cursor = list.cursor();
    project.catch_up(&mut list);
    assert_eq!(list.since(&cursor), [], "nothing projected again");
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
    assert_eq!(list.cursor(), ListCursor::default(), "no stream seen");
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
    let cursor = list.cursor();
    assert_eq!(cursor.sequence(FIRST), Some(1), "the add and its status");

    // The new stream is shorter than the old: its first event's sequence
    // is one the list has already passed.
    prune_purge_and_mint_again(&mut project, FIRST, "Again");
    project.catch_up(&mut list);
    assert_eq!(texts(&list), ["Again"], "the new task, not the pruned one");
    assert_eq!(
        said(&list.since(&cursor)),
        [(FIRST, "Again", "To Do")],
        "not the old Done"
    );
    let restarted = list.cursor();
    assert_eq!(restarted.sequence(FIRST), Some(0), "the new stream's own");
    project.catch_up(&mut list);
    assert_eq!(
        list.since(&restarted),
        [],
        "the new stream is now the one seen"
    );
}

#[test]
fn test_a_cursor_from_before_a_restart_resumes_to_the_new_task() {
    // The old stream and the new are each seen at sequence 0: only which
    // stream it was tells the watcher's from the store's.
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    project.run(&mut list, SECOND, add(SECOND, "Second"));
    let cursor = list.cursor();
    prune_purge_and_mint_again(&mut project, FIRST, "Again");
    project.run(&mut list, SECOND, status("Done", ""));
    project.catch_up(&mut list);

    assert_eq!(list.cursor().sequence(FIRST), cursor.sequence(FIRST));
    assert_eq!(
        said(&list.since(&cursor)),
        [(SECOND, "Second", "Done"), (FIRST, "Again", "To Do")],
        "by harp id"
    );
}

#[test]
fn test_a_cursor_from_before_a_restart_resumes_to_the_new_task_in_another_process() {
    let mut project = Project::new();
    let mut list = every_task();
    project.run(&mut list, FIRST, add(FIRST, "First"));
    let cursor = list.cursor();
    prune_purge_and_mint_again(&mut project, FIRST, "Again");
    assert_eq!(
        said(&project.rebuilt().since(&cursor)),
        [(FIRST, "Again", "To Do")]
    );
}
