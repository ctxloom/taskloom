//! The use cases, over fake ports and angzarr-ports' in-memory store, run through
//! the kernel as the binary runs them over SQLite.

use crate::english::English;
use angzarr_embedded::next::v1::event_meta::Causation;
use angzarr_embedded_diagnostic::{Diagnostic, Report};

use crate::fakes::{
    project, ready, FakeClock, FakeDelay, FakeIdentity, FakeNames, FakeStores, Fault, Flaky,
};
use crate::{
    cover, AppError, ListQuery, NewTask, Service, StatusChange, TagChange, TaskService, TextEdit,
    Written, HARP_ATTEMPTS,
};

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

fn new_task(text: &str) -> NewTask {
    NewTask {
        text: text.to_string(),
        ..NewTask::default()
    }
}

fn add(service: &mut Fake, task: NewTask) -> Result<Written, AppError> {
    ready(service.add(task))
}

fn listed(service: &mut Fake, query: ListQuery) -> Vec<String> {
    ready(service.list(query))
        .unwrap()
        .tasks()
        .map(|t| t.harp_id.clone())
        .collect()
}

/// The command ids recorded on `harp`'s events.
fn command_ids(service: &mut Fake, harp: &str) -> Vec<angzarr_embedded::next::v1::Uuid> {
    let slice = ready(service.stores.store.read_from(&cover(harp), 0))
        .unwrap()
        .expect("the stream");
    slice
        .pages
        .iter()
        .filter_map(|p| match p.meta.as_ref()?.causation.as_ref()? {
            Causation::CommandId(id) => Some(id.clone()),
        })
        .collect()
}

// ------------------------------------------------------------ Send

/// Compiles only for a `Send` value.
fn is_send<T: Send>(_: &T) {}

/// What a server generic over the ports and the service relies on: it
/// compiles only if the traits themselves promise `Send` futures, whatever
/// implements them.
fn spawnable<S, T, D>(service: &mut S, stores: &mut T, delay: &mut D)
where
    S: TaskService + Send,
    T: ports::TaskStores + Send,
    D: ports::Delay + Send,
{
    is_send(&service.add(NewTask::default()));
    is_send(&service.tag(TagChange::default()));
    is_send(&service.list(ListQuery::default()));
    is_send(&service.set_status(StatusChange::default()));
    is_send(&service.edit(TextEdit::default()));
    is_send(&service.show(crate::ShowQuery::default()));
    is_send(&service.summary(crate::Scope::default()));
    is_send(&service.statuses(crate::Scope::default()));
    is_send(&service.tags(ListQuery::default()));
    is_send(&service.lint(crate::Scope::default()));
    is_send(&stores.open(&project()));
    is_send(&stores.open_existing(&project()));
    is_send(&delay.sleep(std::time::Duration::ZERO));
}

#[test]
fn test_every_use_case_and_port_future_is_send_so_a_server_can_spawn_it() {
    let mut service = service(&["swift-amber"]);
    is_send(&service.add(new_task("Ship it")));
    is_send(&service.tag(change("swift-amber", &["x"], &[])));
    is_send(&service.list(ListQuery::default()));
    let (mut stores, mut delay) = (FakeStores::default(), FakeDelay::default());
    spawnable(&mut service, &mut stores, &mut delay);
}

// ------------------------------------------------------------ add

#[test]
fn test_an_add_mints_a_harp_and_answers_with_the_task_and_its_project() {
    let mut service = service(&["swift-amber"]);
    let task = NewTask {
        project: Some("pinned-id".to_string()),
        text: "Ship it".to_string(),
        status: Some("In Progress".to_string()),
        trigger: None,
        tags: vec!["b".to_string(), "a".to_string()],
        origin_session: Some("calm-otter-fox".to_string()),
        request_id: None,
    };
    let written = add(&mut service, task).unwrap();
    assert_eq!(written.project, project());
    assert_eq!(written.task.harp_id, "swift-amber");
    assert_eq!(written.task.text, "Ship it");
    assert_eq!(written.task.status, "In Progress");
    assert_eq!(spelled(&written.task.tags), ["a", "b"]);
    assert_eq!(written.task.origin_session, "calm-otter-fox");
    assert_eq!(
        written.task.created_at.seconds, 1_700_000_001,
        "recorded at the clock's instant"
    );
    assert_eq!(service.identity.pins, [Some("pinned-id".to_string())]);
    assert_eq!(service.stores.opened, ["jolly-mute-magma"]);
}

#[test]
fn test_an_add_whose_harp_is_taken_mints_another_with_a_fresh_command_id() {
    let mut service = service(&["swift-amber", "swift-amber", "calm-otter"]);
    add(&mut service, new_task("First")).unwrap();
    let second = add(&mut service, new_task("Second")).unwrap();
    assert_eq!(second.task.harp_id, "calm-otter");
    assert_eq!(second.task.text, "Second");
    assert_eq!(service.names.minted, 3);
    assert_eq!(service.clock.ids.len(), 3, "one command id per harp tried");
    let landed = command_ids(&mut service, "calm-otter");
    assert_eq!(landed, [service.clock.ids[2].clone()]);
    assert_eq!(command_ids(&mut service, "swift-amber").len(), 1);
}

#[test]
fn test_an_add_gives_up_once_every_harp_it_mints_is_taken() {
    let mut service = service(&["swift-amber"]);
    service.names.then = Some("swift-amber".to_string());
    add(&mut service, new_task("First")).unwrap();
    service.names.minted = 0;
    let error = add(&mut service, new_task("Second")).unwrap_err();
    assert_eq!(error, AppError::HarpsExhausted { attempts: 100 });
    assert_eq!(service.names.minted, HARP_ATTEMPTS);
}

/// Only a taken harp is tried again: any other refusal of a minted harp is
/// the add's answer, at once.
#[test]
fn test_an_add_refused_for_anything_but_a_taken_harp_is_not_tried_again() {
    let mut service = service(&[" ", "calm-otter"]);
    let error = add(&mut service, new_task("First")).unwrap_err();
    let AppError::Rejected(rejection) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(rejection.code(), "tasks-harp-id-required");
    assert_eq!(service.names.minted, 1, "no second harp minted");
}

// A refused add writes nothing: the domain's rules are held before a
// project is bound, a store opened or a harp minted.
#[test]
fn test_an_add_the_domain_refuses_is_refused_before_anything_is_bound_or_created() {
    let deferred = |trigger: Option<&str>| NewTask {
        status: Some("Deferred".to_string()),
        trigger: trigger.map(str::to_string),
        ..new_task("Revisit")
    };
    let reserved = NewTask {
        status: Some("@open".to_string()),
        ..new_task("Ship it")
    };
    let with_status = NewTask {
        status: Some("In Progress".to_string()),
        trigger: Some("the release".to_string()),
        ..new_task("Revisit")
    };
    for (task, code) in [
        (new_task("   "), "tasks-text-required"),
        (deferred(None), "tasks-trigger-required"),
        (reserved, "tasks-status-reserved"),
        (with_status, "tasks-trigger-with-status"),
    ] {
        let mut service = service(&["swift-amber"]);
        let error = add(&mut service, task).unwrap_err();
        let AppError::Rejected(rejection) = &error else {
            panic!("rejected: {error:?}");
        };
        assert_eq!(rejection.code(), code);
        assert_eq!(service.identity.writes, 0, "no project bound");
        assert_eq!(service.identity.bound, None);
        assert!(service.stores.opened.is_empty(), "no store opened");
        assert_eq!(service.names.minted, 0, "no harp minted");
    }
    let mut service = service(&["swift-amber"]);
    let written = add(&mut service, deferred(Some("the release"))).unwrap();
    assert_eq!(written.task.trigger, "the release");
    assert_eq!(service.identity.writes, 1);
}

// A status, trigger or session given blank is given: refused, never read
// as one left out (To Do, no trigger, no session), before the
// configuration is read or anything bound, created or minted.
#[test]
fn test_an_add_given_a_blank_status_trigger_or_session_is_refused_before_anything_is_read() {
    let mut service = configured(&["unreadable"]);
    for blank in ["", "  "] {
        let given = || Some(blank.to_string());
        for (task, field) in [
            (
                NewTask {
                    status: given(),
                    ..new_task("Ship it")
                },
                "status",
            ),
            (
                NewTask {
                    trigger: given(),
                    ..new_task("Ship it")
                },
                "trigger",
            ),
            (
                NewTask {
                    origin_session: given(),
                    ..new_task("Ship it")
                },
                "origin_session",
            ),
        ] {
            let error = ready(service.add(task)).unwrap_err();
            assert_eq!(error, AppError::Blank { field });
        }
    }
    assert!(service.configuration.asked == 0, "no configuration read");
    assert!(service.identity.pins.is_empty(), "no project resolved");
    assert_eq!(service.names.minted, 0, "no harp minted");
}

// A trigger alone defers the task: added Deferred, returning to To Do, the
// initial status, when it is cleared.
#[test]
fn test_an_add_with_a_trigger_alone_is_added_deferred() {
    let mut service = service(&["swift-amber"]);
    let task = NewTask {
        trigger: Some("the release".to_string()),
        ..new_task("Revisit")
    };
    let written = add(&mut service, task).unwrap();
    assert_eq!(
        (written.task.status.as_str(), written.task.trigger.as_str()),
        ("Deferred", "the release")
    );
    let cleared = ready(service.set_status(cleared("swift-amber", None))).unwrap();
    assert_eq!(cleared.task.status, "To Do");
}

// What is left out is the default: To Do, no trigger, no session.
#[test]
fn test_an_add_leaving_out_status_trigger_and_session_is_to_do_with_none() {
    let mut service = service(&["swift-amber"]);
    let written = add(&mut service, new_task("Ship it")).unwrap();
    assert_eq!(written.task.status, "To Do");
    assert_eq!(written.task.trigger, "");
    assert_eq!(written.task.origin_session, "");
}

#[test]
fn test_an_add_retries_its_one_command_id_through_store_failures() {
    let mut service = service(&["swift-amber"]);
    service.stores.store = Flaky::failing(&[Fault::Unavailable, Fault::LandsThenFails]);
    let written = add(&mut service, new_task("Ship it")).unwrap();
    assert_eq!(written.task.harp_id, "swift-amber");
    assert_eq!(
        service.stores.store.begun(),
        2,
        "two writes: the retry after the lost answer found it on the stream"
    );
    assert_eq!(service.delay.slept.len(), 2);
    assert_eq!(service.clock.ids.len(), 1, "the same command id each time");
    assert_eq!(command_ids(&mut service, "swift-amber").len(), 1);
}

#[test]
fn test_an_add_that_cannot_resolve_its_project_fails_with_why() {
    let mut service = service(&["swift-amber"]);
    service.identity.fails = Some(Report::new("project-registry-x"));
    let error = add(&mut service, new_task("Ship it")).unwrap_err();
    assert_eq!(error, AppError::Identity(Report::new("project-registry-x")));
    assert!(service.stores.opened.is_empty());
}

#[test]
fn test_an_add_whose_store_does_not_open_fails() {
    let mut service = service(&["swift-amber"]);
    service.stores.store.open_fails = true;
    let error = add(&mut service, new_task("Ship it")).unwrap_err();
    assert_eq!(error.code(), "angzarr-ports-store-unavailable");
}

// ------------------------------------------------------------ tag

fn change(harp: &str, add: &[&str], remove: &[&str]) -> TagChange {
    TagChange {
        project: Some("pinned-id".to_string()),
        harp_id: harp.to_string(),
        add: add.iter().map(|s| s.to_string()).collect(),
        remove: remove.iter().map(|s| s.to_string()).collect(),
        request_id: None,
    }
}

// A tag outside tagma's grammar could never be queried: it is refused
// before anything is resolved or written.
#[test]
fn test_an_add_with_a_tag_outside_the_grammar_is_refused_and_writes_nothing() {
    let mut service = service(&["swift-amber"]);
    let task = NewTask {
        tags: vec!["ok".to_string(), "not ok".to_string()],
        ..new_task("Ship it")
    };
    let error = add(&mut service, task).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-unparseable");
    assert!(error.to_string().contains("not ok"), "{error}");
    assert!(service.identity.pins.is_empty(), "no project resolved");
    assert!(service.stores.opened.is_empty());
}

// A blank tag given is refused, never left out: the add it came with would
// otherwise write a task without the tag it meant.
#[test]
fn test_an_add_with_a_blank_tag_is_refused_and_writes_nothing() {
    for blank in ["", " "] {
        let mut service = service(&["swift-amber"]);
        let task = NewTask {
            tags: vec!["ok".to_string(), blank.to_string()],
            ..new_task("Ship it")
        };
        let error = add(&mut service, task).unwrap_err();
        assert_eq!(error.code(), "tasks-application-tag-blank");
        assert!(service.identity.pins.is_empty(), "no project resolved");
        assert!(service.stores.opened.is_empty());
    }
}

#[test]
fn test_a_tag_naming_a_tag_outside_the_grammar_is_refused_either_way() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    for change in [
        change("swift-amber", &["a/b"], &[]),
        change("swift-amber", &[], &["a/b"]),
    ] {
        let error = ready(service.tag(change)).unwrap_err();
        assert_eq!(error.code(), "tasks-application-tag-unparseable");
    }
    assert_eq!(command_ids(&mut service, "swift-amber").len(), 1, "the add");
}

// A tag's spelling is not its identity: `k="v"` is `k=v`.
#[test]
fn test_a_tag_spelled_another_way_is_the_same_tag() {
    let mut service = service(&["swift-amber"]);
    let task = NewTask {
        tags: vec!["k=v".to_string()],
        ..new_task("Ship it")
    };
    add(&mut service, task).unwrap();
    let written = ready(service.tag(change("swift-amber", &[], &["k=\"v\""]))).unwrap();
    assert!(written.task.tags.is_empty());
}

// ------------------------------------------------------------ schema

/// What the configuration answers, and how often it was asked.
#[derive(Default)]
struct FakeConfiguration {
    declarations: Vec<String>,
    asked: usize,
}

impl ports::Configuration for FakeConfiguration {
    fn tag_schema(&mut self) -> Result<Vec<String>, Report> {
        self.asked += 1;
        if self.declarations.iter().any(|d| d == "unreadable") {
            return Err(Report::new("x-config"));
        }
        Ok(self.declarations.clone())
    }
}

type Configured =
    Service<FakeIdentity, FakeStores, FakeNames, FakeClock, FakeDelay, FakeConfiguration>;

fn configured(declarations: &[&str]) -> Configured {
    service(&["swift-amber"]).with_configuration(FakeConfiguration {
        declarations: declarations.iter().map(|d| d.to_string()).collect(),
        asked: 0,
    })
}

fn tagged(add: &[&str]) -> NewTask {
    NewTask {
        tags: add.iter().map(|s| s.to_string()).collect(),
        ..new_task("Ship it")
    }
}

// The default schema holds with nothing configured: triage:level is 1-5.
#[test]
fn test_a_write_is_held_to_the_default_schema() {
    let mut service = service(&["swift-amber"]);
    let error = add(&mut service, tagged(&["triage:level=9"])).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-out-of-range");
    assert!(service.identity.pins.is_empty(), "refused before resolving");
    for refused in [
        &["and"][..],
        &["tagma.hide:x=true"],
        &["triage:kind=sparkles"],
    ] {
        assert!(add(&mut service, tagged(refused)).is_err(), "{refused:?}");
    }
}

#[test]
fn test_a_configured_declaration_extends_the_default_schema() {
    let mut service = configured(&[
        r#"tagma.range:"triage:level"="1,9""#,
        r#"tagma.enum:"area:x"="a""#,
    ]);
    let written = ready(service.add(tagged(&["triage:level=9"]))).unwrap();
    assert_eq!(spelled(&written.task.tags), ["triage:level=9"]);
    assert_eq!(service.configuration.asked, 1);
    let error = ready(service.add(tagged(&["area:x=b"]))).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-not-in-enum");
    let error = ready(service.add(tagged(&["triage:kind=sparkles"]))).unwrap_err();
    assert_eq!(
        error.code(),
        "tasks-application-tag-not-in-enum",
        "the default still holds"
    );
}

#[test]
fn test_a_malformed_or_unreadable_configuration_refuses_every_write() {
    let mut service = configured(&["tagma.arty:x=1"]);
    let error = ready(service.add(new_task("Ship it"))).unwrap_err();
    assert_eq!(error.code(), "tasks-application-schema-unknown-facet");
    assert!(matches!(error, AppError::Configuration(_)));
    let error = ready(service.tag(change("swift-amber", &["x"], &[]))).unwrap_err();
    assert_eq!(error.code(), "tasks-application-schema-unknown-facet");
    let mut service = configured(&["unreadable"]);
    let error = ready(service.add(new_task("Ship it"))).unwrap_err();
    assert_eq!(error.code(), "x-config");
    assert!(service.stores.opened.is_empty());
}

// The tag schema a write is held to, as a reader asks for it: the default
// extended by what is configured; and refused as a write is, where the
// configuration is.
#[test]
fn test_the_tag_schema_is_the_one_a_write_is_held_to() {
    let mut service = configured(&[r#"tagma.range:"triage:level"="1,9""#]);
    let schema = ready(service.tag_schema()).unwrap();
    assert_eq!(
        schema.range_of("triage:level"),
        Some(crate::Range { min: 1.0, max: 9.0 })
    );
    assert!(schema.is_scalar("triage:kind"), "the default still holds");
    assert_eq!(service.configuration.asked, 1);
    assert!(service.identity.pins.is_empty(), "no project is resolved");
    let mut service = configured(&["tagma.arty:x=1"]);
    let error = ready(service.tag_schema()).unwrap_err();
    assert_eq!(error.code(), "tasks-application-schema-unknown-facet");
}

// Scalar collapse, resolved in the retag's own unit of work, over the default
// schema's scalars.
#[test]
fn test_a_new_scalar_value_displaces_the_one_the_task_holds() {
    let mut service = service(&["swift-amber"]);
    add(
        &mut service,
        tagged(&["triage:level=3", "triage:kind=chore", "x"]),
    )
    .unwrap();
    let written = ready(service.tag(change("swift-amber", &["triage:level=1"], &[]))).unwrap();
    assert_eq!(
        spelled(&written.task.tags),
        ["triage:kind=chore", "triage:level=1", "x"]
    );
    let written = ready(service.tag(change("swift-amber", &["triage:level=\"1\""], &[]))).unwrap();
    assert_eq!(
        written.task.tags.len(),
        3,
        "the same value, spelled otherwise"
    );
    assert_eq!(
        command_ids(&mut service, "swift-amber").len(),
        2,
        "nothing recorded"
    );
}

// The displacement is resolved against the task as the retag's own unit
// of work reads it: a value another process set since the task was last
// read is displaced too, so a scalar target never holds two.
#[test]
fn test_a_scalar_value_set_by_another_since_is_displaced_too() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, tagged(&["triage:level=3", "x"])).unwrap();
    let parse = |tag: &str| crate::parse_tag(tag).unwrap().unwrap();
    let retag: tasks::TaskCommand = tasks::Retag {
        add: vec![parse("triage:level=2")],
        remove: vec![parse("triage:level=3")],
    }
    .into();
    service.stores.store.set_meanwhile(Box::new(move |store| {
        crate::fakes::run(store, "swift-amber", retag);
    }));
    let written = ready(service.tag(change("swift-amber", &["triage:level=1"], &[]))).unwrap();
    assert!(service.stores.store.meanwhile_ran(), "the other retag ran");
    assert_eq!(spelled(&written.task.tags), ["triage:level=1", "x"]);
}

// There is no unit of work: the retag is shaped from one read and sent at
// the head it read, so a value another process sets between that read and
// the kernel's own is the kernel's conflict, and the retry shapes it
// again: the scalar target still holds one value.
#[test]
fn test_a_scalar_value_set_between_the_shaping_read_and_the_write_is_displaced_too() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, tagged(&["triage:level=3", "x"])).unwrap();
    let parse = |tag: &str| crate::parse_tag(tag).unwrap().unwrap();
    let retag: tasks::TaskCommand = tasks::Retag {
        add: vec![parse("triage:level=2")],
        remove: vec![parse("triage:level=3")],
    }
    .into();
    service.stores.store.set_before_read(
        2,
        Box::new(move |store| crate::fakes::run(store, "swift-amber", retag)),
    );
    let written = ready(service.tag(change("swift-amber", &["triage:level=1"], &[]))).unwrap();
    assert!(
        service.stores.store.before_read_ran(),
        "the other retag ran"
    );
    assert_eq!(spelled(&written.task.tags), ["triage:level=1", "x"]);
    assert_eq!(service.delay.slept.len(), 1, "the stale head, retried once");
}

// Two values for one scalar target in one add are refused, as a rule of
// the add, before a project is resolved: neither is the one meant more than
// the other. One value given twice is one value.
#[test]
fn test_an_add_giving_a_scalar_target_two_values_is_refused_and_writes_nothing() {
    let mut service = service(&["swift-amber"]);
    let error = add(
        &mut service,
        tagged(&["triage:level=3", "area:x=1", "area:x=2", "triage:level=2"]),
    )
    .unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-scalar-conflict");
    assert!(error.to_string().contains("triage:level=2"), "{error}");
    assert!(service.identity.pins.is_empty(), "no project resolved");
    assert!(service.stores.opened.is_empty());
    let written = add(
        &mut service,
        tagged(&[
            "triage:level=3",
            "area:x=1",
            "area:x=2",
            "triage:level=\"3\"",
        ]),
    )
    .unwrap();
    assert_eq!(
        spelled(&written.task.tags),
        ["area:x=1", "area:x=2", "triage:level=3"]
    );
}

// The same holds for one retag: two values for one scalar target are
// refused before anything is read, and the task keeps what it holds.
#[test]
fn test_a_tag_giving_a_scalar_target_two_values_is_refused_and_changes_nothing() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, tagged(&["triage:level=3"])).unwrap();
    let pins = service.identity.pins.len();
    let given = change("swift-amber", &["triage:level=1", "triage:level=2"], &[]);
    let error = ready(service.tag(given)).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-scalar-conflict");
    assert_eq!(service.identity.pins.len(), pins, "no project resolved");
    assert_eq!(command_ids(&mut service, "swift-amber").len(), 1);
}

#[test]
fn test_a_removed_tag_is_never_checked_against_the_schema() {
    let mut service = configured(&[]);
    add_unchecked(&mut service, "triage:level=9");
    let written = ready(service.tag(change("swift-amber", &[], &["triage:level=9"]))).unwrap();
    assert!(written.task.tags.is_empty());
}

/// Add a task holding `tag` as one written before the schema forbade it:
/// its add goes past the use case, straight to the domain.
fn add_unchecked(service: &mut Configured, tag: &str) {
    use crate::tags::parse;
    let add = tasks::AddTask {
        harp_id: "swift-amber".to_string(),
        text: "Ship it".to_string(),
        status: None,
        trigger: None,
        tags: vec![parse(tag).unwrap().unwrap()],
        origin_session: None,
        defers: tasks::DEFERRED.to_string(),
        initial: tasks::TO_DO.to_string(),
    };
    let mut runner = crate::retry::Runner {
        registry: &service.registry,
        clock: &mut service.clock,
        delay: &mut service.delay,
        backoff: service.backoff,
        dead_letters: &service.dead_letters,
    };
    let book = crate::book("swift-amber", add.into());
    let id = angzarr_embedded::next::v1::Uuid::from_bytes([7; 16]);
    ready(runner.run(&service.stores.store, &book, &id)).unwrap();
}

#[test]
fn test_a_tag_adds_then_removes_and_answers_with_the_task() {
    let mut service = service(&["swift-amber"]);
    let task = NewTask {
        tags: vec!["old".to_string()],
        ..new_task("Ship it")
    };
    add(&mut service, task).unwrap();
    let written =
        ready(service.tag(change("swift-amber", &["new", "both"], &["old", "both"]))).unwrap();
    assert_eq!(spelled(&written.task.tags), ["new"]);
    assert_eq!(written.project, project());
    assert_eq!(
        service.identity.pins.last(),
        Some(&Some("pinned-id".to_string()))
    );
}

#[test]
fn test_a_tag_changing_nothing_answers_with_the_task_as_it_is() {
    let mut service = service(&["swift-amber"]);
    let task = NewTask {
        tags: vec!["kept".to_string()],
        ..new_task("Ship it")
    };
    add(&mut service, task).unwrap();
    let written = ready(service.tag(change("swift-amber", &["kept"], &[]))).unwrap();
    assert_eq!(spelled(&written.task.tags), ["kept"]);
}

#[test]
fn test_a_tag_with_nothing_to_add_or_remove_is_refused_before_anything_is_read() {
    let mut service = configured(&["unreadable"]);
    let error = ready(service.tag(change("swift-amber", &[], &[]))).unwrap_err();
    assert_eq!(error.code(), "tasks-tag-required");
    for (add, remove) in [(&[" "][..], &[][..]), (&[][..], &[""][..])] {
        let error = ready(service.tag(change("swift-amber", add, remove))).unwrap_err();
        assert_eq!(error.code(), "tasks-application-tag-blank");
    }
    assert!(service.configuration.asked == 0, "no configuration read");
    assert!(service.identity.pins.is_empty(), "no project resolved");
    let error = ready(service.tag(change("swift-amber", &[], &[" x "]))).unwrap_err();
    assert_eq!(error.code(), "x-config", "one tag is something to do");
}

#[test]
fn test_a_tag_of_no_task_is_refused() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let error = ready(service.tag(change("calm-otter", &["x"], &[]))).unwrap_err();
    assert_eq!(error.code(), "tasks-no-task");
}

#[test]
fn test_a_tag_where_there_is_no_project_fails_and_binds_none() {
    let mut service = service(&[]);
    let unpinned = TagChange {
        project: None,
        ..change("swift-amber", &["x"], &[])
    };
    let error = ready(service.tag(unpinned)).unwrap_err();
    assert_eq!(error, AppError::NoProject);
    assert_eq!(
        service.identity.bound, None,
        "resolved for a read: none minted"
    );
    assert!(service.stores.opened.is_empty() && service.stores.looked.is_empty());
}

#[test]
fn test_a_tag_in_a_project_with_no_store_yet_is_refused_and_creates_none() {
    let mut service = service(&[]);
    service.stores.absent = true;
    let error = ready(service.tag(change("swift-amber", &["x"], &[]))).unwrap_err();
    assert_eq!(error.code(), "tasks-no-task");
    assert_eq!(service.stores.looked, ["jolly-mute-magma"]);
    assert!(service.stores.opened.is_empty(), "no store opened to write");
}

#[test]
fn test_a_tag_retries_through_store_failures() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    service
        .stores
        .store
        .fail([Fault::Conflict, Fault::LandsThenFails]);
    let written = ready(service.tag(change("swift-amber", &["x"], &[]))).unwrap();
    assert_eq!(spelled(&written.task.tags), ["x"]);
    assert_eq!(service.delay.slept.len(), 2);
    assert_eq!(
        command_ids(&mut service, "swift-amber").len(),
        2,
        "add, retag"
    );
}

#[test]
fn test_a_tag_that_gives_up_says_so() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    service.backoff.attempts = 2;
    service
        .stores
        .store
        .fail([Fault::Unavailable, Fault::Unavailable]);
    let error = ready(service.tag(change("swift-amber", &["x"], &[]))).unwrap_err();
    assert!(
        matches!(error, AppError::GaveUp { attempts: 2, .. }),
        "{error:?}"
    );
}

#[test]
fn test_a_tag_whose_project_does_not_resolve_fails_with_why() {
    let mut service = service(&[]);
    service.identity.fails = Some(Report::new("project-registry-x"));
    let error = ready(service.tag(change("swift-amber", &["x"], &[]))).unwrap_err();
    assert_eq!(error.code(), "project-registry-x");
}

#[test]
fn test_a_tag_whose_store_does_not_open_fails() {
    let mut service = service(&[]);
    service.identity.bound = Some(project());
    service.stores.store.open_fails = true;
    let error = ready(service.tag(change("swift-amber", &["x"], &[]))).unwrap_err();
    assert_eq!(error.code(), "angzarr-ports-store-unavailable");
}

// ------------------------------------------------------------ status

fn moved(harp: &str, status: &str, trigger: Option<&str>) -> StatusChange {
    StatusChange {
        project: Some("pinned-id".to_string()),
        harp_id: harp.to_string(),
        status: Some(status.to_string()),
        trigger: trigger.map(str::to_string),
        clear_trigger: false,
        request_id: None,
    }
}

/// A status change giving only a trigger.
fn triggered(harp: &str, trigger: &str) -> StatusChange {
    StatusChange {
        status: None,
        ..moved(harp, "", Some(trigger))
    }
}

/// A status change clearing the trigger, moving to `status` if given.
fn cleared(harp: &str, status: Option<&str>) -> StatusChange {
    StatusChange {
        status: status.map(str::to_string),
        clear_trigger: true,
        ..moved(harp, "", None)
    }
}

#[test]
fn test_a_status_change_moves_the_task_and_answers_with_it_and_its_project() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let written = ready(service.set_status(moved("swift-amber", "Done", None))).unwrap();
    assert_eq!(written.task.status, "Done");
    assert!(written.task.checked);
    assert_eq!(written.project, project());
    assert_eq!(
        service.identity.pins.last(),
        Some(&Some("pinned-id".to_string()))
    );
    assert_eq!(command_ids(&mut service, "swift-amber").len(), 2);
}

#[test]
fn test_deferred_needs_a_trigger_given_or_held() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let error = ready(service.set_status(moved("swift-amber", "Deferred", None))).unwrap_err();
    assert_eq!(error.code(), "tasks-trigger-required");
    let written =
        ready(service.set_status(moved("swift-amber", "Deferred", Some("the release")))).unwrap();
    assert_eq!(written.task.trigger, "the release");
    let again = ready(service.set_status(moved("swift-amber", "Deferred", None))).unwrap();
    assert_eq!(again.task.trigger, "the release", "held while Deferred");
    // Reverses the reviewed rule that leaving Deferred kept the trigger
    // (the human's ruling on L, 2026-10-27): it clears it.
    let left = ready(service.set_status(moved("swift-amber", "To Do", None))).unwrap();
    assert_eq!(left.task.trigger, "", "cleared on leaving Deferred");
    let error = ready(service.set_status(moved("swift-amber", "Deferred", None))).unwrap_err();
    assert_eq!(error.code(), "tasks-trigger-required", "none held now");
}

// A trigger alone defers the task, keeping its status to return to; clearing
// the trigger returns it there.
#[test]
fn test_a_trigger_alone_defers_the_task_and_clearing_it_returns_it() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    ready(service.set_status(moved("swift-amber", "In Progress", None))).unwrap();
    let deferred = ready(service.set_status(triggered("swift-amber", "the release"))).unwrap();
    assert_eq!(
        (
            deferred.task.status.as_str(),
            deferred.task.trigger.as_str()
        ),
        ("Deferred", "the release")
    );
    let back = ready(service.set_status(cleared("swift-amber", None))).unwrap();
    assert_eq!(
        (back.task.status.as_str(), back.task.trigger.as_str()),
        ("In Progress", "")
    );
    ready(service.set_status(triggered("swift-amber", "the release"))).unwrap();
    let done = ready(service.set_status(cleared("swift-amber", Some("Done")))).unwrap();
    assert_eq!(
        (done.task.status.as_str(), done.task.trigger.as_str()),
        ("Done", "")
    );
}

#[test]
fn test_a_trigger_with_another_status_is_refused() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let change = moved("swift-amber", "In Progress", Some("the release"));
    let error = ready(service.set_status(change)).unwrap_err();
    assert_eq!(error.code(), "tasks-trigger-with-status");
}

// A trigger given and cleared at once is refused, before the configuration
// is read or a project resolved.
#[test]
fn test_a_trigger_given_and_cleared_at_once_is_refused_before_anything_is_read() {
    let mut service = configured(&["unreadable"]);
    for status in [None, Some("Deferred")] {
        let change = StatusChange {
            trigger: Some("the release".to_string()),
            ..cleared("swift-amber", status)
        };
        let error = ready(service.set_status(change)).unwrap_err();
        assert_eq!(error, AppError::TriggerAndClear);
    }
    assert!(service.configuration.asked == 0, "no configuration read");
    assert!(service.identity.pins.is_empty(), "no project resolved");
}

// A status given blank is given: refused, never read as none.
#[test]
fn test_a_status_change_given_a_blank_status_is_refused_before_anything_is_read() {
    let mut service = configured(&["unreadable"]);
    for blank in ["", " "] {
        let error = ready(service.set_status(moved("swift-amber", blank, None))).unwrap_err();
        assert_eq!(error, AppError::Blank { field: "status" });
    }
    assert!(service.configuration.asked == 0, "no configuration read");
    assert!(service.identity.pins.is_empty(), "no project resolved");
}

#[test]
fn test_a_status_change_giving_nothing_is_refused() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let change = StatusChange {
        status: None,
        ..moved("swift-amber", "", None)
    };
    let error = ready(service.set_status(change)).unwrap_err();
    assert_eq!(error.code(), "tasks-status-required");
}

// A trigger given blank is given: refused, never read as none (which keeps
// the one held), before the configuration is read or a project resolved.
#[test]
fn test_a_status_change_given_a_blank_trigger_is_refused_before_anything_is_read() {
    let mut service = configured(&["unreadable"]);
    for blank in ["", " "] {
        let change = moved("swift-amber", "Deferred", Some(blank));
        let error = ready(service.set_status(change)).unwrap_err();
        assert_eq!(error, AppError::Blank { field: "trigger" });
    }
    assert!(service.configuration.asked == 0, "no configuration read");
    assert!(service.identity.pins.is_empty(), "no project resolved");
}

#[test]
fn test_a_status_class_is_no_status_to_move_to() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let error = ready(service.set_status(moved("swift-amber", "@open", None))).unwrap_err();
    assert_eq!(error.code(), "tasks-status-reserved");
}

#[test]
fn test_a_status_change_of_no_task_or_where_there_is_none_is_refused() {
    let mut held = service(&["swift-amber"]);
    add(&mut held, new_task("Ship it")).unwrap();
    let error = ready(held.set_status(moved("calm-otter", "Done", None))).unwrap_err();
    assert_eq!(error.code(), "tasks-no-task");
    let mut nowhere = service(&[]);
    let unpinned = StatusChange {
        project: None,
        ..moved("swift-amber", "Done", None)
    };
    let error = ready(nowhere.set_status(unpinned)).unwrap_err();
    assert_eq!(error, AppError::NoProject);
    assert!(nowhere.stores.opened.is_empty() && nowhere.stores.looked.is_empty());
    let mut storeless = service(&[]);
    storeless.stores.absent = true;
    let error = ready(storeless.set_status(moved("swift-amber", "Done", None))).unwrap_err();
    assert_eq!(error.code(), "tasks-no-task");
    assert!(storeless.stores.opened.is_empty(), "no store created");
}

// What the domain refuses of a change alone is refused before its project
// is resolved or its store opened.
#[test]
fn test_a_status_change_or_edit_the_domain_refuses_alone_resolves_nothing() {
    let mut service = configured(&["unreadable"]);
    // Neither a status nor a trigger (a status given blank is refused as
    // blank, before the domain sees it), and a reserved status.
    for status in [None, Some("@open")] {
        let change = StatusChange {
            status: status.map(str::to_string),
            ..moved("swift-amber", "", None)
        };
        let error = ready(service.set_status(change)).unwrap_err();
        assert!(matches!(error, AppError::Rejected(_)), "{error:?}");
    }
    let error = ready(service.edit(edited("swift-amber", " "))).unwrap_err();
    assert_eq!(error.code(), "tasks-text-required");
    assert!(service.configuration.asked == 0, "no configuration read");
    assert!(service.identity.pins.is_empty(), "no project resolved");
    assert!(service.stores.opened.is_empty() && service.stores.looked.is_empty());
}

#[test]
fn test_a_status_change_retries_through_store_failures() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    service.stores.store.fail([Fault::Conflict]);
    let written = ready(service.set_status(moved("swift-amber", "Done", None))).unwrap();
    assert_eq!(written.task.status, "Done");
    assert_eq!(service.delay.slept.len(), 1);
}

#[test]
fn test_a_status_change_under_a_malformed_configuration_is_refused() {
    let mut service = configured(&["tagma.arty:x=1"]);
    let error = ready(service.set_status(moved("swift-amber", "Done", None))).unwrap_err();
    assert_eq!(error.code(), "tasks-application-schema-unknown-facet");
    assert!(service.identity.pins.is_empty());
}

// ------------------------------------------------------------ edit

fn edited(harp: &str, text: &str) -> TextEdit {
    TextEdit {
        project: Some("pinned-id".to_string()),
        harp_id: harp.to_string(),
        text: text.to_string(),
        request_id: None,
    }
}

#[test]
fn test_an_edit_replaces_the_text_whole_and_keeps_the_status_and_trigger() {
    let mut service = service(&["swift-amber"]);
    let deferred = NewTask {
        status: Some("Deferred".to_string()),
        trigger: Some("the release".to_string()),
        ..new_task("Ship it")
    };
    let before = add(&mut service, deferred).unwrap();
    let written = ready(service.edit(edited("swift-amber", "  Ship it, then tell\n"))).unwrap();
    assert_eq!(written.task.text, "Ship it, then tell", "trimmed");
    assert_ne!(written.task.text_hash, before.task.text_hash);
    assert_eq!(written.task.status, "Deferred");
    assert_eq!(written.task.trigger, "the release");
    assert_eq!(written.project, project());
    assert_eq!(
        service.identity.pins.last(),
        Some(&Some("pinned-id".to_string()))
    );
}

#[test]
fn test_an_edit_to_no_text_or_no_task_is_refused() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let error = ready(service.edit(edited("swift-amber", " "))).unwrap_err();
    assert_eq!(error.code(), "tasks-text-required");
    let error = ready(service.edit(edited("calm-otter", "x"))).unwrap_err();
    assert_eq!(error.code(), "tasks-no-task");
    let mut nowhere = self::service(&[]);
    let unpinned = TextEdit {
        project: None,
        ..edited("swift-amber", "x")
    };
    assert_eq!(
        ready(nowhere.edit(unpinned)).unwrap_err(),
        AppError::NoProject
    );
    let mut configured = configured(&["tagma.arty:x=1"]);
    let error = ready(configured.edit(edited("swift-amber", "x"))).unwrap_err();
    assert_eq!(error.code(), "tasks-application-schema-unknown-facet");
}

// ------------------------------------------------------------ list

/// A service holding tasks in each default status, added in this order.
fn with_every_status() -> Fake {
    let mut service = service(&["to-do", "doing", "deferred", "done", "archived", "other"]);
    for (status, trigger) in [
        ("To Do", None),
        ("In Progress", None),
        ("Deferred", Some("the release")),
        ("Done", None),
        ("Archived", None),
        ("Blocked", None),
    ] {
        let task = NewTask {
            status: Some(status.to_string()),
            trigger: trigger.map(str::to_string),
            ..new_task(status)
        };
        add(&mut service, task).unwrap();
    }
    service
}

#[test]
fn test_a_list_by_default_holds_the_open_tasks_in_the_order_added() {
    let mut service = with_every_status();
    assert_eq!(
        listed(&mut service, ListQuery::default()),
        ["to-do", "doing", "other"]
    );
}

#[test]
fn test_a_list_of_all_holds_every_task() {
    let mut service = with_every_status();
    let all = ListQuery {
        all: true,
        ..ListQuery::default()
    };
    assert_eq!(
        listed(&mut service, all),
        ["to-do", "doing", "deferred", "done", "archived", "other"]
    );
}

#[test]
fn test_a_list_of_named_statuses_holds_exactly_their_tasks() {
    let mut service = with_every_status();
    let named = ListQuery {
        statuses: vec!["Done".to_string(), "Deferred".to_string()],
        ..ListQuery::default()
    };
    assert_eq!(listed(&mut service, named), ["deferred", "done"]);
}

// ------------------------------------------------------------ tag queries

fn queried(query: &str) -> ListQuery {
    ListQuery {
        tag_query: Some(query.to_string()),
        ..ListQuery::default()
    }
}

// The tasks a query selects keep the listing's order.
#[test]
fn test_a_tag_query_keeps_the_tasks_it_selects_in_the_listings_order() {
    let mut service = service(&["zebra-a", "mango-b", "apple-c", "kiwi-d"]);
    for tags in [&["urgent"][..], &["urgent"], &["urgent", "x"], &[]] {
        add(&mut service, tagged(tags)).unwrap();
    }
    assert_eq!(
        listed(&mut service, queried("urgent")),
        ["zebra-a", "mango-b", "apple-c"]
    );
    assert_eq!(
        listed(&mut service, queried("urgent and not x")),
        ["zebra-a", "mango-b"]
    );
    assert_eq!(listed(&mut service, queried("urgent/not")), ["kiwi-d"]);
}

// `not` complements within the tasks the listing admits, never past them.
#[test]
fn test_not_complements_within_the_listed_tasks() {
    let mut service = service(&["keep-me", "done-urgent"]);
    add(&mut service, new_task("Keep")).unwrap();
    let done = NewTask {
        status: Some("Done".to_string()),
        ..tagged(&["urgent"])
    };
    add(&mut service, done).unwrap();
    assert_eq!(listed(&mut service, queried("urgent/not")), ["keep-me"]);
    let all = ListQuery {
        all: true,
        ..queried("urgent")
    };
    assert_eq!(listed(&mut service, all), ["done-urgent"]);
}

#[test]
fn test_a_malformed_tag_query_is_refused_even_with_nothing_to_list() {
    let mut service = service(&[]);
    let error = ready(service.list(queried("urgent/and"))).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-query");
    assert!(matches!(error, AppError::TagQuery(_)));
    assert!(service.identity.pins.is_empty(), "refused before resolving");
}

// The default schema declares triage:blocks-release a SemVer version.
#[test]
fn test_a_tag_query_orders_a_semver_target_by_precedence() {
    let mut service = service(&["below-a", "pre-b", "above-c"]);
    for version in ["0.6.9", "0.7.0-pre001", "0.8.0"] {
        let tag = format!("triage:blocks-release={version}");
        add(&mut service, tagged(&[tag.as_str()])).unwrap();
    }
    assert_eq!(
        listed(&mut service, queried("triage:blocks-release<=0.7.0")),
        ["below-a", "pre-b"]
    );
}

#[test]
fn test_tasks_added_at_one_instant_list_by_harp_id() {
    let mut service = service(&["zebra-b", "alpha-a"]);
    add(&mut service, new_task("First")).unwrap();
    service.clock.seconds = 0;
    add(&mut service, new_task("Second")).unwrap();
    assert_eq!(
        listed(&mut service, ListQuery::default()),
        ["alpha-a", "zebra-b"]
    );
}

#[test]
fn test_a_list_answers_with_each_tasks_values_and_its_project() {
    let mut service = service(&["swift-amber"]);
    let added = add(&mut service, new_task("Ship it")).unwrap();
    let listing = ready(service.list(ListQuery::default())).unwrap();
    assert_eq!(listing.project(), Some(&project()));
    assert_eq!(listing.tasks().collect::<Vec<_>>(), [&added.task]);
}

#[test]
fn test_a_list_where_there_is_no_project_in_a_work_root_is_empty_and_opens_no_store() {
    let mut service = service(&[]);
    let listing = ready(service.list(ListQuery::default())).unwrap();
    assert_eq!(listing.reach, crate::Reach::NoProject);
    assert!(listing.rows.is_empty());
    assert!(service.stores.opened.is_empty() && service.stores.looked.is_empty());
    assert_eq!(
        service.identity.bound, None,
        "resolved for a read: none minted"
    );
}

#[test]
fn test_a_list_of_a_project_with_no_store_yet_is_empty_and_creates_none() {
    let mut service = service(&[]);
    service.stores.absent = true;
    let query = ListQuery {
        project: Some("pinned-id".to_string()),
        ..ListQuery::default()
    };
    let listing = ready(service.list(query)).unwrap();
    assert_eq!(listing.project(), Some(&project()));
    assert!(listing.rows.is_empty());
    assert_eq!(service.stores.looked, ["jolly-mute-magma"]);
    assert!(service.stores.opened.is_empty(), "no store opened to write");
    assert_eq!(service.identity.pins, [Some("pinned-id".to_string())]);
}

#[test]
fn test_a_list_opens_its_store_to_read() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    listed(&mut service, ListQuery::default());
    assert_eq!(service.stores.opened, ["jolly-mute-magma"], "the add's");
    assert_eq!(service.stores.looked, ["jolly-mute-magma"], "the list's");
}

#[test]
fn test_a_list_whose_project_does_not_resolve_fails_with_why() {
    let mut service = service(&[]);
    service.identity.fails = Some(Report::new("project-registry-x"));
    let error = ready(service.list(ListQuery::default())).unwrap_err();
    assert_eq!(error.code(), "project-registry-x");
}

#[test]
fn test_a_list_whose_store_does_not_open_fails() {
    let mut service = service(&[]);
    service.identity.bound = Some(project());
    service.stores.store.open_fails = true;
    let error = ready(service.list(ListQuery::default())).unwrap_err();
    assert_eq!(error.code(), "angzarr-ports-store-unavailable");
}

#[test]
fn test_an_add_in_a_status_class_is_refused() {
    let mut service = service(&["swift-amber"]);
    let task = NewTask {
        status: Some("@terminal".to_string()),
        ..new_task("Ship it")
    };
    let error = add(&mut service, task).unwrap_err();
    assert_eq!(error.code(), "tasks-status-reserved");
}

/// Each tag in its canonical spelling.
fn spelled(tags: &[tasks::Tag]) -> Vec<String> {
    tags.iter().map(ToString::to_string).collect()
}

// ------------------------------------------------------------ reading back

#[test]
fn test_a_task_read_back_as_no_events_is_never_a_blank_task() {
    // A stream purged mid-read can read back as a stream with no events.
    let mut service = service(&["swift-amber"]);
    service.stores.store.hollow = true;
    let error = add(&mut service, new_task("Ship it")).unwrap_err();
    assert_eq!(
        error,
        AppError::Vanished {
            harp_id: "swift-amber".to_string()
        }
    );
    assert!(listed(&mut service, ListQuery::default()).is_empty());
}

// ------------------------------------------------------------ dead letters

// the kernel writes a dead letter only for what angzarr would
// dead-letter (an ingest that panicked, a conflict window decide never
// acknowledged, a policy's malformed book); the tasks domain, with no
// routes, can be made to produce none. So each test writes one to the log
// the kernel is handed, as the kernel would during the write.
fn dead_letter(service: &Fake, line: &str) {
    angzarr_embedded::next::DeadLetterLog::write(&service.dead_letters, line);
}

fn letters(lines: &[&str]) -> Vec<crate::DeadLetter> {
    lines
        .iter()
        .map(|line| crate::DeadLetter {
            line: line.to_string(),
        })
        .collect()
}

// Every write returns the dead letters written while it ran, each once.
#[test]
fn test_every_write_returns_the_dead_letters_written_while_it_ran() {
    let mut service = service(&["swift-amber"]);
    dead_letter(&service, "DEAD LETTER: add");
    let written = add(&mut service, new_task("Ship it")).unwrap();
    assert_eq!(written.dead_letters, letters(&["DEAD LETTER: add"]));
    dead_letter(&service, "DEAD LETTER: tag");
    let written = ready(service.tag(change("swift-amber", &["x"], &[]))).unwrap();
    assert_eq!(written.dead_letters, letters(&["DEAD LETTER: tag"]));
    dead_letter(&service, "DEAD LETTER: status");
    let set = StatusChange {
        harp_id: "swift-amber".to_string(),
        status: Some("Done".to_string()),
        ..StatusChange::default()
    };
    let written = ready(service.set_status(set)).unwrap();
    assert_eq!(written.dead_letters, letters(&["DEAD LETTER: status"]));
    dead_letter(&service, "DEAD LETTER: edit");
    let edit = TextEdit {
        harp_id: "swift-amber".to_string(),
        text: "Ship it now".to_string(),
        ..TextEdit::default()
    };
    let written = ready(service.edit(edit.clone())).unwrap();
    assert_eq!(written.dead_letters, letters(&["DEAD LETTER: edit"]));
    let written = ready(service.edit(edit)).unwrap();
    assert!(written.dead_letters.is_empty(), "each is returned once");
}

// A write that fails carries the dead letters written while it ran on its
// error, each once; the error's diagnostic is the failure's own.
#[test]
fn test_every_failed_write_carries_the_dead_letters_written_while_it_ran() {
    let mut service = service(&["swift-amber", "calm-otter"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let missing = || StatusChange {
        harp_id: "no-such-harp".to_string(),
        status: Some("Done".to_string()),
        ..StatusChange::default()
    };
    let edit = TextEdit {
        harp_id: "no-such-harp".to_string(),
        text: "x".to_string(),
        ..TextEdit::default()
    };
    let failures: [(&str, Result<Written, AppError>); 4] = [
        ("add", {
            dead_letter(&service, "DEAD LETTER: add");
            add(&mut service, new_task(""))
        }),
        ("tag", {
            dead_letter(&service, "DEAD LETTER: tag");
            ready(service.tag(change("no-such-harp", &["x"], &[])))
        }),
        ("status", {
            dead_letter(&service, "DEAD LETTER: status");
            ready(service.set_status(missing()))
        }),
        ("edit", {
            dead_letter(&service, "DEAD LETTER: edit");
            ready(service.edit(edit))
        }),
    ];
    for (write, failed) in failures {
        let error = failed.unwrap_err();
        let line = format!("DEAD LETTER: {write}");
        assert_eq!(error.dead_letters(), letters(&[&line]), "{write}");
        let AppError::DeadLettered { error: cause, .. } = &error else {
            panic!("{write}: {error:?}");
        };
        assert_eq!(error.code(), cause.code(), "{write}");
        assert_eq!(
            format!("{:?}", error.args()),
            format!("{:?}", cause.args()),
            "{write}"
        );
        assert_eq!(error.to_string(), cause.to_string(), "{write}");
    }
    let error = ready(service.set_status(missing())).unwrap_err();
    assert_eq!(error.code(), "tasks-no-task");
    assert!(error.dead_letters().is_empty(), "each once");
    let written = add(&mut service, new_task("Ship more")).unwrap();
    assert!(written.dead_letters.is_empty(), "each once");
}

#[test]
fn test_a_dead_letter_renders_its_line() {
    let letter = &letters(&["DEAD LETTER: x"])[0];
    assert_eq!(letter.code(), "tasks-application-dead-letter");
    assert_eq!(letter.english(), "DEAD LETTER: x");
    assert_eq!(letter.args().len(), 1);
}

// ------------------------------------------- idempotency: a request's key
//
// The command id is given FAR earlier, at the edge, as the request's key:
// a request retried under its key sends the same command id, so the kernel
// answers what already applied DUPLICATE and nothing applies twice.

#[test]
fn test_a_status_change_retried_under_its_request_id_applies_once() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let change = StatusChange {
        request_id: Some("r-1".to_string()),
        ..moved("swift-amber", "In Progress", None)
    };
    let first = ready(service.set_status(change.clone())).unwrap();
    let again = ready(service.set_status(change)).unwrap();
    assert_eq!(again.task, first.task, "its time of change included");
    let ids = command_ids(&mut service, "swift-amber");
    assert_eq!(ids.len(), 2, "the add, and the status change once");
    assert_eq!(ids[1], crate::request_command_id("set_status", "r-1"));
}

// DUPLICATE answers with the task as it stands: a later change by another
// request is not undone, and the retried one is not applied over it.
#[test]
fn test_an_edit_retried_after_another_change_is_not_applied_again() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let first = TextEdit {
        request_id: Some("r-1".to_string()),
        ..edited("swift-amber", "First")
    };
    ready(service.edit(first.clone())).unwrap();
    ready(service.edit(edited("swift-amber", "Second"))).unwrap();
    let retried = ready(service.edit(first)).unwrap();
    assert_eq!(retried.task.text, "Second");
    let ids = command_ids(&mut service, "swift-amber");
    assert_eq!(ids.len(), 3, "the add and the two edits, once each");
    assert_eq!(ids[1], crate::request_command_id("edit", "r-1"));
}

#[test]
fn test_a_tag_change_retried_under_its_request_id_derives_its_command_id() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let tagging = TagChange {
        request_id: Some("r-1".to_string()),
        ..change("swift-amber", &["x"], &[])
    };
    ready(service.tag(tagging.clone())).unwrap();
    ready(service.tag(tagging)).unwrap();
    let ids = command_ids(&mut service, "swift-amber");
    assert_eq!(
        ids,
        [ids[0].clone(), crate::request_command_id("tag", "r-1")]
    );
}

// Until O, an add's request id is refused, given at all and before
// anything is read or written: an add's stream is its minted harp's, and
// the kernel deduplicates within a stream, so a retried add would mint another
// harp and add the task twice. The refusal says so, rather than the key
// promising what it cannot keep.
#[test]
fn test_an_add_given_a_request_id_is_refused_until_o() {
    let mut service = configured(&["unreadable"]);
    for key in ["r-1", ""] {
        let task = NewTask {
            request_id: Some(key.to_string()),
            ..new_task("Ship it")
        };
        let error = ready(service.add(task)).unwrap_err();
        assert_eq!(error, AppError::AddRequestIdNotYet, "{key:?}");
        assert_eq!(error.code(), "tasks-application-add-request-id-not-yet");
        assert_eq!(
            error.english(),
            "not yet: an add's request id needs fix O (the harp position pinned by the key)"
        );
    }
    assert!(service.configuration.asked == 0, "no configuration read");
    assert!(service.identity.pins.is_empty(), "no project resolved");
}

// O's starting point: once the key pins the harp an add mints, a retried
// add finds its first on that harp's stream and answers with it.
#[test]
#[ignore = "fix O: the harp position pinned by the request's key; until then an add's request id is refused"]
fn test_an_add_retried_under_its_request_id_answers_with_the_first_task() {
    let mut service = service(&["swift-amber", "calm-otter"]);
    let task = NewTask {
        request_id: Some("r-1".to_string()),
        ..new_task("Ship it")
    };
    let first = add(&mut service, task.clone()).unwrap();
    let again = add(&mut service, task).unwrap();
    assert_eq!(again.task.harp_id, first.task.harp_id);
    assert_eq!(listed(&mut service, ListQuery::default()), ["swift-amber"]);
}

#[test]
fn test_without_a_request_id_each_request_is_its_own() {
    let mut service = service(&["swift-amber"]);
    add(&mut service, new_task("Ship it")).unwrap();
    let change = moved("swift-amber", "In Progress", None);
    ready(service.set_status(change.clone())).unwrap();
    ready(service.set_status(change)).unwrap();
    assert_eq!(command_ids(&mut service, "swift-amber").len(), 3);
}

// A key given blank is given: refused, never read as none (which would
// make the retry a second request), before anything is read.
#[test]
fn test_a_blank_request_id_is_refused_before_anything_is_read() {
    let mut service = configured(&["unreadable"]);
    let blank = AppError::Blank {
        field: "request_id",
    };
    for key in ["", " "] {
        let key = Some(key.to_string());
        let moving = StatusChange {
            request_id: key.clone(),
            ..moved("swift-amber", "Done", None)
        };
        assert_eq!(ready(service.set_status(moving)).unwrap_err(), blank);
        let tagging = TagChange {
            request_id: key.clone(),
            ..change("swift-amber", &["x"], &[])
        };
        assert_eq!(ready(service.tag(tagging)).unwrap_err(), blank);
        let editing = TextEdit {
            request_id: key,
            ..edited("swift-amber", "New")
        };
        assert_eq!(ready(service.edit(editing)).unwrap_err(), blank);
    }
    assert!(service.configuration.asked == 0, "no configuration read");
    assert!(service.identity.pins.is_empty(), "no project resolved");
}
