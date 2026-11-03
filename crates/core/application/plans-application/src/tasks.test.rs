//! The tasks port over the real tasks use cases, on the fakes' one
//! in-memory store: what exists, and the rules a store states.

use ports::{Names, Tasks};
use tasks_application::{NewTask, Service, TaskService};

use super::ProjectTasks;
use crate::fakes::{ready, FakeClock, FakeDelay, FakeIdentity, FakeStores};

/// Task ids in the order given.
struct Scripted(Vec<&'static str>);

impl Names for Scripted {
    fn task_id(&mut self) -> String {
        self.0.remove(0).to_string()
    }
    fn task_id_for(&mut self, _request_id: &str, _position: u64) -> String {
        unreachable!("no add here is given a request id")
    }
    fn project_id(&mut self) -> String {
        unreachable!("no project is minted here")
    }
}

type Real = Service<FakeIdentity, FakeStores, Scripted, FakeClock, FakeDelay>;

/// The tasks of a project whose store its first add makes.
fn tasks(held: &[&'static str]) -> ProjectTasks<Real> {
    let mut service = Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        Scripted(held.to_vec()),
        FakeClock::default(),
        FakeDelay::default(),
    );
    for harp in held {
        let task = NewTask {
            text: format!("Task {harp}"),
            ..NewTask::default()
        };
        ready(service.add(task)).unwrap();
    }
    ProjectTasks::new(service, Some("jolly-mute-magma".into()))
}

#[test]
fn a_task_the_project_holds_exists_and_another_does_not() {
    let mut tasks = tasks(&["swift-amber"]);
    assert_eq!(ready(tasks.exists("swift-amber")), Ok(true));
    assert_eq!(ready(tasks.exists("calm-otter")), Ok(false));
}

#[test]
fn the_rules_are_the_stores_tags_in_tagma_syntax() {
    let mut tasks = tasks(&["swift-amber"]);
    let rules = ready(tasks.rules()).unwrap();
    assert_eq!(rules, tag_rules::StoreRules::default().export());
    assert!(
        rules.iter().any(|r| r.starts_with("tagma.permit")),
        "{rules:?}"
    );
}
