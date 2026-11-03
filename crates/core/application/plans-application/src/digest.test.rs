//! The plans' digest over the fakes: one entry per plan stream, as the
//! kernel loads it. The streams are written by the kernel directly, as
//! another process would; the digest needs no work tree, tasks or mint.

use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::v1::{CommandBook, Cover};
use angzarr_embedded::{CommandBookExt, CoverExt, EventStore, Kernel, Sampled};
use plans::{AddPlan, PlanAggregate, PlanCommand, PlanStatus};
use ports::{Timestamp, Uuid};

use crate::fakes::{ready, FakeClock, FakeDelay, FakeIdentity, FakeStores};
use crate::{registry, DigestPlans, PlanService};

/// The plan service over the fakes, its work tree, tasks and mint none.
type Digesting = PlanService<FakeIdentity, FakeStores, (), (), (), FakeClock, FakeDelay>;

fn service() -> Digesting {
    PlanService::new(
        FakeIdentity::default(),
        FakeStores::default(),
        (),
        (),
        (),
        FakeClock::default(),
        FakeDelay::default(),
    )
}

/// Add plan `harp` to `store` through the kernel.
fn added(store: &MemoryStore, harp: &str) {
    let registry = registry();
    let command = PlanCommand::Add(AddPlan {
        harp_id: harp.to_string(),
        title: format!("Plan {harp}"),
        status: PlanStatus::Draft,
        path: format!(".taskloom/plans/{harp}.plan.md"),
        content_hash: "h".to_string(),
        origin_session: "inept-sixth-stick".to_string(),
    });
    let book = CommandBook::command_of(
        Cover::natural(plans::PLAN_DOMAIN, harp),
        &plans::messages::PlanCommand::from(&command),
    );
    let sampled = Sampled {
        now: Timestamp {
            seconds: 1_800_000_000,
            nanos: 0,
        },
        command_id: Uuid::from_bytes([harp.len() as u8; 16]),
    };
    ready(Kernel::new(&registry).handle::<PlanAggregate, _>(store, book, &sampled))
        .unwrap_or_else(|failure| panic!("{failure:?}"));
}

#[test]
fn test_the_digest_folds_every_plan_stream_as_the_kernel_loads_it() {
    let mut service = service();
    added(service.stores.store.inner(), "calm-reef");
    added(service.stores.store.inner(), "dire-shore-x");
    let folded = ready(service.fold_digest()).expect("it folds");
    let store = service.stores.store.inner();
    let covers = ready(store.covers(plans::PLAN_DOMAIN)).unwrap();
    assert_eq!(covers.len(), 2);
    assert_eq!(
        folded.iter().map(|f| f.cover.clone()).collect::<Vec<_>>(),
        covers,
        "one per plan stream, in the store's order"
    );
    let registry = registry();
    let (plan, head) =
        ready(Kernel::new(&registry).load::<PlanAggregate, _>(store, &covers[1])).unwrap();
    assert_eq!(folded[1].head, head);
    assert_eq!(
        folded[1].state,
        angzarr_embedded::pack(&angzarr_embedded::State::to_snapshot(plan.state())),
        "its state as the kernel loads it"
    );
    assert_eq!(
        ready(service.fold_digest()).unwrap(),
        folded,
        "the same again"
    );
}

#[test]
fn test_the_digest_of_a_project_with_no_store_is_empty() {
    let mut service = service();
    service.stores.absent.insert("jolly-mute-magma".to_string());
    assert_eq!(ready(service.fold_digest()), Ok(Vec::new()));
}
