//! A plan on angzarr-embedded, driven through its kernel exactly as the
//! binary drives it: `Kernel::handle` over angzarr-ports' in-memory store,
//! which checks, identifies, deduplicates against the store, hands the
//! command to the marked-up [`PlanAggregate`] through angzarr-router, and
//! commits. The rules themselves are `lib.test.rs`'s, over decide and apply;
//! these hold the aggregate to them, the stream it belongs in, and the
//! wire's round trip.

use std::future::Future;

use angzarr_embedded::next::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::next::v1::{command_page, Command, CommandBook, CommandPage, Cover, Uuid};
use angzarr_embedded::next::{
    ids, Domain, EventStore, Failure, Handled, Kernel, MemoryDeadLetters, OnRetryable, Payload,
    Registry, Rejected, Sampled, Stream, StreamRef, Timestamp,
};
use angzarr_embedded_diagnostic::{Arg, Diagnostic};

use crate::{
    AddPlan, PlanAggregate, PlanCommand, PlanEvent, PlanRejection, PlanState, PlanStatus,
    PLAN_DOMAIN,
};

const HARP: &str = "brisk-cedar-otter";
const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const HASH2: &str = "1111111111111111111111111111111111111111111111111111111111111111";

/// Poll `future` once: the in-memory store never waits.
fn ready<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let waker = std::task::Waker::noop();
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(waker))
    {
        std::task::Poll::Ready(out) => out,
        std::task::Poll::Pending => panic!("the in-memory store never waits"),
    }
}

/// One project's store holding plan streams.
struct Plans {
    store: MemoryStore,
    registry: Registry,
    seconds: i64,
}

impl Plans {
    fn new() -> Plans {
        let mut registry = Registry::new();
        registry.aggregate::<PlanAggregate>(PlanAggregate::default);
        Plans {
            store: MemoryStore::new(),
            registry,
            seconds: 100,
        }
    }

    /// Run `command` on `harp`'s stream: the events it committed, or its
    /// refusal.
    fn run(&mut self, harp: &str, command: PlanCommand) -> Result<Vec<PlanEvent>, Rejected> {
        match self.try_run(book(harp, command.to_any())) {
            Ok(Handled::Applied(recorded)) => Ok(recorded
                .books()
                .iter()
                .flat_map(|book| &book.pages)
                .map(|page| PlanEvent::from_any(page.payload.as_ref().unwrap()).unwrap())
                .collect()),
            Ok(Handled::Duplicate(_)) => panic!("a fresh command id is never a duplicate"),
            Ok(Handled::Parked) => panic!("only a fact is parked, and these are commands"),
            Err(failure) => match *failure {
                Failure::Rejected(rejected) => Err(rejected),
                other => panic!("not a rejection: {other:?}"),
            },
        }
    }

    /// Handle `book` through the kernel, one second after the last command,
    /// under a command id of its own. A retryable failure is not retried.
    fn try_run(&mut self, book: CommandBook) -> Result<Handled, Box<Failure<Rejected>>> {
        self.seconds += 1;
        let sampled = Sampled {
            now: Timestamp {
                seconds: self.seconds,
                nanos: 0,
            },
            command_id: Uuid::from_bytes([self.seconds as u8; 16]),
        };
        let dead_letters = MemoryDeadLetters::new();
        ready(
            Kernel::new(&self.registry, &dead_letters)
                .on_retryable(OnRetryable {
                    max_retries: 0,
                    ..OnRetryable::default()
                })
                .handle::<PlanAggregate, _, _>(&self.store, book, &sampled, &mut ()),
        )
        .map_err(Box::new)
    }

    fn pages(&self, harp: &str) -> Vec<angzarr_embedded::next::v1::EventPage> {
        let cover = cover(harp);
        let stream = StreamRef::try_from(&cover).unwrap();
        ready(self.store.read(stream, 0, None)).unwrap().pages
    }

    /// The plan, replayed from the store as the next process would.
    fn state(&self, harp: &str) -> PlanState {
        Stream::<PlanAggregate>::replay(cover(harp), &self.pages(harp), &self.registry)
            .unwrap()
            .state()
            .state()
            .clone()
    }
}

fn cover(harp: &str) -> Cover {
    Cover {
        domain: PLAN_DOMAIN.to_string(),
        root: Some(ids::compute_root(PLAN_DOMAIN, harp).unwrap()),
        edition: None,
    }
}

/// A client's command to `harp`'s stream, with no ids of its own.
fn book(harp: &str, payload: angzarr_embedded::next::Any) -> CommandBook {
    CommandBook {
        cover: Some(cover(harp)),
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
        title: "Plans move into taskloom".to_string(),
        status: PlanStatus::Draft,
        path: format!(".taskloom/plans/{harp}.plan.md"),
        content_hash: HASH.to_string(),
        origin_session: "inept-sixth-stick".to_string(),
    })
}

/// A rejection's code and its args as text.
fn said(rejected: &Rejected) -> (String, Vec<(String, String)>) {
    let args = rejected
        .args()
        .into_iter()
        .map(|(name, arg)| {
            let text = match arg {
                Arg::Str(s) => s,
                other => format!("{other:?}"),
            };
            (name.to_string(), text)
        })
        .collect();
    (rejected.code().to_string(), args)
}

#[test]
fn the_aggregate_is_the_plans_domain() {
    assert_eq!(<PlanAggregate as Domain>::NAME, PLAN_DOMAIN);
}

#[test]
fn an_add_through_the_kernel_records_plan_added_and_replays_to_the_plan() {
    let mut plans = Plans::new();
    let events = plans.run(HARP, add(HARP)).unwrap();
    let PlanCommand::Add(added) = add(HARP) else {
        unreachable!()
    };
    assert_eq!(events, vec![PlanEvent::PlanAdded(added)]);
    let state = plans.state(HARP);
    assert!(state.added);
    assert_eq!(state.harp_id, HARP);
    assert_eq!(state.path, ".taskloom/plans/brisk-cedar-otter.plan.md");
    assert_eq!(state.origin_session, "inept-sixth-stick");
}

#[test]
fn every_command_routes_through_the_kernel_and_replays() {
    let mut plans = Plans::new();
    plans.run(HARP, add(HARP)).unwrap();
    for command in [
        PlanCommand::SetStatus(PlanStatus::Active),
        PlanCommand::Govern("swift-amber".into()),
        PlanCommand::Govern("calm-heron".into()),
        PlanCommand::Ungovern("calm-heron".into()),
        PlanCommand::Supersede("old-plan".into()),
        PlanCommand::Supersede("older-plan".into()),
        PlanCommand::Unsupersede("older-plan".into()),
        PlanCommand::MarkSuperseded("next-plan".into()),
        PlanCommand::Reinstate,
        PlanCommand::MarkSuperseded("other-plan".into()),
        PlanCommand::Rehash(HASH2.into()),
    ] {
        assert_eq!(
            plans.run(HARP, command.clone()).map(|e| e.len()),
            Ok(1),
            "{command:?}"
        );
    }
    let state = plans.state(HARP);
    assert_eq!(state.status, PlanStatus::Superseded);
    assert_eq!(state.superseded_by, "other-plan");
    assert_eq!(
        state.governs.iter().collect::<Vec<_>>(),
        vec!["swift-amber"]
    );
    assert_eq!(
        state.supersedes.iter().collect::<Vec<_>>(),
        vec!["old-plan"]
    );
    assert_eq!(state.content_hash, HASH2);
    assert_eq!(plans.pages(HARP).len(), 12);
}

#[test]
fn a_refusal_through_the_kernel_is_its_code_and_args_and_records_nothing() {
    let mut plans = Plans::new();
    plans.run(HARP, add(HARP)).unwrap();
    let refused = plans
        .run(HARP, PlanCommand::SetStatus(PlanStatus::Done))
        .unwrap_err();
    assert_eq!(
        said(&refused),
        (
            "plans-transition".to_string(),
            vec![
                ("from".to_string(), "draft".to_string()),
                ("to".to_string(), "done".to_string())
            ]
        )
    );
    assert_eq!(plans.pages(HARP).len(), 1);
}

#[test]
fn a_second_add_to_a_stream_is_refused_as_already_added() {
    let mut plans = Plans::new();
    plans.run(HARP, add(HARP)).unwrap();
    let refused = plans.run(HARP, add(HARP)).unwrap_err();
    assert_eq!(said(&refused).0, "plans-already-added");
}

#[test]
fn an_add_naming_another_harp_than_its_stream_is_refused() {
    let mut plans = Plans::new();
    let refused = plans.run(HARP, add("calm-dusty-heron")).unwrap_err();
    assert_eq!(
        said(&refused),
        (
            "plans-not-its-stream".to_string(),
            vec![("harp_id".to_string(), "calm-dusty-heron".to_string())]
        )
    );
    assert!(plans.pages(HARP).is_empty());
}

#[test]
fn a_command_to_an_empty_stream_is_refused_as_no_such_plan() {
    let mut plans = Plans::new();
    let refused = plans.run(HARP, PlanCommand::Reinstate).unwrap_err();
    assert_eq!(said(&refused).0, "plans-no-such-plan");
}

#[test]
fn every_command_and_event_round_trips_through_its_payload() {
    let PlanCommand::Add(added) = add(HARP) else {
        unreachable!()
    };
    for command in [
        add(HARP),
        PlanCommand::SetStatus(PlanStatus::Abandoned),
        PlanCommand::Govern("t".into()),
        PlanCommand::Ungovern("t".into()),
        PlanCommand::Supersede("p".into()),
        PlanCommand::Unsupersede("p".into()),
        PlanCommand::MarkSuperseded("p".into()),
        PlanCommand::Reinstate,
        PlanCommand::Rehash(HASH2.into()),
    ] {
        assert_eq!(PlanCommand::from_any(&command.to_any()), Ok(command));
    }
    for event in [
        PlanEvent::PlanAdded(added),
        PlanEvent::StatusChanged {
            from: PlanStatus::Done,
            to: PlanStatus::Active,
        },
        PlanEvent::Governed("t".into()),
        PlanEvent::Ungoverned("t".into()),
        PlanEvent::Superseded("p".into()),
        PlanEvent::Unsuperseded("p".into()),
        PlanEvent::MarkedSuperseded {
            by: "p".into(),
            from: PlanStatus::Done,
        },
        PlanEvent::Reinstated,
        PlanEvent::Rehashed(HASH2.into()),
    ] {
        assert_eq!(PlanEvent::from_any(&event.to_any()), Ok(event));
    }
    assert_eq!(PlanCommand::type_urls().len(), 9);
    assert_eq!(PlanEvent::type_urls().len(), 9);
}

#[test]
fn a_payload_of_another_type_is_unknown() {
    let other = angzarr_embedded::next::pack(&plans_proto::v1::PlanState::default());
    assert!(matches!(
        PlanEvent::from_any(&other),
        Err(angzarr_embedded::next::PayloadError::UnknownType { .. })
    ));
    assert!(matches!(
        PlanCommand::from_any(&other),
        Err(angzarr_embedded::next::PayloadError::UnknownType { .. })
    ));
}

#[test]
fn a_status_left_unspecified_on_the_wire_is_an_invalid_payload() {
    let unspecified = angzarr_embedded::next::pack(&plans_proto::v1::SetPlanStatus { status: 0 });
    assert!(matches!(
        PlanCommand::from_any(&unspecified),
        Err(angzarr_embedded::next::PayloadError::Invalid { .. })
    ));
}

#[test]
fn a_rejections_display_is_its_code_and_args() {
    assert_eq!(
        PlanRejection::AlreadyGoverns("swift-amber".into()).to_string(),
        "plans-already-governs task=swift-amber"
    );
}

#[test]
fn check_refuses_what_decide_refuses_of_a_command_alone() {
    let PlanCommand::Add(good) = add(HARP) else {
        unreachable!()
    };
    let refused = |add: AddPlan| crate::check(&PlanCommand::Add(add));
    assert_eq!(crate::check(&PlanCommand::Add(good.clone())), Ok(()));
    assert_eq!(
        refused(AddPlan {
            title: " ".into(),
            ..good.clone()
        }),
        Err(PlanRejection::EmptyTitle)
    );
    assert_eq!(
        refused(AddPlan {
            status: PlanStatus::Done,
            ..good.clone()
        }),
        Err(PlanRejection::InitialStatus(PlanStatus::Done))
    );
    assert_eq!(
        refused(AddPlan {
            content_hash: "x".into(),
            ..good
        }),
        Err(PlanRejection::BadHash("x".into()))
    );
    assert_eq!(
        crate::check(&PlanCommand::Rehash("ABC".into())),
        Err(PlanRejection::BadHash("ABC".into()))
    );
    assert_eq!(
        crate::check(&PlanCommand::SetStatus(PlanStatus::Superseded)),
        Err(PlanRejection::SupersededBySupersedeOnly)
    );
    assert_eq!(
        crate::check(&PlanCommand::SetStatus(PlanStatus::Done)),
        Ok(())
    );
    assert_eq!(crate::check(&PlanCommand::Reinstate), Ok(()));
}

#[test]
fn a_hash_is_well_formed_only_as_64_lowercase_hex_digits() {
    assert_eq!(crate::well_formed(HASH2), Ok(()));
    let upper = "A".repeat(64);
    assert_eq!(
        crate::well_formed(&upper),
        Err(PlanRejection::BadHash(upper.clone()))
    );
    let short = "a".repeat(63);
    assert!(crate::well_formed(&short).is_err());
    let g = format!("{}g", "a".repeat(63));
    assert!(crate::well_formed(&g).is_err());
}

// A status change replays: the next process sees the plan Active, and so
// lets it be done.
#[test]
fn a_status_change_replays_into_the_next_command() {
    let mut plans = Plans::new();
    plans.run(HARP, add(HARP)).unwrap();
    plans
        .run(HARP, PlanCommand::SetStatus(PlanStatus::Active))
        .unwrap();
    assert_eq!(plans.state(HARP).status, PlanStatus::Active);
    assert_eq!(
        plans
            .run(HARP, PlanCommand::SetStatus(PlanStatus::Done))
            .map(|e| e.len()),
        Ok(1)
    );
    assert_eq!(plans.state(HARP).status, PlanStatus::Done);
}
