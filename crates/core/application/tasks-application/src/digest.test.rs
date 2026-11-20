//! The digest over fake ports and angzarr-ports' in-memory store: one
//! entry per stream of the tasks' domains, each as the kernel loads it.

use angzarr_embedded::Kernel;

use crate::fakes::{ready, FakeClock, FakeDelay, FakeIdentity, FakeNames, FakeStores};
use crate::{registry, AppError, DigestService, NewTask, Service, TaskService};

fn service(
    identity: FakeIdentity,
    stores: FakeStores,
) -> Service<FakeIdentity, FakeStores, FakeNames, FakeClock, FakeDelay> {
    Service::new(
        identity,
        stores,
        FakeNames::of(&["dire-shore", "calm-reef"]),
        FakeClock::default(),
        FakeDelay::default(),
    )
}

#[test]
fn test_the_digest_folds_every_task_stream_then_the_schema_stream() {
    let mut service = service(FakeIdentity::bound(), FakeStores::permitting_every_tag());
    for text in ["first", "second"] {
        ready(service.add(NewTask {
            text: text.to_string(),
            ..NewTask::default()
        }))
        .unwrap();
    }
    let folded = ready(service.fold_digest(None)).expect("it folds");
    let store = service.stores.store.inner();
    let mut covers = ready(angzarr_embedded::EventStore::covers(store, "tasks")).unwrap();
    covers.extend(ready(angzarr_embedded::EventStore::covers(store, "schema")).unwrap());
    assert_eq!(covers.len(), 3, "two tasks and the schema stream");
    assert_eq!(
        folded.iter().map(|f| f.cover.clone()).collect::<Vec<_>>(),
        covers,
        "the task streams in the store's order, then the schema stream"
    );
    let registry = registry();
    let kernel = Kernel::new(&registry);
    for (entry, cover) in folded.iter().zip(&covers[..2]) {
        let (_, head) = ready(kernel.load::<tasks::TaskAggregate, _>(store, cover)).unwrap();
        assert_eq!(entry.head, head, "its head as the kernel loads it");
    }
    // The task and schema aggregates take no snapshot (no `snapshot =
    // ..`), so the runtime has no form of their state to show: a finding
    // against the plan's section 5.3.2, whose digest compares the state.
    assert!(
        folded.iter().all(|f| f.state.is_none()),
        "no state for a domain with no snapshot form"
    );
    assert_eq!(
        ready(service.fold_digest(None)).unwrap(),
        folded,
        "the same again"
    );
}

#[test]
fn test_the_digest_of_a_project_with_no_store_is_empty() {
    let stores = FakeStores {
        absent: true,
        ..FakeStores::default()
    };
    let mut service = service(FakeIdentity::bound(), stores);
    assert_eq!(ready(service.fold_digest(None)), Ok(Vec::new()));
}

#[test]
fn test_the_digest_with_no_project_is_refused_and_pins_what_it_was_given() {
    let mut unbound = service(FakeIdentity::default(), FakeStores::default());
    assert_eq!(ready(unbound.fold_digest(None)), Err(AppError::NoProject));
    let mut pinned = service(FakeIdentity::default(), FakeStores::default());
    ready(pinned.fold_digest(Some("jolly-mute-magma".to_string()))).unwrap();
    assert_eq!(
        pinned.identity.pins,
        [Some("jolly-mute-magma".to_string())],
        "the pin resolved, never minted"
    );
    assert_eq!(pinned.identity.writes, 0);
}

/// The digest of a known sequence is fixed: the same domains and heads,
/// whichever ids the streams were minted under (a golden, so a change to
/// how a stream folds shows as a change here, not only as a mismatch
/// between two builds).
#[test]
fn test_the_digest_of_a_known_sequence_is_its_domains_and_heads() {
    let mut service = service(FakeIdentity::bound(), FakeStores::permitting_every_tag());
    for text in ["first", "second"] {
        ready(service.add(NewTask {
            text: text.to_string(),
            ..NewTask::default()
        }))
        .unwrap();
    }
    let digest = ready(service.fold_digest(None)).unwrap();
    let shape: Vec<(String, u64)> = digest
        .iter()
        .map(|f| (f.cover.domain.clone(), f.head))
        .collect();
    assert_eq!(
        shape,
        [
            ("tasks".to_string(), 1),
            ("tasks".to_string(), 1),
            ("schema".to_string(), 1),
        ]
    );
}
