//! Watching: a poll answers whether the store's change token moved since
//! the last, and reads no stream; a watch binds no project. A scan reads
//! the task streams from the positions it is handed, one event at a time
//! with the task as the event left it, and says which stream is gone.

use angzarr_diagnostic::Diagnostic;
use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::v1::CommandBook;
use angzarr_embedded::CommandBookExt;
use tasks::{AddTask, RemovalReason, RemoveTask, TaskCommand};

use super::{Found, Positions, Seen, StreamKey, Watch, WatchRequest, WatchService};
use crate::fakes::{project, ready, FakeClock, FakeDelay, FakeIdentity, FakeNames, FakeStores};
use crate::{AppError, NewTask, Service, StatusChange, TagChange, TaskService, TextEdit};

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
    let book = CommandBook::command_of(crate::task_cover(harp), &tasks::TaskCommand::from(command));
    let command_id = ports::CommandIds::command_id(&mut service.clock);
    let mut runner = kernel_runner::Runner {
        registry: &service.registry,
        clock: &mut service.clock,
        delay: &mut service.delay,
        retry: service.retry,
        dead_letters: &mut service.dead_letters,
    };
    let outcome = ready(runner.run::<tasks::TaskAggregate, tasks::TaskRejection, _>(
        &service.stores.store,
        book,
        &command_id,
    ));
    assert!(matches!(outcome, Ok(Ok(_))));
}

/// Start watching, as the project is now.
fn start(service: &mut Fake, request: WatchRequest) -> Watch {
    ready(service.watch(request))
        .expect("resolves")
        .expect("a project")
}

/// Whether a poll found the store changed.
fn changed(service: &mut Fake, watch: &mut Watch) -> bool {
    ready(service.poll(watch)).expect("polled")
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
fn test_a_watch_starts_as_the_store_is_so_a_poll_answers_only_what_came_after() {
    let mut service = service(&["swift-amber", "quiet-heron"]);
    add(&mut service, "Before");
    let mut watch = start(&mut service, WatchRequest::default());
    assert_eq!(watch.project, project());
    assert!(!changed(&mut service, &mut watch), "nothing since it began");

    add(&mut service, "After");
    assert!(changed(&mut service, &mut watch));
    assert!(!changed(&mut service, &mut watch), "a change once");
}

#[test]
fn test_a_burst_of_commits_between_polls_is_one_change() {
    let mut service = service(&["swift-amber", "quiet-heron"]);
    add(&mut service, "One");
    let mut watch = start(&mut service, WatchRequest::default());
    add(&mut service, "Two");
    remove(&mut service, "swift-amber");
    assert!(changed(&mut service, &mut watch));
    assert!(!changed(&mut service, &mut watch));
}

#[test]
fn test_a_watch_of_a_project_with_no_store_yet_waits_for_one() {
    let mut service = service(&["swift-amber"]);
    service.identity.bound = Some(project());
    service.stores.absent = true;
    let mut watch = start(&mut service, WatchRequest::default());
    for _ in 0..3 {
        assert!(!changed(&mut service, &mut watch));
    }
    assert!(service.stores.opened.is_empty(), "a poll creates no store");

    add(&mut service, "First");
    assert!(changed(&mut service, &mut watch), "the store appeared");
    assert!(!changed(&mut service, &mut watch));
}

#[test]
fn test_a_pinned_project_is_watched_and_a_store_that_fails_is_an_error() {
    let mut service = service(&[]);
    let request = WatchRequest {
        project: Some("jolly-mute-magma".to_string()),
    };
    let mut watch = start(&mut service, request);
    assert_eq!(
        service.identity.pins,
        [Some("jolly-mute-magma".to_string())]
    );
    service.stores.open_fails = true;
    let failed = ready(service.poll(&mut watch)).unwrap_err();
    assert!(matches!(failed, AppError::Failed(_)), "{failed:?}");
}

#[test]
fn test_a_watch_whose_project_does_not_resolve_says_why() {
    let mut service = service(&[]);
    let why = angzarr_diagnostic::Report::of(&AppError::NoProject);
    service.identity.fails = Some(why);
    let failed = ready(service.watch(WatchRequest::default()))
        .err()
        .expect("an error");
    assert!(matches!(failed, AppError::Identity(_)));
    assert_eq!(failed.code(), AppError::NoProject.code());
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
fn test_a_watch_reads_no_stream() {
    let mut service = service(&["swift-amber", "quiet-heron"]);
    add(&mut service, "One");
    let reads = service.stores.store.units();
    let mut watch = start(&mut service, WatchRequest::default());
    assert!(!changed(&mut service, &mut watch));
    add_elsewhere(service.stores.store.inner(), "far-heron");
    assert!(changed(&mut service, &mut watch));
    assert_eq!(
        service.stores.store.units(),
        reads,
        "only the token is read"
    );
}

#[test]
fn test_a_commit_landing_as_the_token_is_asked_for_is_that_polls_change() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let mut watch = start(&mut service, WatchRequest::default());
    // Another process commits just before the token is read: the token
    // covers it, so this poll is the change, and the next is not.
    service.stores.before_token = Some(Box::new(|store| add_elsewhere(store, "far-heron")));
    assert!(changed(&mut service, &mut watch));
    assert!(!changed(&mut service, &mut watch));
}

#[test]
fn test_a_commit_by_another_process_between_polls_is_a_change() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let mut watch = start(&mut service, WatchRequest::default());
    assert!(!changed(&mut service, &mut watch));
    add_elsewhere(service.stores.store.inner(), "far-heron");
    assert!(changed(&mut service, &mut watch));
}

#[test]
fn test_a_store_opened_again_is_a_change_whatever_its_token() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let mut watch = start(&mut service, WatchRequest::default());
    // A new handle: its tokens say nothing about the old one's.
    service.stores.handle += 1;
    assert!(changed(&mut service, &mut watch));
    assert!(!changed(&mut service, &mut watch), "and held after");
}

#[test]
fn test_a_token_that_moved_with_no_task_changed_is_a_change() {
    // The owner's ruling (D7, 2026-10-30): the watch keeps no tasks to
    // compare, so a commit no task sees is a change; the subscriber
    // re-queries and finds the same tasks.
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let mut watch = start(&mut service, WatchRequest::default());
    crate::fakes::purge(service.stores.store.inner(), "no-such-task");
    assert!(changed(&mut service, &mut watch));
}

#[test]
fn test_a_prune_landing_between_polls_is_a_change() {
    let mut service = service(&["swift-amber", "quiet-heron"]);
    add(&mut service, "Pruned");
    add(&mut service, "Kept");
    let mut watch = start(&mut service, WatchRequest::default());
    remove_for(&mut service, "swift-amber", RemovalReason::Pruned);
    crate::fakes::purge(service.stores.store.inner(), "swift-amber");
    assert!(changed(&mut service, &mut watch));
    assert!(!changed(&mut service, &mut watch));
}

#[test]
fn test_a_watch_that_has_seen_no_token_changes_at_its_first_poll() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, "One");
    let mut watch = Watch::of(project());
    assert!(changed(&mut service, &mut watch));
    assert!(!changed(&mut service, &mut watch));
}

/// Compiles only for a `Send` value.
fn is_send<T: Send>(_: &T) {}

/// Compiles only if the trait itself promises `Send` futures.
fn spawnable<W: WatchService + Send>(service: &mut W, watch: &mut Watch) {
    is_send(&service.poll(watch));
    is_send(&service.heads(watch));
    is_send(&service.after(watch, &Positions::new()));
}

#[test]
fn test_watch_futures_are_send_so_a_server_can_spawn_them() {
    let mut service = service(&["swift-amber"]);
    is_send(&service.watch(WatchRequest::default()));
    add(&mut service, "One");
    let mut watch = start(&mut service, WatchRequest::default());
    is_send(&watch);
    spawnable(&mut service, &mut watch);
}

// ------------------------------------------------------------ the scan

type Permitting = Service<FakeIdentity, FakeStores, FakeNames, FakeClock, FakeDelay>;

/// A service whose store admits every tag, naming tasks as `names` say.
fn permitting(names: &[&str]) -> Permitting {
    Service::new(
        FakeIdentity::bound(),
        FakeStores::permitting_every_tag(),
        FakeNames::of(names),
        FakeClock::default(),
        FakeDelay::default(),
    )
}

fn key_of(harp: &str) -> StreamKey {
    let cover = crate::task_cover(harp);
    let stream = angzarr_embedded::StreamRef::try_from(&cover).unwrap();
    StreamKey {
        edition: None,
        root: stream.root,
    }
}

fn heads(service: &mut Permitting, watch: &Watch) -> Positions {
    ready(service.heads(watch)).expect("heads")
}

fn after(service: &mut Permitting, watch: &Watch, positions: &Positions) -> Vec<Found> {
    ready(service.after(watch, positions)).expect("a scan")
}

fn event(harp: &str, sequence: u64, text: &str, status: &str, tags: &[&str]) -> Found {
    Found::Event {
        stream: key_of(harp),
        sequence,
        task: Seen {
            harp_id: harp.to_string(),
            text: text.to_string(),
            status: status.to_string(),
            tags: tags.iter().map(ToString::to_string).collect(),
            removed: false,
        },
    }
}

/// Edit, move and tag `harp`: its events 1, 2 and 3.
fn change(service: &mut Permitting, harp: &str) {
    ready(service.edit(TextEdit {
        harp_id: harp.to_string(),
        text: "Beta".to_string(),
        ..TextEdit::default()
    }))
    .expect("edited");
    ready(service.set_status(StatusChange {
        harp_id: harp.to_string(),
        status: Some("In Progress".to_string()),
        ..StatusChange::default()
    }))
    .expect("moved");
    ready(service.tag(TagChange {
        harp_id: harp.to_string(),
        add: vec!["area:cli".to_string()],
        ..TagChange::default()
    }))
    .expect("tagged");
}

#[test]
fn test_the_heads_are_each_task_streams_next_sequence_and_no_other_domains() {
    let mut service = permitting(&["swift-amber", "quiet-heron"]);
    add(&mut service, "One");
    add(&mut service, "Two");
    change(&mut service, "swift-amber");
    let watch = start(&mut service, WatchRequest::default());
    // The schema stream (written by the first add) is not a task stream.
    let expected: Positions = [(key_of("swift-amber"), 4), (key_of("quiet-heron"), 1)].into();
    assert_eq!(heads(&mut service, &watch), expected);
}

#[test]
fn test_the_heads_of_a_project_with_no_store_are_none_and_a_scan_finds_nothing() {
    let mut service = permitting(&[]);
    service.stores.absent = true;
    let watch = Watch::of(project());
    assert_eq!(heads(&mut service, &watch), Positions::new());
    assert_eq!(after(&mut service, &watch, &Positions::new()), []);
    assert!(service.stores.opened.is_empty(), "a scan creates no store");
}

#[test]
fn test_a_scan_from_nothing_reads_every_stream_from_its_start_with_the_task_at_each_event() {
    let mut service = permitting(&["swift-amber"]);
    add(&mut service, "Alpha");
    change(&mut service, "swift-amber");
    let watch = start(&mut service, WatchRequest::default());
    assert_eq!(
        after(&mut service, &watch, &Positions::new()),
        [
            event("swift-amber", 0, "Alpha", "To Do", &[]),
            event("swift-amber", 1, "Beta", "To Do", &[]),
            event("swift-amber", 2, "Beta", "In Progress", &[]),
            event("swift-amber", 3, "Beta", "In Progress", &["area:cli"]),
        ]
    );
}

#[test]
fn test_a_scan_from_a_position_starts_there_with_the_state_the_events_before_it_left() {
    let mut service = permitting(&["swift-amber"]);
    add(&mut service, "Alpha");
    change(&mut service, "swift-amber");
    let watch = start(&mut service, WatchRequest::default());
    let from: Positions = [(key_of("swift-amber"), 2)].into();
    assert_eq!(
        after(&mut service, &watch, &from),
        [
            event("swift-amber", 2, "Beta", "In Progress", &[]),
            event("swift-amber", 3, "Beta", "In Progress", &["area:cli"]),
        ]
    );
    // At the head: nothing.
    let head = heads(&mut service, &watch);
    assert_eq!(after(&mut service, &watch, &head), []);
}

#[test]
fn test_a_stream_the_positions_lack_is_read_from_its_start() {
    let mut service = permitting(&["swift-amber", "quiet-heron"]);
    add(&mut service, "Old");
    let watch = start(&mut service, WatchRequest::default());
    let head = heads(&mut service, &watch);
    add(&mut service, "New");
    assert_eq!(
        after(&mut service, &watch, &head),
        [event("quiet-heron", 0, "New", "To Do", &[])]
    );
}

#[test]
fn test_a_stream_shorter_than_its_position_is_expired_and_read_again_from_its_start() {
    let mut service = permitting(&["swift-amber"]);
    add(&mut service, "Alpha");
    let watch = start(&mut service, WatchRequest::default());
    let from: Positions = [(key_of("swift-amber"), 5)].into();
    assert_eq!(
        after(&mut service, &watch, &from),
        [
            Found::Expired {
                stream: key_of("swift-amber")
            },
            event("swift-amber", 0, "Alpha", "To Do", &[]),
        ]
    );
}

#[test]
fn test_a_stream_that_is_gone_is_expired_but_one_at_its_start_is_not_missed() {
    let mut service = permitting(&["swift-amber", "quiet-heron"]);
    add(&mut service, "Pruned");
    add(&mut service, "Kept");
    let watch = start(&mut service, WatchRequest::default());
    let head = heads(&mut service, &watch);
    remove_for(&mut service, "swift-amber", RemovalReason::Pruned);
    crate::fakes::purge(service.stores.store.inner(), "swift-amber");
    assert_eq!(
        after(&mut service, &watch, &head),
        [Found::Expired {
            stream: key_of("swift-amber")
        }]
    );
    // A position of 0 holds nothing to lose: a stream absent from the
    // store at 0 is not expired.
    let nothing: Positions = [(key_of("far-heron"), 0)].into();
    assert_eq!(after(&mut service, &watch, &nothing).len(), 1, "Kept's add");
}

#[test]
fn test_a_removal_is_an_event_that_says_the_task_is_removed() {
    let mut service = permitting(&["swift-amber"]);
    add(&mut service, "Alpha");
    let watch = start(&mut service, WatchRequest::default());
    let head = heads(&mut service, &watch);
    remove(&mut service, "swift-amber");
    let found = after(&mut service, &watch, &head);
    let [Found::Event { sequence, task, .. }] = found.as_slice() else {
        panic!("one event: {found:?}");
    };
    assert_eq!(*sequence, 1);
    assert!(task.removed, "{task:?}");
}

#[test]
fn test_a_scan_reads_in_one_moment_and_the_streams_in_the_stores_order() {
    let mut service = permitting(&["swift-amber", "quiet-heron", "far-heron"]);
    for text in ["A", "B", "C"] {
        add(&mut service, text);
    }
    let watch = start(&mut service, WatchRequest::default());
    let reads = service.stores.store.units();
    let found = after(&mut service, &watch, &Positions::new());
    assert_eq!(service.stores.store.units(), reads + 1, "one unit");
    let streams: Vec<&StreamKey> = found
        .iter()
        .map(|f| match f {
            Found::Event { stream, .. } | Found::Expired { stream } => stream,
        })
        .collect();
    let mut sorted = streams.clone();
    sorted.sort();
    assert_eq!(streams, sorted);
    assert_eq!(streams.len(), 3);
}

#[test]
fn test_a_watch_lacks_a_store_only_once_a_poll_found_none() {
    let mut service = permitting(&["swift-amber"]);
    add(&mut service, "One");
    assert!(!start(&mut service, WatchRequest::default()).lacks_store());

    let mut service = permitting(&[]);
    service.stores.absent = true;
    let mut watch = start(&mut service, WatchRequest::default());
    assert!(watch.lacks_store());
    // Until the store appears.
    service.stores.absent = false;
    changed(&mut service, &mut watch);
    assert!(!watch.lacks_store());
    // And a watch that has not looked yet lacks nothing.
    assert!(!Watch::of(project()).lacks_store());
    assert!(Watch::without_store(project()).lacks_store());
}

#[test]
fn test_a_scan_that_cannot_open_the_store_fails() {
    let mut service = permitting(&["swift-amber"]);
    add(&mut service, "One");
    let watch = start(&mut service, WatchRequest::default());
    service.stores.open_fails = true;
    let failed = ready(service.after(&watch, &Positions::new())).unwrap_err();
    assert!(matches!(failed, AppError::Failed(_)), "{failed:?}");
    let failed = ready(service.heads(&watch)).unwrap_err();
    assert!(matches!(failed, AppError::Failed(_)), "{failed:?}");
}
