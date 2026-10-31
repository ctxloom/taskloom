//! The service beyond the reviewed use cases: its races, forced (the store
//! runs another process's write between the service's read and its
//! write: the racing test, not a racing product), a supersede left half
//! done, the hash recorded once, the pinned project, and an import's
//! reruns.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use angzarr_embedded::next::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::next::{Kernel, MemoryDeadLetters, Payload, Sampled};
use angzarr_embedded_diagnostic::Report;
use plans::{PlanCommand, PlanRejection};
use ports::{Mint, PlanFiles, Tasks, Timestamp, Uuid};

use super::{book, registry};
use crate::fakes::{ready, Service};
use crate::{
    content_hash, Link, NewPlan, PlanError, PlanQuery, PlanStatus, Plans, KERNEL_RETRY, RETIRED_TAG,
};

#[derive(Default)]
struct Files(RefCell<BTreeMap<String, Vec<u8>>>);

impl Files {
    fn put(&self, path: &str, text: &str) {
        self.0
            .borrow_mut()
            .insert(path.to_string(), text.as_bytes().to_vec());
    }
    fn text(&self, path: &str) -> Option<String> {
        self.0
            .borrow()
            .get(path)
            .map(|b| String::from_utf8(b.clone()).unwrap())
    }
}

impl PlanFiles for Files {
    fn work_root(&self) -> Result<String, Report> {
        Ok("/tree".to_string())
    }
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.borrow().get(path).cloned()
    }
    fn create(&self, path: &str, text: &[u8]) -> Result<bool, Report> {
        let mut files = self.0.borrow_mut();
        if files.contains_key(path) {
            return Ok(false);
        }
        files.insert(path.to_string(), text.to_vec());
        Ok(true)
    }
    fn plan_files(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{dir}/");
        self.0
            .borrow()
            .keys()
            .filter(|p| p.starts_with(&prefix) && !p[prefix.len()..].contains('/'))
            .cloned()
            .collect()
    }
}

/// Tasks by harp id, each with its `ctxloom:plan` values.
#[derive(Default)]
struct FakeTasks(RefCell<BTreeMap<String, BTreeSet<String>>>);

impl FakeTasks {
    fn with(tasks: &[(&str, &[&str])]) -> FakeTasks {
        let fake = FakeTasks::default();
        for (harp, values) in tasks {
            fake.0.borrow_mut().insert(
                harp.to_string(),
                values.iter().map(|v| v.to_string()).collect(),
            );
        }
        fake
    }
}

impl Tasks for &FakeTasks {
    async fn exists(&mut self, harp_id: &str) -> Result<bool, Report> {
        Ok(self.0.borrow().contains_key(harp_id))
    }
    async fn tagged(&mut self, key: &str) -> Result<Vec<(String, String)>, Report> {
        assert_eq!(key, RETIRED_TAG);
        Ok(self
            .0
            .borrow()
            .iter()
            .flat_map(|(h, vs)| vs.iter().map(move |v| (h.clone(), v.clone())))
            .collect())
    }
    async fn untag(&mut self, harp_id: &str, _: &str, value: &str) -> Result<(), Report> {
        self.0.borrow_mut().get_mut(harp_id).unwrap().remove(value);
        Ok(())
    }
}

/// Mints `plan-1`, `plan-2`, ... after any scripted ids.
#[derive(Default)]
struct Counter(RefCell<(u32, Vec<String>)>);

impl Mint for Counter {
    fn mint(&self) -> String {
        let mut state = self.0.borrow_mut();
        if !state.1.is_empty() {
            return state.1.remove(0);
        }
        state.0 += 1;
        format!("plan-{}", state.0)
    }
}

fn new(title: &str) -> NewPlan {
    NewPlan {
        title: title.to_string(),
        ..NewPlan::default()
    }
}

/// Run `command` on plan `harp`'s stream in `store` directly, as another
/// process would.
fn other_process(store: &MemoryStore, harp: &str, command: PlanCommand) {
    let registry = registry();
    let dead_letters = MemoryDeadLetters::new();
    let sampled = Sampled {
        now: Timestamp {
            seconds: 1_800_000_000,
            nanos: 0,
        },
        command_id: Uuid::from_bytes([0xee; 16]),
    };
    let head = {
        let cover = super::cover(harp);
        let at = angzarr_embedded::next::StreamRef::try_from(&cover).unwrap();
        ready(angzarr_embedded::next::EventStore::read(store, at, 0, None))
            .unwrap()
            .next_sequence
    };
    ready(
        Kernel::new(&registry, &dead_letters)
            .on_retryable(KERNEL_RETRY)
            .handle::<plans::PlanAggregate, _, _>(
                store,
                book(harp, &command, head),
                &sampled,
                &mut (),
            ),
    )
    .unwrap_or_else(|failure| panic!("{failure:?}"));
}

#[test]
fn a_write_landing_between_the_read_and_the_write_is_decided_again_and_both_land() {
    let tasks = FakeTasks::with(&[("task-a", &[]), ("task-b", &[])]);
    let (files, mint) = (Files::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    service.add(new("One")).unwrap();
    service
        .0
        .borrow()
        .stores
        .store
        .before_next_write(Box::new(|store| {
            other_process(store, "plan-1", PlanCommand::Govern("task-b".into()))
        }));
    let plan = service
        .link("plan-1", Link::Governs("task-a".into()))
        .unwrap();
    assert_eq!(plan.governs, vec!["task-a", "task-b"]);
    let service = service.0.borrow();
    assert_eq!(service.delay.slept.len(), 1, "one conflict, one wait");
}

#[test]
fn a_race_won_by_the_same_change_comes_back_as_its_refusal() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    service.add(new("One")).unwrap();
    service
        .0
        .borrow()
        .stores
        .store
        .before_next_write(Box::new(|store| {
            other_process(store, "plan-1", PlanCommand::SetStatus(PlanStatus::Active))
        }));
    assert_eq!(
        service.set_status("plan-1", PlanStatus::Active),
        Err(PlanError::Rejected(PlanRejection::StatusUnchanged(
            PlanStatus::Active
        )))
    );
}

#[test]
fn a_supersede_of_a_plan_another_supersedes_is_refused_naming_it() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    for title in ["One", "Two", "Three"] {
        service.add(new(title)).unwrap();
    }
    service
        .link("plan-2", Link::Supersedes("plan-1".into()))
        .unwrap();
    assert_eq!(
        service.link("plan-3", Link::Supersedes("plan-1".into())),
        Err(PlanError::AlreadySuperseded {
            plan: "plan-1".into(),
            by: "plan-2".into()
        })
    );
    assert_eq!(
        service.link("plan-2", Link::Supersedes("plan-1".into())),
        Err(PlanError::Rejected(PlanRejection::AlreadySupersedes(
            "plan-1".into()
        )))
    );
    assert_eq!(
        service.link("plan-1", Link::Supersedes("plan-1".into())),
        Err(PlanError::Rejected(PlanRejection::SupersedesItself))
    );
}

#[test]
fn a_supersede_left_half_done_is_finished_by_linking_again() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    service.add(new("One")).unwrap();
    service.add(new("Two")).unwrap();
    // The first write landed; the second never did.
    other_process(
        &service.0.borrow().stores.store.inner,
        "plan-2",
        PlanCommand::Supersede("plan-1".into()),
    );
    service
        .link("plan-2", Link::Supersedes("plan-1".into()))
        .unwrap();
    let one = service.show("plan-1").unwrap().plan;
    assert_eq!(
        (one.status, one.superseded_by.as_str()),
        (PlanStatus::Superseded, "plan-2")
    );
}

#[test]
fn an_unlink_left_half_done_is_finished_by_unlinking_again() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    service.add(new("One")).unwrap();
    service.add(new("Two")).unwrap();
    service
        .link("plan-2", Link::Supersedes("plan-1".into()))
        .unwrap();
    other_process(
        &service.0.borrow().stores.store.inner,
        "plan-2",
        PlanCommand::Unsupersede("plan-1".into()),
    );
    service
        .unlink("plan-2", Link::Supersedes("plan-1".into()))
        .unwrap();
    assert_eq!(
        service.show("plan-1").unwrap().plan.status,
        PlanStatus::Active
    );
    assert_eq!(
        service.unlink("plan-2", Link::Supersedes("plan-1".into())),
        Err(PlanError::Rejected(PlanRejection::NotSuperseded(
            "plan-1".into()
        )))
    );
}

#[test]
fn unlink_governs_removes_the_link_and_a_missing_plan_is_named() {
    let tasks = FakeTasks::with(&[("task-a", &[])]);
    let (files, mint) = (Files::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    service
        .add(NewPlan {
            governs: vec!["task-a".into(), "task-a".into()],
            ..new("One")
        })
        .unwrap();
    let plan = service
        .unlink("plan-1", Link::Governs("task-a".into()))
        .unwrap();
    assert!(plan.governs.is_empty());
    assert_eq!(
        service.unlink("plan-9", Link::Governs("task-a".into())),
        Err(PlanError::NoSuchPlan("plan-9".into()))
    );
    assert_eq!(
        service.set_status("plan-9", PlanStatus::Active),
        Err(PlanError::NoSuchPlan("plan-9".into()))
    );
}

#[test]
fn an_edit_is_recorded_once_however_often_it_is_read() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    service.add(new("One")).unwrap();
    files.put(".taskloom/plans/plan-1.plan.md", "edited");
    let before = service.0.borrow().stores.store.writes();
    service.list(&PlanQuery::default());
    service.show("plan-1").unwrap();
    service.check();
    assert_eq!(service.0.borrow().stores.store.writes(), before + 1);
    let shown = service.show("plan-1").unwrap();
    assert_eq!(shown.plan.content_hash, content_hash(b"edited"));
    assert_eq!(shown.content.as_deref(), Some("edited"));
}

#[test]
fn a_new_plans_file_carries_its_title_and_session_and_the_list_its_sessions() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    let plan = service
        .add(NewPlan {
            session: "inept-sixth-stick".into(),
            status: PlanStatus::Active,
            ..new("  Plans: phase 2  ")
        })
        .unwrap();
    assert_eq!(plan.title, "Plans: phase 2");
    assert_eq!(plan.status, PlanStatus::Active);
    assert_eq!(plan.sessions, vec!["inept-sixth-stick"]);
    let text = files.text(".taskloom/plans/plan-1.plan.md").unwrap();
    assert!(text.contains("title: \"Plans: phase 2\""), "{text}");
}

#[test]
fn an_add_the_aggregate_would_refuse_writes_no_file() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    assert_eq!(
        service.add(new("  ")),
        Err(PlanError::Rejected(PlanRejection::EmptyTitle))
    );
    assert_eq!(
        service.add(NewPlan {
            status: PlanStatus::Done,
            ..new("T")
        }),
        Err(PlanError::Rejected(PlanRejection::InitialStatus(
            PlanStatus::Done
        )))
    );
    assert!(files.0.borrow().is_empty());
}

#[test]
fn a_minted_id_a_plan_holds_is_minted_again() {
    let (files, tasks) = (Files::default(), FakeTasks::default());
    let mint = Counter(RefCell::new((0, vec!["plan-1".into(), "plan-1".into()])));
    let mut service = Service::new(&files, &tasks, &mint);
    assert_eq!(service.add(new("One")).unwrap().harp_id, "plan-1");
    // plan-1 twice more, both held, then the counter's plan-1, then plan-2.
    assert_eq!(service.add(new("Two")).unwrap().harp_id, "plan-2");
}

#[test]
fn the_pinned_project_is_the_one_resolved() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let service = Service::new(&files, &tasks, &mint);
    let pinned = service
        .0
        .into_inner()
        .pinned(Some("calm-mute-otter".into()));
    let mut service = Service(RefCell::new(pinned));
    service.add(new("One")).unwrap();
    assert!(service
        .0
        .borrow()
        .identity
        .pins
        .iter()
        .all(|pin| pin.as_deref() == Some("calm-mute-otter")));
    assert!(service.0.get_mut().dead_letters().is_empty());
}

#[test]
fn an_external_plan_tagged_again_after_its_import_is_its_copys_plan() {
    let files = Files::default();
    files.put("/home/b/v1.plan.md", "# V1\n");
    let tasks = FakeTasks::with(&[("task-a", &["/home/b/v1.plan.md"]), ("task-b", &[])]);
    let mint = Counter::default();
    let mut service = Service::new(&files, &tasks, &mint);
    let first = service.import(false);
    assert!(first.plans[0].new);
    assert!(!first.left_any());
    tasks
        .0
        .borrow_mut()
        .get_mut("task-b")
        .unwrap()
        .insert("/home/b/v1.plan.md".into());
    let again = service.import(false);
    assert_eq!(again.plans.len(), 1);
    assert!(!again.plans[0].new);
    assert_eq!(again.plans[0].path, ".taskloom/plans/plan-1.plan.md");
    assert_eq!(again.plans[0].governs, vec!["task-a", "task-b"]);
    assert_eq!(service.list(&PlanQuery::default()).len(), 1);
}

#[test]
fn an_import_with_a_value_left_says_so() {
    let files = Files::default();
    let tasks = FakeTasks::with(&[("task-a", &["gone.plan.md"])]);
    let mint = Counter::default();
    let mut service = Service::new(&files, &tasks, &mint);
    let report = service.import(false);
    assert!(report.left_any());
    assert!(report.plans.is_empty() && report.retired.is_empty());
    assert_eq!(tasks.0.borrow()["task-a"].len(), 1, "its tag is kept");
}

#[test]
fn a_registered_plan_governs_by_its_command_and_its_event_names_it() {
    // The book a command is sent in: on the plan's stream, at the head read.
    let command = PlanCommand::Govern("task-a".into());
    let sent = book("plan-1", &command, 3);
    assert_eq!(sent.handling.unwrap().expected_sequence, Some(3));
    assert_eq!(sent.cover, Some(super::cover("plan-1")));
    let page = sent.page.unwrap();
    let Some(angzarr_embedded::next::v1::command_page::Body::Command(body)) = page.body else {
        panic!("a command page");
    };
    assert_eq!(PlanCommand::from_any(&body.payload.unwrap()), Ok(command));
}
