//! Watching: a watcher resumed from its cursor is answered what was
//! committed since, and nothing twice; a watch binds no project and waits
//! for a store to appear; a purge, or another watch's cursor, is answered
//! a snapshot.

use std::collections::BTreeMap;

use angzarr_embedded::next::angzarr_ports::memory::MemoryStore;
use angzarr_embedded_diagnostic::Diagnostic;
use tasks::{AddTask, RemovalReason, RemoveTask, TaskCommand};

use super::{Changed, Polled, Watch, WatchCursor, WatchRequest, WatchService};
use crate::fakes::{project, ready, FakeClock, FakeDelay, FakeIdentity, FakeNames, FakeStores};
use crate::{book, retry, AppError, NewTask, Service, TaskService};

type Fake = Service<FakeIdentity, FakeStores, FakeNames, FakeClock, FakeDelay>;

fn service(names: &[&str]) -> Fake {
    Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        FakeNames::of(names),
        FakeClock::default(),
        FakeDelay::default(),
    )
}

fn add(service: &mut Fake, text: &str) -> String {
    let task = NewTask {
        text: text.to_string(),
        ..NewTask::default()
    };
    ready(service.add(task)).expect("added").task.harp_id
}

/// Remove `harp` straight through the kernel: no use case removes yet.
fn remove(service: &mut Fake, harp: &str) {
    remove_for(service, harp, RemovalReason::Removed);
}

/// Remove `harp` for `reason` straight through the kernel: a prune's
/// removal, without its purge, when `Pruned`.
fn remove_for(service: &mut Fake, harp: &str, reason: RemovalReason) {
    let command = RemoveTask { reason };
    let book = book(harp, command.into());
    let command_id = ports::CommandIds::command_id(&mut service.clock);
    let mut runner = retry::Runner {
        registry: &service.registry,
        clock: &mut service.clock,
        delay: &mut service.delay,
        backoff: service.backoff,
        dead_letters: &service.dead_letters,
    };
    let outcome = ready(runner.run(&service.stores.store, &book, &command_id));
    assert!(matches!(outcome, Ok(retry::Outcome::Committed)));
}

/// A watch and its watcher's cursor: what a subscriber holds.
struct Watcher {
    watch: Watch,
    cursor: WatchCursor,
}

impl Watcher {
    /// Start watching, the watcher taking the first answer whole.
    fn start(service: &mut Fake, request: WatchRequest) -> Watcher {
        let watch = ready(service.watch(request))
            .expect("resolves")
            .expect("a project");
        let cursor = cursor_of(&watch.since(None));
        Watcher { watch, cursor }
    }

    /// Poll, and resume the watcher from its cursor: what moved since, or
    /// `None` for a snapshot.
    fn poll(&mut self, service: &mut Fake) -> Result<Option<Vec<Changed>>, AppError> {
        let polled = ready(service.poll(&mut self.watch, Some(&self.cursor)))?;
        self.cursor = cursor_of(&polled);
        Ok(match polled {
            Polled::Changes { changes, .. } => Some(changes),
            Polled::Snapshot { .. } => None,
        })
    }

    /// [`Watcher::poll`]'s changes, when it resumed.
    fn changes(&mut self, service: &mut Fake) -> Vec<Changed> {
        self.poll(service)
            .unwrap()
            .expect("resumed, not a snapshot")
    }
}

fn cursor_of(polled: &Polled) -> WatchCursor {
    match polled {
        Polled::Changes { cursor, .. } | Polled::Snapshot { cursor, .. } => cursor.clone(),
    }
}

fn texts(changes: &[Changed]) -> Vec<String> {
    changes
        .iter()
        .map(|c| match c {
            Changed::Task(task) => format!("{} {}", task.harp_id, task.status),
            Changed::Removed { harp_id } => format!("{harp_id} removed"),
        })
        .collect()
}

#[test]
fn test_a_watch_where_there_is_no_project_is_none_and_binds_none() {
    let mut service = service(&[]);
    let watch = ready(service.watch(WatchRequest::default())).expect("resolves");
    assert!(watch.is_none());
    assert_eq!(service.identity.bound, None, "a watch binds no project");
    assert!(service.stores.opened.is_empty(), "nor creates a store");
}

#[test]
fn test_a_watch_starts_caught_up_so_a_poll_answers_only_what_came_after() {
    let mut service = service(&["swift-amber", "quiet-heron"]);
    add(&mut service, "Before");
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    assert_eq!(watch.watch.project, project());
    assert_eq!(watch.changes(&mut service), vec![]);

    add(&mut service, "After");
    let changes = watch.changes(&mut service);
    assert_eq!(texts(&changes), ["quiet-heron To Do"]);
    assert_eq!(watch.changes(&mut service), vec![], "nothing twice");
}

#[test]
fn test_a_poll_answers_every_commit_since_the_last_in_order_and_a_removal() {
    let mut service = service(&["swift-amber", "quiet-heron"]);
    add(&mut service, "One");
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    add(&mut service, "Two");
    remove(&mut service, "swift-amber");
    let changes = watch.changes(&mut service);
    // The projector takes the streams in the store's order, by root.
    let mut seen = texts(&changes);
    seen.sort();
    assert_eq!(seen, ["quiet-heron To Do", "swift-amber removed"]);
}

#[test]
fn test_a_watch_of_a_project_with_no_store_yet_waits_for_one() {
    let mut service = service(&["swift-amber"]);
    service.identity.bound = Some(project());
    service.stores.absent = true;
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    assert_eq!(watch.changes(&mut service), vec![]);
    assert!(service.stores.opened.is_empty(), "a poll creates no store");

    add(&mut service, "First");
    let changes = watch.changes(&mut service);
    assert_eq!(texts(&changes), ["swift-amber To Do"]);
}

#[test]
fn test_a_pinned_project_is_watched_and_a_store_that_fails_is_an_error() {
    let mut service = service(&[]);
    let request = WatchRequest {
        project: Some("jolly-mute-magma".to_string()),
    };
    let mut watch = Watcher::start(&mut service, request);
    assert_eq!(
        service.identity.pins,
        [Some("jolly-mute-magma".to_string())]
    );
    service.stores.store.open_fails = true;
    let failed = watch.poll(&mut service).unwrap_err();
    assert!(matches!(failed, AppError::Failed(_)), "{failed:?}");
}

#[test]
fn test_a_watch_whose_project_does_not_resolve_says_why() {
    let mut service = service(&[]);
    let why = angzarr_embedded_diagnostic::Report::of(&AppError::NoProject);
    service.identity.fails = Some(why);
    let failed = ready(service.watch(WatchRequest::default()))
        .err()
        .expect("an error");
    assert!(matches!(failed, AppError::Identity(_)));
    assert_eq!(failed.code(), "tasks-application-no-project");
}

/// Add `harp` to `store` directly, as another process would.
fn add_elsewhere(store: &MemoryStore, harp: &str) {
    let command: TaskCommand = AddTask {
        harp_id: harp.to_string(),
        text: "Elsewhere".to_string(),
        status: None,
        trigger: None,
        tags: Vec::new(),
        origin_session: None,
        defers: tasks::DEFERRED.to_string(),
        initial: tasks::TO_DO.to_string(),
    }
    .into();
    crate::fakes::run(store, harp, command);
}

// ------------------------------------------------------------ the token

#[test]
fn test_a_poll_whose_token_has_not_moved_reads_no_stream() {
    let mut service = service(&["swift-amber", "quiet-heron"]);
    add(&mut service, "One");
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    let reads = service.stores.store.reads();
    for _ in 0..3 {
        assert_eq!(watch.changes(&mut service), vec![]);
    }
    assert_eq!(
        service.stores.store.reads(),
        reads,
        "nothing committed, nothing read"
    );

    add(&mut service, "Two");
    // The add reads its task back: count from after it.
    let reads = service.stores.store.reads();
    let changes = watch.changes(&mut service);
    assert_eq!(texts(&changes), ["quiet-heron To Do"]);
    assert_eq!(service.stores.store.reads(), reads + 1, "read once for it");
    assert_eq!(watch.changes(&mut service), vec![]);
    assert_eq!(service.stores.store.reads(), reads + 1);
}

#[test]
fn test_a_commit_landing_as_the_token_is_asked_for_is_caught_up_at_that_poll() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    // Another process commits just before the token is read: the token
    // covers it, so the poll must read after it (a poll that read the
    // streams first and the token after would never see it).
    service.stores.before_token = Some(Box::new(|store| add_elsewhere(store, "far-heron")));
    let changes = watch.changes(&mut service);
    assert_eq!(texts(&changes), ["far-heron To Do"]);
}

#[test]
fn test_a_commit_by_another_process_between_polls_is_caught_up() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    assert_eq!(watch.changes(&mut service), vec![]);
    add_elsewhere(&service.stores.store.inner, "far-heron");
    let changes = watch.changes(&mut service);
    assert_eq!(texts(&changes), ["far-heron To Do"]);
}

#[test]
fn test_a_store_opened_again_is_read_again_whatever_its_token() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    let reads = service.stores.store.reads();
    // A new handle: its tokens say nothing about the old one's.
    service.stores.handle += 1;
    assert_eq!(watch.changes(&mut service), vec![]);
    assert_eq!(service.stores.store.reads(), reads + 1);
    assert_eq!(watch.changes(&mut service), vec![]);
    assert_eq!(service.stores.store.reads(), reads + 1, "and held after");
}

#[test]
fn test_a_token_that_moved_with_nothing_new_answers_nothing() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    let reads = service.stores.store.reads();
    // A purge of a stream there never was: a commit with nothing in it.
    crate::fakes::purge(&service.stores.store.inner, "no-such-task");
    assert_eq!(watch.changes(&mut service), vec![]);
    assert_eq!(
        service.stores.store.reads(),
        reads + 1,
        "read, and nothing found"
    );
}

#[test]
fn test_a_watch_of_no_store_yet_reads_nothing_until_one_appears() {
    let mut service = service(&["swift-amber"]);
    service.identity.bound = Some(project());
    service.stores.absent = true;
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    for _ in 0..3 {
        assert_eq!(watch.changes(&mut service), vec![]);
    }
    assert_eq!(service.stores.store.reads(), 0);
    add(&mut service, "First");
    let reads = service.stores.store.reads();
    let changes = watch.changes(&mut service);
    assert_eq!(texts(&changes), ["swift-amber To Do"]);
    assert_eq!(watch.changes(&mut service), vec![]);
    assert_eq!(service.stores.store.reads(), reads + 1, "then the token");
}

// ------------------------------------------------------------ prune

#[test]
fn test_a_prune_landing_whole_between_polls_resyncs_the_watcher() {
    let mut service = service(&["swift-amber", "quiet-heron"]);
    add(&mut service, "Pruned");
    add(&mut service, "Kept");
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    remove_for(&mut service, "swift-amber", RemovalReason::Pruned);
    crate::fakes::purge(&service.stores.store.inner, "swift-amber");
    assert_eq!(watch.poll(&mut service).unwrap(), None, "a purge resyncs");
    let tasks: Vec<String> = match watch.watch.since(None) {
        Polled::Snapshot { tasks, .. } => tasks.into_iter().map(|t| t.harp_id).collect(),
        other => panic!("a snapshot: {other:?}"),
    };
    assert_eq!(tasks, ["quiet-heron"]);
    assert_eq!(watch.changes(&mut service), vec![], "then resumed");
}

/// Compiles only for a `Send` value.
fn is_send<T: Send>(_: &T) {}

/// Compiles only if the trait itself promises `Send` futures.
fn spawnable<W: WatchService + Send>(service: &mut W, watch: &mut Watch) {
    is_send(&service.poll(watch, None));
}

#[test]
fn test_watch_futures_are_send_so_a_server_can_spawn_them() {
    let mut service = service(&["swift-amber"]);
    is_send(&service.watch(WatchRequest::default()));
    add(&mut service, "One");
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    is_send(&watch.watch);
    spawnable(&mut service, &mut watch.watch);
}

#[test]
fn test_a_watcher_with_no_cursor_is_answered_every_task_and_the_cursor_to_resume_from() {
    let mut service = service(&["swift-amber", "quiet-heron"]);
    add(&mut service, "One");
    add(&mut service, "Two");
    let watch = ready(service.watch(WatchRequest::default()))
        .unwrap()
        .unwrap();
    let Polled::Snapshot { tasks, cursor } = watch.since(None) else {
        panic!("a snapshot");
    };
    let harps: Vec<&str> = tasks.iter().map(|t| t.harp_id.as_str()).collect();
    assert_eq!(harps, ["quiet-heron", "swift-amber"], "by harp id");
    assert!(cursor.instance.is_some(), "the list's instance");
    assert_eq!(
        cursor.streams,
        BTreeMap::from([
            ("quiet-heron".to_string(), 0),
            ("swift-amber".to_string(), 0)
        ]),
        "each stream's own sequence, by harp"
    );
}

#[test]
fn test_a_cursor_from_another_watch_is_answered_a_snapshot() {
    // Another process's watch: another list, another instance.
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let theirs = Watcher::start(&mut service, WatchRequest::default()).cursor;
    let ours = Watcher::start(&mut service, WatchRequest::default());
    assert!(matches!(
        ours.watch.since(Some(&theirs)),
        Polled::Snapshot { .. }
    ));
}

#[test]
fn test_a_cursor_read_back_from_the_wire_resumes() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let mut watch = Watcher::start(&mut service, WatchRequest::default());
    let read_back = WatchCursor {
        instance: watch.cursor.instance.clone(),
        streams: watch.cursor.streams.clone(),
    };
    let status = crate::StatusChange {
        harp_id: "swift-amber".to_string(),
        status: Some("Done".to_string()),
        ..crate::StatusChange::default()
    };
    ready(service.set_status(status)).unwrap();
    let polled = ready(service.poll(&mut watch.watch, Some(&read_back))).unwrap();
    let Polled::Changes { changes, cursor } = polled else {
        panic!("resumed");
    };
    assert_eq!(texts(&changes), ["swift-amber Done"]);
    assert_eq!(cursor.streams["swift-amber"], 1);
}

#[test]
fn test_a_watch_that_has_seen_nothing_answers_every_task_at_its_first_poll() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let mut watch = Watch::of(project());
    let polled = ready(service.poll(&mut watch, None)).unwrap();
    let Polled::Snapshot { tasks, .. } = polled else {
        panic!("a snapshot");
    };
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].harp_id, "swift-amber");
}
