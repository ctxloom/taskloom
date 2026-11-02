//! A plan on angzarr-embedded, driven through its kernel exactly as the
//! binary drives it: `Kernel::handle` over angzarr-ports' in-memory store,
//! which checks, identifies, deduplicates against the store, hands the
//! command to the marked-up [`PlanAggregate`] through angzarr-router, and
//! commits. The rules themselves are `lib.test.rs`'s, through this
//! harness; these hold the aggregate to the stream it belongs in, and the
//! wire's round trip.

use std::collections::BTreeMap;

use angzarr_diagnostic::{code, Arg, Diagnostic};
use angzarr_embedded::markup::Rejection;
use angzarr_embedded::testing::{Harness, Samples};
use angzarr_embedded::v1::Cover;
use angzarr_embedded::{CoverExt, Domain, Failure, Payload, Registry, Rejected};

use crate::{
    messages, AddPlan, PlanAggregate, PlanCommand, PlanEvent, PlanRejection, PlanState, PlanStatus,
    PLAN_DOMAIN,
};

pub(crate) const HARP: &str = "brisk-cedar-otter";
const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const HASH2: &str = "1111111111111111111111111111111111111111111111111111111111111111";

/// Plan streams, each through angzarr-embedded's test support: the kernel
/// over an in-memory store, one second after the last command, under a
/// command id of its own, a retryable failure not retried.
pub(crate) struct Plans {
    streams: BTreeMap<String, Harness<PlanAggregate>>,
}

impl Plans {
    pub(crate) fn new() -> Plans {
        Plans {
            streams: BTreeMap::new(),
        }
    }

    /// `harp`'s stream.
    fn stream(&mut self, harp: &str) -> &mut Harness<PlanAggregate> {
        self.streams.entry(harp.to_string()).or_insert_with(|| {
            let mut registry = Registry::new();
            registry
                .aggregate::<PlanAggregate>(PlanAggregate::default)
                .unwrap();
            Harness::new(registry, cover(harp)).with_samples(Samples::starting_at(100))
        })
    }

    /// Run `command` on `harp`'s stream: the events it committed, or its
    /// refusal.
    // The kernel's own Rejected, as its callers receive it, holding the
    // handler's code as owned text.
    #[allow(clippy::result_large_err)]
    fn run(&mut self, harp: &str, command: PlanCommand) -> Result<Vec<PlanEvent>, Rejected> {
        let message = messages::PlanCommand::from(&command);
        match self.stream(harp).when(message).result() {
            Ok(events) => Ok(events.into_iter().map(PlanEvent::from).collect()),
            Err(Failure::Rejected(rejected)) => Err(rejected),
            Err(other) => panic!("not a rejection: {other:?}"),
        }
    }

    /// [`Plans::run`], its refusal the plan's own, typed, as its handler
    /// made it.
    pub(crate) fn decide(
        &mut self,
        harp: &str,
        command: PlanCommand,
    ) -> Result<Vec<PlanEvent>, PlanRejection> {
        self.run(harp, command).map_err(|rejected| {
            rejected
                .rejection::<<PlanAggregate as Domain>::Rejection>()
                .and_then(Rejection::message)
                .cloned()
                .unwrap_or_else(|| panic!("the plan's own refusal: {rejected:?}"))
        })
    }

    fn pages(&mut self, harp: &str) -> Vec<angzarr_embedded::v1::EventPage> {
        self.stream(harp).pages()
    }

    /// The plan, replayed from the store as the next process would.
    pub(crate) fn state(&mut self, harp: &str) -> PlanState {
        self.stream(harp).state().state().clone()
    }
}

fn cover(harp: &str) -> Cover {
    Cover::natural(PLAN_DOMAIN, harp)
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
            code!("transition").to_string(),
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
    assert_eq!(said(&refused).0, code!("already-added"));
}

#[test]
fn an_add_naming_another_harp_than_its_stream_is_refused() {
    let mut plans = Plans::new();
    let refused = plans.run(HARP, add("calm-dusty-heron")).unwrap_err();
    assert_eq!(
        said(&refused),
        (
            "angzarr-embedded-not-its-stream".to_string(),
            vec![
                ("domain".to_string(), "plans".to_string()),
                ("key".to_string(), "calm-dusty-heron".to_string())
            ]
        )
    );
    assert!(plans.pages(HARP).is_empty());
}

#[test]
fn a_command_to_an_empty_stream_is_refused_as_no_such_plan() {
    let mut plans = Plans::new();
    let refused = plans.run(HARP, PlanCommand::Reinstate).unwrap_err();
    assert_eq!(said(&refused).0, code!("no-such-plan"));
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
        let packed = messages::PlanCommand::from(&command).to_any();
        let read = messages::PlanCommand::from_any(&packed).map(PlanCommand::from);
        assert_eq!(read, Ok(command));
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
        let packed = messages::PlanEvent::from(&event).to_any();
        let read = messages::PlanEvent::from_any(&packed).map(PlanEvent::from);
        assert_eq!(read, Ok(event));
    }
    assert_eq!(messages::PlanCommand::type_urls().len(), 9);
    assert_eq!(messages::PlanEvent::type_urls().len(), 9);
}

#[test]
fn a_payload_of_another_type_is_unknown() {
    let other = angzarr_embedded::pack(&plans_proto::v1::PlanState::default());
    assert!(matches!(
        messages::PlanEvent::from_any(&other),
        Err(angzarr_embedded::PayloadError::UnknownType { .. })
    ));
    assert!(matches!(
        messages::PlanCommand::from_any(&other),
        Err(angzarr_embedded::PayloadError::UnknownType { .. })
    ));
}

#[test]
fn a_status_left_unspecified_on_the_wire_is_an_invalid_payload() {
    let unspecified = angzarr_embedded::pack(&plans_proto::v1::SetPlanStatus { status: 0 });
    assert!(matches!(
        messages::PlanCommand::from_any(&unspecified),
        Err(angzarr_embedded::PayloadError::Invalid { .. })
    ));
}

#[test]
fn a_rejections_display_is_its_code_and_args() {
    assert_eq!(
        PlanRejection::AlreadyGoverns("swift-amber".into()).to_string(),
        concat!(code!("already-governs"), " task=swift-amber")
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
