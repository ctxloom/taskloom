//! Watching: a poll answers whether the store's change token moved since
//! the last, and reads no stream; a watch binds no project and waits for a
//! store to appear.

use angzarr_embedded::next::angzarr_ports::memory::MemoryStore;
use angzarr_embedded_diagnostic::Diagnostic;
use tasks::{AddTask, RemovalReason, RemoveTask, TaskCommand};

use super::{Watch, WatchRequest, WatchService};
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
    service.stores.store.open_fails = true;
    let failed = ready(service.poll(&mut watch)).unwrap_err();
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
fn test_a_watch_reads_no_stream() {
    let mut service = service(&["swift-amber", "quiet-heron"]);
    add(&mut service, "One");
    let reads = service.stores.store.reads();
    let mut watch = start(&mut service, WatchRequest::default());
    assert!(!changed(&mut service, &mut watch));
    add_elsewhere(&service.stores.store.inner, "far-heron");
    assert!(changed(&mut service, &mut watch));
    assert_eq!(
        service.stores.store.reads(),
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
    add_elsewhere(&service.stores.store.inner, "far-heron");
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
    crate::fakes::purge(&service.stores.store.inner, "no-such-task");
    assert!(changed(&mut service, &mut watch));
}

#[test]
fn test_a_prune_landing_between_polls_is_a_change() {
    let mut service = service(&["swift-amber", "quiet-heron"]);
    add(&mut service, "Pruned");
    add(&mut service, "Kept");
    let mut watch = start(&mut service, WatchRequest::default());
    remove_for(&mut service, "swift-amber", RemovalReason::Pruned);
    crate::fakes::purge(&service.stores.store.inner, "swift-amber");
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
