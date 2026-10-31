//! The plan list, projected live and caught up: each command runs through
//! angzarr-embedded's kernel over angzarr-ports' in-memory store with the
//! list attached as a projection; a fresh list catches up from the store,
//! as every read of another process does, and gives the same list.

use std::future::Future;
use std::marker::PhantomData;

use angzarr_embedded::next::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::next::v1::{command_page, Command, CommandBook, CommandPage, Cover, Uuid};
use angzarr_embedded::next::{
    ids, Kernel, MemoryDeadLetters, OnRetryable, Payload, ProjectionRoute, Registry, Sampled,
    Timestamp,
};
use plans::{AddPlan, PlanAggregate, PlanCommand, PlanStatus, PLAN_DOMAIN};

use crate::PlanList;

const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

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
        registry.aggregate::<PlanAggregate>(PlanAggregate::default);
        Project {
            store: MemoryStore::new(),
            registry,
            seconds: 100,
        }
    }

    /// Run `command` on plan `harp`'s stream, projecting into `list`.
    fn run(&mut self, list: &mut PlanList, harp: &str, command: PlanCommand) {
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
            name: "plans",
            projection: list,
            domain: PhantomData::<fn() -> PlanAggregate>,
        };
        ready(
            Kernel::new(&self.registry, &dead_letters)
                .on_retryable(OnRetryable {
                    max_retries: 0,
                    ..OnRetryable::default()
                })
                .handle::<PlanAggregate, _, _>(
                    &self.store,
                    book(harp, command.to_any()),
                    &sampled,
                    &mut route,
                ),
        )
        .unwrap_or_else(|failure| panic!("{failure:?}"));
    }

    /// A list built afresh from the store: another process's.
    fn rebuilt(&self) -> PlanList {
        let mut list = PlanList::new();
        ready(list.catch_up(&self.store, &self.registry)).unwrap();
        list
    }
}

fn book(harp: &str, payload: angzarr_embedded::next::Any) -> CommandBook {
    CommandBook {
        cover: Some(Cover {
            domain: PLAN_DOMAIN.to_string(),
            root: Some(ids::compute_root(PLAN_DOMAIN, harp).unwrap()),
            edition: None,
        }),
        page: Some(CommandPage {
            body: Some(command_page::Body::Command(Command {
                payload: Some(payload),
            })),
            ..CommandPage::default()
        }),
        handling: None,
    }
}

fn add(harp: &str) -> PlanCommand {
    PlanCommand::Add(AddPlan {
        harp_id: harp.to_string(),
        title: format!("Title of {harp}"),
        status: PlanStatus::Draft,
        path: format!(".taskloom/plans/{harp}.plan.md"),
        content_hash: HASH.to_string(),
        origin_session: String::new(),
    })
}

/// Two plans: `calm-heron` governs a task and supersedes `brisk-otter`.
fn two(project: &mut Project, live: &mut PlanList) {
    project.run(live, "brisk-otter", add("brisk-otter"));
    project.run(live, "calm-heron", add("calm-heron"));
    project.run(
        live,
        "calm-heron",
        PlanCommand::Govern("swift-amber".into()),
    );
    project.run(
        live,
        "calm-heron",
        PlanCommand::Supersede("brisk-otter".into()),
    );
    project.run(
        live,
        "brisk-otter",
        PlanCommand::MarkSuperseded("calm-heron".into()),
    );
}

#[test]
fn a_list_projected_live_is_the_list_caught_up_from_the_store() {
    let (mut project, mut live) = (Project::new(), PlanList::new());
    two(&mut project, &mut live);
    let rebuilt = project.rebuilt();
    assert_eq!(rebuilt.plans(), live.plans());
    let otter = rebuilt.get("brisk-otter").unwrap();
    assert_eq!(otter.status, PlanStatus::Superseded);
    assert_eq!(otter.superseded_by, "calm-heron");
    let heron = rebuilt.get("calm-heron").unwrap();
    assert_eq!(heron.governs, vec!["swift-amber"]);
    assert_eq!(heron.supersedes, vec!["brisk-otter"]);
    assert_eq!(
        rebuilt
            .governing("swift-amber")
            .iter()
            .map(|r| r.harp_id.as_str())
            .collect::<Vec<_>>(),
        vec!["calm-heron"]
    );
}

#[test]
fn a_list_caught_up_again_sees_what_landed_since_and_folds_nothing_twice() {
    let mut project = Project::new();
    two(&mut project, &mut PlanList::new());
    let mut list = project.rebuilt();
    project.run(
        &mut PlanList::new(),
        "calm-heron",
        PlanCommand::Ungovern("swift-amber".into()),
    );
    ready(list.catch_up(&project.store, &project.registry)).unwrap();
    ready(list.catch_up(&project.store, &project.registry)).unwrap();
    assert!(list.get("calm-heron").unwrap().governs.is_empty());
    assert!(list.governing("swift-amber").is_empty());
    assert_eq!(list.projected.get("calm-heron"), Some(&3));
}

#[test]
fn an_empty_store_catches_up_to_an_empty_list() {
    assert!(Project::new().rebuilt().plans().is_empty());
}
