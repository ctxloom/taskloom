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
    let (first, head) = ready(kernel.load::<tasks::TaskAggregate, _>(store, &covers[0])).unwrap();
    assert_eq!(folded[0].head, head);
    assert_eq!(
        folded[0].state,
        angzarr_embedded::pack(&angzarr_embedded::State::to_snapshot(first.state())),
        "its state as the kernel loads it"
    );
    assert_eq!(
        folded[2].state.type_url, "/ctxloom.tasks.v1.SchemaState",
        "the schema stream's state"
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
