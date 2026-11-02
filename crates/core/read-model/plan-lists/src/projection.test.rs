//! The plan list on the projector port, caught up: each command runs
//! through angzarr-embedded's kernel over angzarr-ports' in-memory store,
//! and a list kept across commands catches up after each; a fresh list
//! catches up from the store, as every read of another process does, and
//! gives the same list.

use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::v1::{command_page, Command, CommandBook, CommandPage, Cover, Uuid};
use angzarr_embedded::v1::{EventBook, EventPage};
use angzarr_embedded::{ids, CatchUp, Kernel, OnRetryable, Payload, Registry, Sampled, Timestamp};
use plans::{messages, AddPlan, PlanAggregate, PlanCommand, PlanEvent, PlanStatus, PLAN_DOMAIN};

use crate::{PlanList, PlanLists};

/// A list, caught up from a store as the application keeps one.
type List = CatchUp<PlanLists>;

const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Run a future whose every await is ready at once: the test support's.
use angzarr_embedded::testing::block_on_ready as ready;

/// One project's store, and the clock its commands are sampled at.
struct Project {
    store: MemoryStore,
    registry: Registry,
    seconds: i64,
}

impl Project {
    fn new() -> Project {
        let mut registry = Registry::new();
        registry
            .aggregate::<PlanAggregate>(PlanAggregate::default)
            .unwrap();
        Project {
            store: MemoryStore::new(),
            registry,
            seconds: 100,
        }
    }

    /// Run `command` on plan `harp`'s stream, then catch `list` up.
    fn run(&mut self, list: &mut List, harp: &str, command: PlanCommand) {
        self.seconds += 1;
        let sampled = Sampled {
            now: Timestamp {
                seconds: self.seconds,
                nanos: 0,
            },
            command_id: Uuid::from_bytes([self.seconds as u8; 16]),
        };
        ready(
            Kernel::new(&self.registry)
                .on_retryable(OnRetryable {
                    max_retries: 0,
                    ..OnRetryable::default()
                })
                .handle::<PlanAggregate, _>(
                    &self.store,
                    book(harp, messages::PlanCommand::from(&command).to_any()),
                    &sampled,
                ),
        )
        .unwrap_or_else(|failure| panic!("{failure:?}"));
        self.catch_up(list);
    }

    /// Catch `list` up: the streams it handed a book.
    fn catch_up(&self, list: &mut List) -> usize {
        let caught = ready(list.step(&self.store, self.registry.catalog())).unwrap();
        assert!(caught.failures().is_empty(), "{:?}", caught.failures());
        caught.handed().len()
    }

    /// A list built afresh from the store: another process's.
    fn rebuilt(&self) -> List {
        let mut list = PlanList::catch_up();
        self.catch_up(&mut list);
        list
    }
}

fn book(harp: &str, payload: angzarr_embedded::Any) -> CommandBook {
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
fn two(project: &mut Project, live: &mut List) {
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
    let (mut project, mut live) = (Project::new(), PlanList::catch_up());
    two(&mut project, &mut live);
    let rebuilt = project.rebuilt();
    let rebuilt = rebuilt.projector().list();
    assert_eq!(rebuilt.plans(), live.projector().list().plans());
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
    two(&mut project, &mut PlanList::catch_up());
    let mut list = project.rebuilt();
    project.run(
        &mut PlanList::catch_up(),
        "calm-heron",
        PlanCommand::Ungovern("swift-amber".into()),
    );
    assert_eq!(project.catch_up(&mut list), 1, "the one stream written");
    assert_eq!(project.catch_up(&mut list), 0, "nothing twice");
    let list = list.projector().list();
    assert!(list.get("calm-heron").unwrap().governs.is_empty());
    assert!(list.governing("swift-amber").is_empty());
}

#[test]
fn an_empty_store_catches_up_to_an_empty_list() {
    assert!(Project::new()
        .rebuilt()
        .projector()
        .list()
        .plans()
        .is_empty());
}

/// A plan stream's book, `events` from `from` on, of a stream `length` long.
fn plan_book(harp: &str, from: u64, events: &[PlanEvent], length: u64) -> EventBook {
    EventBook {
        cover: book(harp, angzarr_embedded::Any::default()).cover,
        pages: (from..)
            .zip(events)
            .map(|(sequence, event)| EventPage {
                sequence,
                payload: Some(messages::PlanEvent::from(event).to_any()),
                meta: None,
            })
            .collect(),
        next_sequence: length,
        snapshot: None,
    }
}

fn added(harp: &str) -> PlanEvent {
    let PlanCommand::Add(add) = add(harp) else {
        unreachable!()
    };
    PlanEvent::PlanAdded(add)
}

/// A stream handed again from its start (one purged and written again) is
/// a new plan: what the list held of the old one, its governing included,
/// leaves it first.
#[test]
fn a_stream_handed_from_its_start_replaces_what_the_list_held_of_it() {
    let mut list = PlanList::new();
    let governed = PlanEvent::Governed("swift-amber".into());
    list.project(&plan_book(
        "calm-heron",
        0,
        &[added("calm-heron"), governed],
        2,
    ))
    .unwrap();
    assert_eq!(list.governing("swift-amber").len(), 1);
    list.project(&plan_book("calm-heron", 0, &[added("calm-heron")], 1))
        .unwrap();
    assert!(list.get("calm-heron").unwrap().governs.is_empty());
    assert!(list.governing("swift-amber").is_empty(), "no old governing");
}

/// A stream handed as gone (no event, at 0) takes its plan with it; a
/// later book of another stream is untouched by it.
#[test]
fn a_gone_stream_takes_its_plan_with_it() {
    let mut list = PlanList::new();
    list.project(&plan_book("calm-heron", 0, &[added("calm-heron")], 1))
        .unwrap();
    list.project(&plan_book("brisk-otter", 0, &[added("brisk-otter")], 1))
        .unwrap();
    list.project(&plan_book("calm-heron", 0, &[], 0)).unwrap();
    let plans: Vec<&str> = list.plans().iter().map(|p| p.harp_id.as_str()).collect();
    assert_eq!(plans, ["brisk-otter"]);
    list.project(&plan_book("brisk-otter", 1, &[], 1)).unwrap();
    assert_eq!(
        list.plans().len(),
        1,
        "an empty book past the start changes nothing"
    );
}

/// The list on the projector port answers each book with its projection:
/// the stream, its length, and the list's name.
#[test]
fn the_list_answers_each_book_with_its_projection() {
    use angzarr_embedded::ProjectorHandler;
    let book = plan_book("calm-heron", 0, &[added("calm-heron")], 1);
    let lists = PlanLists::default();
    let answered = ready(lists.dispatch(&book)).unwrap();
    assert_eq!(answered.projector, "plan-list");
    assert_eq!(answered.cover, book.cover);
    assert_eq!(answered.next_sequence, 1);
    assert_eq!(lists.list().plans().len(), 1, "and folds it");
}

/// An event the plans domain cannot read fails its book as a bug naming
/// the type it could not read, as the task list's does; the list keeps
/// nothing of the book.
#[test]
fn an_unreadable_event_is_the_lists_bug_naming_its_type() {
    use angzarr_embedded::ProjectorHandler;
    let unknown = "/ctxloom.plans.v9.Unknown";
    let mut book = plan_book("calm-heron", 0, &[added("calm-heron")], 2);
    book.pages.push(EventPage {
        sequence: 1,
        payload: Some(angzarr_embedded::Any {
            type_url: unknown.to_string(),
            value: Vec::new(),
        }),
        meta: None,
    });
    let lists = PlanLists::default();
    let error = ready(lists.dispatch(&book)).unwrap_err();
    assert_eq!(
        error.extras.get("cause.type_url").map(String::as_str),
        Some(unknown),
        "{error:?}"
    );
    assert!(lists.list().plans().is_empty());
}
