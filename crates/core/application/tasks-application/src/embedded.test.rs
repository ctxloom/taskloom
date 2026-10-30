//! RED, step 4 of taskloom-on-angzarr-embedded.plan.md: the use cases on
//! angzarr-embedded, over angzarr-ports' in-memory store behind a store
//! that fails on cue and counts what it is asked.
//!
//! These are the step's own: commands through the kernel, one retry loop,
//! a prune through `Purge`, a watch through `Watch` and the per-stream
//! cursor. At green the use cases' existing suites (`lib.test.rs`,
//! `read.test.rs`, `prune.test.rs`, `watch.test.rs`, ...) move onto these
//! fakes as the use cases move, and keep every check they make.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use angzarr_embedded::next::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::next::v1::{
    command_page, Command, CommandBook, CommandPage, Cover, EventPage, Snapshot, Uuid,
};
use angzarr_embedded::next::{
    ids, ChangeToken, EventStore, Head, Kernel, MemoryDeadLetters, OnRetryable, PositionKey,
    PositionStore, Purge, ReadStore, Registry, Sampled, Slice, SnapshotStore, StoreError,
    StreamRef, Watch, Write, Written as Landed,
};
use angzarr_ports::Cause;
use annals::next::Payload;
use annals_diagnostic::Diagnostic;
use ports::embedded::{CommandIds, HeldToken, TaskStores, WatchedStores};
use ports::Project;
use tasks::{AddTask, TaskAggregate, TaskCommand, TaskEvent, TASK_DOMAIN};

use super::{
    Changed, Polled, Service, Watch as Watching, WatchCursor, WatchRequest, WatchService,
    KERNEL_RETRY,
};
use crate::fakes::{project, ready, FakeClock, FakeDelay, FakeIdentity, FakeNames};
use crate::{AppError, NewTask, PruneRequest, PruneService, StatusChange, TaskService};

const HARP: &str = "swift-amber";
const OTHER: &str = "calm-otter";

// ------------------------------------------------------------ the fakes

/// A failure the store answers a write with, in its turn.
#[derive(Clone)]
enum Fail {
    /// The store could not say (UNAVAILABLE).
    Unavailable,
    /// Someone else appended first (ABORTED).
    Conflict,
}

/// What runs just before the store purges: another hand's writes.
type Meanwhile = Box<dyn FnOnce(&MemoryStore) + Send>;

/// angzarr-ports' in-memory store, failing writes on cue, running another
/// hand's writes before a purge, and counting writes and read units.
#[derive(Default)]
struct Flaky {
    inner: MemoryStore,
    fails: Mutex<VecDeque<Fail>>,
    /// Fail every write, after `fails` runs out.
    always: Option<Fail>,
    writes: AtomicU32,
    reads: AtomicU32,
    before_purge: Mutex<Option<Meanwhile>>,
}

impl Flaky {
    fn failing(fails: &[Fail]) -> Flaky {
        Flaky {
            fails: Mutex::new(fails.iter().cloned().collect()),
            ..Flaky::default()
        }
    }

    fn failure(fail: Fail, stream: StreamRef<'_>) -> StoreError {
        match fail {
            Fail::Unavailable => StoreError::Unavailable(Cause::new(
                "angzarr-store-sqlite-unavailable",
                [
                    ("errno", angzarr_ports::Arg::Uint(5)),
                    (
                        "ext_err_txt",
                        angzarr_ports::Arg::Str("database is locked".to_string()),
                    ),
                ],
            )),
            Fail::Conflict => StoreError::conflict(stream, 0, 1),
        }
    }
}

impl EventStore for Flaky {
    async fn write(&self, write: Write<'_>) -> Result<Landed, StoreError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        let next = self.fails.lock().unwrap().pop_front();
        if let Some(fail) = next.or_else(|| self.always.clone()) {
            let stream = write.events.map_or(nowhere(), |append| append.stream);
            return Err(Flaky::failure(fail, stream));
        }
        self.inner.write(write).await
    }

    async fn read(
        &self,
        stream: StreamRef<'_>,
        from: u64,
        to: Option<u64>,
    ) -> Result<Slice, StoreError> {
        self.inner.read(stream, from, to).await
    }

    async fn head(&self, stream: StreamRef<'_>) -> Result<Head, StoreError> {
        self.inner.head(stream).await
    }

    async fn find_command(
        &self,
        stream: StreamRef<'_>,
        command_id: angzarr_ports::Uuid,
    ) -> Result<Vec<EventPage>, StoreError> {
        self.inner.find_command(stream, command_id).await
    }

    async fn covers(&self, domain: &str) -> Result<Vec<Cover>, StoreError> {
        self.inner.covers(domain).await
    }
}

impl SnapshotStore for Flaky {
    async fn read_snapshot(&self, stream: StreamRef<'_>) -> Result<Option<Snapshot>, StoreError> {
        self.inner.read_snapshot(stream).await
    }

    async fn write_snapshot(
        &self,
        stream: StreamRef<'_>,
        snapshot: &Snapshot,
    ) -> Result<(), StoreError> {
        self.inner.write_snapshot(stream, snapshot).await
    }
}

impl PositionStore for Flaky {
    async fn position(&self, key: PositionKey<'_>) -> Result<u64, StoreError> {
        self.inner.position(key).await
    }
}

impl ReadStore for Flaky {
    type Reads<'s> = <MemoryStore as ReadStore>::Reads<'s>;

    async fn begin_read(&self) -> Result<Self::Reads<'_>, StoreError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.begin_read().await
    }
}

impl Purge for Flaky {
    async fn purge(&self, stream: StreamRef<'_>) -> Result<u64, StoreError> {
        let meanwhile = self.before_purge.lock().unwrap().take();
        if let Some(meanwhile) = meanwhile {
            meanwhile(&self.inner);
        }
        self.inner.purge(stream).await
    }
}

impl Watch for Flaky {
    async fn change_token(&self) -> Result<ChangeToken, StoreError> {
        self.inner.change_token().await
    }
}

/// One project's store, made on the first write.
#[derive(Default)]
struct Stores {
    store: Flaky,
    made: bool,
}

impl TaskStores for Stores {
    type Store = Flaky;

    async fn open(&mut self, _: &Project) -> Result<&Flaky, StoreError> {
        self.made = true;
        Ok(&self.store)
    }

    async fn open_existing(&mut self, _: &Project) -> Result<Option<&Flaky>, StoreError> {
        Ok(self.made.then_some(&self.store))
    }
}

impl WatchedStores for Stores {
    async fn change_token(&mut self, _: &Project) -> Result<Option<HeldToken>, StoreError> {
        if !self.made {
            return Ok(None);
        }
        Ok(Some(HeldToken {
            handle: 1,
            change: self.store.change_token().await?,
        }))
    }
}

/// The fake clock's ids, as angzarr's: numbered from 1, as its own are.
impl CommandIds for FakeClock {
    fn command_id(&mut self) -> Uuid {
        let annals = ports::CommandIds::command_id(self);
        Uuid::from_bytes(annals.value[..].try_into().expect("16 bytes"))
    }
}

type Fake = Service<FakeIdentity, Stores, FakeNames, FakeClock, FakeDelay>;

fn service(store: Flaky, harps: &[&str]) -> Fake {
    Service::new(
        FakeIdentity::default(),
        Stores { store, made: false },
        FakeNames::of(harps),
        FakeClock::default(),
        FakeDelay::default(),
    )
}

/// The stream a failed write names when it wrote to none.
fn nowhere() -> StreamRef<'static> {
    StreamRef::main(TASK_DOMAIN, angzarr_ports::Uuid::from_bytes([0; 16]))
}

fn cover(harp: &str) -> Cover {
    Cover {
        domain: TASK_DOMAIN.to_string(),
        root: Some(ids::compute_root(TASK_DOMAIN, harp).unwrap()),
        edition: None,
    }
}

/// The events the store holds for `harp`'s task, oldest first.
fn events(store: &MemoryStore, harp: &str) -> Vec<TaskEvent> {
    let cover = cover(harp);
    ready(store.read(StreamRef::try_from(&cover).unwrap(), 0, None))
        .unwrap()
        .pages
        .iter()
        .map(|page| TaskEvent::from_any(page.payload.as_ref().unwrap()).unwrap())
        .collect()
}

fn new_task(text: &str) -> NewTask {
    NewTask {
        text: text.to_string(),
        ..NewTask::default()
    }
}

fn done(harp: &str) -> StatusChange {
    StatusChange {
        harp_id: harp.to_string(),
        status: Some("Done".to_string()),
        ..StatusChange::default()
    }
}

// ------------------------------------------------------------ commands

#[test]
fn test_an_add_runs_through_the_kernel_and_is_stored_as_task_added() {
    let mut service = service(Flaky::default(), &[HARP]);
    let written = ready(service.add(new_task("Ship it"))).unwrap();
    assert_eq!(written.task.harp_id, HARP);
    assert_eq!(written.task.text, "Ship it");
    assert_eq!(written.project, project());
    assert!(written.dead_letters.is_empty());
    let stored = events(&service.stores.store.inner, HARP);
    assert!(
        matches!(&stored[..], [TaskEvent::TaskAdded(added)] if added.text == "Ship it" && added.at.seconds > 1_700_000_000),
        "{stored:?}"
    );
}

#[test]
fn test_a_change_where_there_is_no_store_is_refused_no_task_and_makes_none() {
    let mut service = service(Flaky::default(), &[]);
    service.identity.bound = Some(project());
    let error = ready(service.set_status(done(HARP))).unwrap_err();
    assert_eq!(error.code(), "tasks-no-task");
    assert!(!service.stores.made, "no store made to say so");
}

#[test]
fn test_a_change_to_a_harp_with_no_task_is_refused_no_task() {
    let mut service = service(Flaky::default(), &[HARP]);
    ready(service.add(new_task("Ship it"))).unwrap();
    let error = ready(service.set_status(done(OTHER))).unwrap_err();
    assert_eq!(error.code(), "tasks-no-task");
}

#[test]
fn test_a_retried_request_is_answered_as_the_task_stands_and_applies_once() {
    let mut service = service(Flaky::default(), &[HARP]);
    ready(service.add(new_task("Ship it"))).unwrap();
    let change = StatusChange {
        request_id: Some("req-1".to_string()),
        ..done(HARP)
    };
    let first = ready(service.set_status(change.clone())).unwrap();
    let again = ready(service.set_status(change)).unwrap();
    assert_eq!(again.task, first.task);
    let stored = events(&service.stores.store.inner, HARP);
    assert_eq!(stored.len(), 2, "the add and one status change: {stored:?}");
}

#[test]
fn test_the_application_has_the_kernel_retry_nothing() {
    assert_eq!(KERNEL_RETRY.max_retries, 0);
    assert_eq!(
        OnRetryable {
            max_retries: 0,
            ..OnRetryable::default()
        },
        KERNEL_RETRY,
        "the kernel's own backoff otherwise, unused"
    );
}

// ------------------------------------------------------------ one retry loop

#[test]
fn test_a_store_that_could_not_say_is_retried_by_taskloom_alone() {
    let flaky = Flaky::failing(&[Fail::Unavailable, Fail::Unavailable]);
    let mut service = service(flaky, &[HARP]);
    let written = ready(service.add(new_task("Ship it"))).unwrap();
    assert_eq!(
        service.stores.store.writes.load(Ordering::SeqCst),
        3,
        "two failures, then the write: one attempt each, never the kernel's too"
    );
    assert_eq!(
        service.delay.slept.len(),
        2,
        "taskloom's waits, one a failure"
    );
    assert!(written.dead_letters.is_empty(), "nothing dead-lettered");
    assert_eq!(events(&service.stores.store.inner, HARP).len(), 1);
}

#[test]
fn test_a_store_that_never_answers_is_given_up_after_taskloom_s_attempts_alone() {
    let flaky = Flaky {
        always: Some(Fail::Unavailable),
        ..Flaky::default()
    };
    let mut service = service(flaky, &[HARP]);
    let attempts = service.backoff.attempts;
    let error = ready(service.add(new_task("Ship it"))).unwrap_err();
    assert!(
        matches!(&error, AppError::GaveUp { attempts: n, error } if *n == attempts && error.code() == "angzarr-ports-store-unavailable"),
        "{error:?}"
    );
    assert_eq!(
        service.stores.store.writes.load(Ordering::SeqCst),
        attempts,
        "taskloom's attempts, not the kernel's retries times them"
    );
    assert!(
        error.dead_letters().is_empty(),
        "the person is told, nothing dead-lettered"
    );
}

#[test]
fn test_a_lost_race_is_retried_under_the_same_command_id_and_lands_once() {
    let flaky = Flaky::failing(&[Fail::Conflict]);
    let mut service = service(flaky, &[HARP]);
    ready(service.add(new_task("Ship it"))).unwrap();
    ready(service.set_status(done(HARP))).unwrap();
    assert_eq!(service.stores.store.writes.load(Ordering::SeqCst), 3);
    assert_eq!(service.delay.slept.len(), 1);
    assert_eq!(events(&service.stores.store.inner, HARP).len(), 2);
}

#[test]
fn test_a_failure_no_retry_mends_is_not_retried() {
    // A refusal is the domain's: one attempt, no wait.
    let mut service = service(Flaky::default(), &[HARP]);
    ready(service.add(new_task("Ship it"))).unwrap();
    let blank = StatusChange {
        status: None,
        ..done(HARP)
    };
    let writes = service.stores.store.writes.load(Ordering::SeqCst);
    let error = ready(service.set_status(blank)).unwrap_err();
    assert_eq!(error.code(), "tasks-status-required");
    assert_eq!(service.stores.store.writes.load(Ordering::SeqCst), writes);
    assert!(service.delay.slept.is_empty());
}

// ------------------------------------------------------------ prune

fn prune_done() -> PruneRequest {
    PruneRequest {
        statuses: vec!["Done".to_string()],
        older_than: "7d".to_string(),
        apply: true,
        ..PruneRequest::default()
    }
}

/// Eight days on: past a `7d` prune's cut.
fn later(service: &mut Fake) {
    service.clock.seconds += 8 * 86_400;
}

#[test]
fn test_a_prune_records_the_removal_then_purges_through_purge_and_frees_the_harp() {
    let mut service = service(Flaky::default(), &[HARP, HARP]);
    ready(service.add(new_task("Ship it"))).unwrap();
    ready(service.set_status(done(HARP))).unwrap();
    later(&mut service);
    let pruning = ready(service.prune(prune_done())).unwrap();
    assert_eq!(pruning.pruned.len(), 1);
    assert_eq!(
        pruning.events_removed, 3,
        "the add, the status, the removal"
    );
    assert!(
        events(&service.stores.store.inner, HARP).is_empty(),
        "purged"
    );
    let again = ready(service.add(new_task("Again"))).unwrap();
    assert_eq!(again.task.harp_id, HARP, "the freed harp, minted again");
}

#[test]
fn test_a_prune_never_purges_a_task_minted_on_its_harp_since_it_read() {
    // Another prune purges the stream after this one read it removed, and
    // a task is minted on the freed harp, all before this prune's purge
    // lands: the new task is not this prune's to purge.
    let mut service = service(Flaky::default(), &[HARP]);
    ready(service.add(new_task("Ship it"))).unwrap();
    ready(service.set_status(done(HARP))).unwrap();
    later(&mut service);
    *service.stores.store.before_purge.lock().unwrap() = Some(Box::new(|inner: &MemoryStore| {
        let cover = cover(HARP);
        ready(inner.purge(StreamRef::try_from(&cover).unwrap())).unwrap();
        mint_elsewhere(inner, HARP, "Minted since");
    }));
    ready(service.prune(prune_done())).unwrap();
    let stored = events(&service.stores.store.inner, HARP);
    assert!(
        matches!(&stored[..], [TaskEvent::TaskAdded(added)] if added.text == "Minted since"),
        "the task minted since is kept: {stored:?}"
    );
}

/// A task added to `harp` on `store` by another process's kernel.
fn mint_elsewhere(store: &MemoryStore, harp: &str, text: &str) {
    let mut registry = Registry::new();
    registry.aggregate::<TaskAggregate>(TaskAggregate::default);
    let add = AddTask {
        harp_id: harp.to_string(),
        text: text.to_string(),
        status: None,
        trigger: None,
        tags: Vec::new(),
        origin_session: None,
        defers: tasks::DEFERRED.to_string(),
        initial: tasks::TO_DO.to_string(),
    };
    let book = CommandBook {
        cover: Some(cover(harp)),
        page: Some(CommandPage {
            body: Some(command_page::Body::Command(Command {
                payload: Some(TaskCommand::from(add).to_any()),
            })),
            ..CommandPage::default()
        }),
        handling: None,
    };
    let sampled = Sampled {
        now: angzarr_embedded::next::Timestamp {
            seconds: 1_800_000_000,
            nanos: 0,
        },
        command_id: Uuid::from_bytes([0xee; 16]),
    };
    let dead_letters = MemoryDeadLetters::new();
    ready(
        Kernel::new(&registry, &dead_letters)
            .on_retryable(KERNEL_RETRY)
            .handle::<TaskAggregate, _, _>(store, book, &sampled, &mut ()),
    )
    .unwrap_or_else(|failure| panic!("{failure:?}"));
}

// ------------------------------------------------------------ watch

fn watching(service: &mut Fake) -> Watching {
    ready(service.watch(WatchRequest::default()))
        .unwrap()
        .expect("a project to watch")
}

fn snapshot_harps(polled: &Polled) -> Vec<&str> {
    match polled {
        Polled::Snapshot { tasks, .. } => tasks.iter().map(|t| t.harp_id.as_str()).collect(),
        Polled::Changes { .. } => panic!("a snapshot, not changes: {polled:?}"),
    }
}

/// A resumed watcher's changes as (harp, status), a removal as (harp,
/// "removed"), and its cursor after.
fn changes(polled: &Polled) -> (Vec<(&str, &str)>, &WatchCursor) {
    let Polled::Changes { changes, cursor } = polled else {
        panic!("changes, not a snapshot: {polled:?}");
    };
    let said = changes
        .iter()
        .map(|change| match change {
            Changed::Task(task) => (task.harp_id.as_str(), task.status.as_str()),
            Changed::Removed { harp_id } => (harp_id.as_str(), "removed"),
        })
        .collect();
    (said, cursor)
}

fn cursor_of(polled: &Polled) -> WatchCursor {
    match polled {
        Polled::Snapshot { cursor, .. } | Polled::Changes { cursor, .. } => cursor.clone(),
    }
}

#[test]
fn test_a_watcher_with_no_cursor_is_answered_a_snapshot_and_the_cursor_to_resume_from() {
    let mut service = service(Flaky::default(), &[HARP, OTHER]);
    ready(service.add(new_task("First"))).unwrap();
    ready(service.add(new_task("Second"))).unwrap();
    let watch = watching(&mut service);
    let first = watch.since(None);
    assert_eq!(snapshot_harps(&first), [OTHER, HARP], "by harp id");
    let cursor = cursor_of(&first);
    assert!(cursor.instance.is_some(), "the list's instance");
    assert_eq!(
        cursor.streams,
        BTreeMap::from([(OTHER.to_string(), 0), (HARP.to_string(), 0)]),
        "each stream's own sequence, by harp"
    );
}

#[test]
fn test_a_watcher_resumes_from_its_cursor_to_only_what_moved() {
    let mut service = service(Flaky::default(), &[HARP, OTHER]);
    ready(service.add(new_task("First"))).unwrap();
    ready(service.add(new_task("Second"))).unwrap();
    let mut watch = watching(&mut service);
    let cursor = cursor_of(&watch.since(None));
    ready(service.set_status(done(HARP))).unwrap();
    assert!(ready(service.poll(&mut watch)).unwrap(), "the token moved");
    let polled = watch.since(Some(&cursor));
    let (said, after) = changes(&polled);
    assert_eq!(said, [(HARP, "Done")]);
    assert_eq!(after.streams[HARP], 1);
    assert_eq!(after.instance, cursor.instance);
}

#[test]
fn test_a_poll_whose_token_has_not_moved_reads_no_stream() {
    let mut service = service(Flaky::default(), &[HARP]);
    ready(service.add(new_task("First"))).unwrap();
    let mut watch = watching(&mut service);
    let reads = service.stores.store.reads.load(Ordering::SeqCst);
    assert!(
        !ready(service.poll(&mut watch)).unwrap(),
        "nothing committed"
    );
    assert_eq!(service.stores.store.reads.load(Ordering::SeqCst), reads);
}

#[test]
fn test_a_watch_where_there_is_no_store_yet_answers_an_empty_snapshot_until_one_appears() {
    let mut service = service(Flaky::default(), &[HARP]);
    service.identity.bound = Some(project());
    let mut watch = watching(&mut service);
    assert_eq!(snapshot_harps(&watch.since(None)), Vec::<&str>::new());
    ready(service.add(new_task("First"))).unwrap();
    assert!(ready(service.poll(&mut watch)).unwrap(), "a store appeared");
    assert_eq!(snapshot_harps(&watch.since(None)), [HARP]);
}

#[test]
fn test_a_purge_resyncs_every_watcher_with_a_snapshot() {
    let mut service = service(Flaky::default(), &[HARP, OTHER]);
    ready(service.add(new_task("First"))).unwrap();
    ready(service.add(new_task("Second"))).unwrap();
    ready(service.set_status(done(HARP))).unwrap();
    let mut watch = watching(&mut service);
    let cursor = cursor_of(&watch.since(None));
    later(&mut service);
    ready(service.prune(prune_done())).unwrap();
    ready(service.poll(&mut watch)).unwrap();
    let polled = watch.since(Some(&cursor));
    assert_eq!(snapshot_harps(&polled), [OTHER], "resynced, not resumed");
    assert_ne!(
        cursor_of(&polled).instance,
        cursor.instance,
        "a new instance"
    );
}

#[test]
fn test_a_cursor_from_another_watch_is_answered_a_snapshot() {
    // Another process's watch: another list, another instance.
    let mut service = service(Flaky::default(), &[HARP]);
    ready(service.add(new_task("First"))).unwrap();
    let theirs = cursor_of(&watching(&mut service).since(None));
    let ours = watching(&mut service);
    assert_eq!(snapshot_harps(&ours.since(Some(&theirs))), [HARP]);
}

#[test]
fn test_a_cursor_read_back_from_the_wire_resumes() {
    let mut service = service(Flaky::default(), &[HARP]);
    ready(service.add(new_task("First"))).unwrap();
    let mut watch = watching(&mut service);
    let sent = cursor_of(&watch.since(None));
    let read_back = WatchCursor {
        instance: sent.instance.clone(),
        streams: sent.streams.clone(),
    };
    ready(service.set_status(done(HARP))).unwrap();
    ready(service.poll(&mut watch)).unwrap();
    assert_eq!(changes(&watch.since(Some(&read_back))).0, [(HARP, "Done")]);
}
