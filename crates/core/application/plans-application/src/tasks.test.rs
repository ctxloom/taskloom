//! The tasks port over the real tasks use cases, on the fakes' one
//! in-memory store: what exists, the tags of a key, and their removal.

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
    fn task_id_for(&mut self, _request_id: &str) -> String {
        unreachable!("no add here is given a request id")
    }
    fn project_id(&mut self) -> String {
        unreachable!("no project is minted here")
    }
}

type Real = Service<FakeIdentity, FakeStores, Scripted, FakeClock, FakeDelay>;

fn tasks(tagged: &[(&'static str, &[&str])]) -> ProjectTasks<Real> {
    let mut service = Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        Scripted(tagged.iter().map(|(harp, _)| *harp).collect()),
        FakeClock::default(),
        FakeDelay::default(),
    );
    for (harp, tags) in tagged {
        let task = NewTask {
            text: format!("Task {harp}"),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..NewTask::default()
        };
        ready(service.add(task)).unwrap();
    }
    ProjectTasks::new(service, Some("jolly-mute-magma".into()))
}

#[test]
fn a_task_the_project_holds_exists_and_another_does_not() {
    let mut tasks = tasks(&[("swift-amber", &[])]);
    assert_eq!(ready(tasks.exists("swift-amber")), Ok(true));
    assert_eq!(ready(tasks.exists("calm-otter")), Ok(false));
}

#[test]
fn tagged_gives_each_task_and_value_of_a_key_and_untag_removes_one() {
    let mut tasks = tasks(&[
        (
            "swift-amber",
            &["doc:link=a", "doc:link=\"docs/b.plan.md\"", "urgent"],
        ),
        ("calm-otter", &["doc:link=a", "doc:other=a"]),
        ("brisk-heron", &["link=a"]),
    ]);
    let mut tagged = ready(tasks.tagged("doc:link")).unwrap();
    tagged.sort();
    let pair = |h: &str, v: &str| (h.to_string(), v.to_string());
    assert_eq!(
        tagged,
        vec![
            pair("calm-otter", "a"),
            pair("swift-amber", "a"),
            pair("swift-amber", "docs/b.plan.md"),
        ]
    );
    ready(tasks.untag("swift-amber", "doc:link", "docs/b.plan.md")).unwrap();
    let mut tagged = ready(tasks.tagged("doc:link")).unwrap();
    tagged.sort();
    assert_eq!(
        tagged,
        vec![pair("calm-otter", "a"), pair("swift-amber", "a")]
    );
    assert!(tasks.dead_letters().is_empty());
}

#[test]
fn untag_of_a_task_the_project_does_not_hold_is_the_tasks_refusal() {
    let mut tasks = tasks(&[("swift-amber", &[])]);
    let refused = ready(tasks.untag("calm-otter", "doc:link", "a")).unwrap_err();
    assert!(
        angzarr_embedded_diagnostic::Diagnostic::code(&refused).starts_with("tasks"),
        "{refused}"
    );
}
