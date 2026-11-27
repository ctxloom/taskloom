//! The store's lock: shared by every ordinary open, exclusive for a
//! migration, on a filesystem of the test's own. A wait is forced by the
//! pause the taker is given, never by sleeping.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use file_lock::LockError;
use file_system::{Fs, Rooted};
use os::Mode;

use super::{holder_file, lock_file, take_shared, within, ExclusiveLock};
use crate::StoreOpenError;

/// A store's path on a filesystem of its own, its directory made.
fn store(fs: &Arc<Rooted>) -> PathBuf {
    let dir = os::fixture("/home/u/.ctxloom/projects/jolly-mute-magma");
    fs.create_dir_all(&dir, Mode::DIR).unwrap();
    dir.join("state.db")
}

/// A pause that must not be reached: the lock was free.
fn never() -> impl FnMut() -> Result<(), LockError> {
    || panic!("the taker waited for a lock that was free")
}

/// What a refusal names as the holder, the pid first.
fn holder_of(error: StoreOpenError) -> String {
    match error {
        StoreOpenError::Held {
            holder: Some(holder),
            ..
        } => holder,
        other => panic!("not held by a named holder: {other:?}"),
    }
}

#[test]
fn test_two_shared_takes_coexist() {
    let fs = Arc::new(Rooted::temp().unwrap());
    let store = store(&fs);
    let first = take_shared(fs.clone(), &store, &mut never()).unwrap();
    let second = take_shared(fs.clone(), &store, &mut never()).unwrap();
    drop((first, second));
}

#[test]
fn test_an_exclusive_take_is_refused_while_a_shared_one_holds_naming_the_holder() {
    let fs = Arc::new(Rooted::temp().unwrap());
    let store = store(&fs);
    let _shared = take_shared(fs.clone(), &store, &mut never()).unwrap();
    let refused = ExclusiveLock::take(fs.clone(), &store).unwrap_err();
    assert!(
        matches!(&refused, StoreOpenError::Held { lock, .. } if lock.ends_with("state.db.lock")),
        "{refused:?}"
    );
    assert!(holder_of(refused).starts_with(&std::process::id().to_string()));
}

#[test]
fn test_a_shared_take_is_refused_while_an_exclusive_one_holds_naming_the_holder() {
    let fs = Arc::new(Rooted::temp().unwrap());
    let store = store(&fs);
    let _exclusive = ExclusiveLock::take(fs.clone(), &store).unwrap();
    let refused = take_shared(fs.clone(), &store, &mut within(&store, Duration::ZERO)).unwrap_err();
    assert!(holder_of(refused).starts_with(&std::process::id().to_string()));
}

#[test]
fn test_a_shared_take_waits_then_succeeds_when_the_exclusive_holder_lets_go_in_time() {
    let fs = Arc::new(Rooted::temp().unwrap());
    let store = store(&fs);
    let exclusive = Cell::new(Some(ExclusiveLock::take(fs.clone(), &store).unwrap()));
    let waits = Cell::new(0);
    let mut pause = || {
        waits.set(waits.get() + 1);
        drop(exclusive.take());
        Ok(())
    };
    take_shared(fs.clone(), &store, &mut pause).expect("taken once the exclusive holder let go");
    assert_eq!(waits.get(), 1, "it waited once, for the holder");
}

#[test]
fn test_a_shared_take_is_held_when_the_window_passes_with_the_exclusive_holder_still_there() {
    let fs = Arc::new(Rooted::temp().unwrap());
    let store = store(&fs);
    let _exclusive = ExclusiveLock::take(fs.clone(), &store).unwrap();
    let mut pause = || {
        Err(LockError::Timeout {
            path: lock_file(&store).display().to_string(),
            waited_ms: 10_000,
        })
    };
    let refused = take_shared(fs.clone(), &store, &mut pause).unwrap_err();
    assert!(
        matches!(refused, StoreOpenError::Held { .. }),
        "{refused:?}"
    );
}

#[test]
fn test_dropping_a_lock_lets_the_next_taker_in_and_clears_its_record() {
    let fs = Arc::new(Rooted::temp().unwrap());
    let store = store(&fs);
    drop(ExclusiveLock::take(fs.clone(), &store).unwrap());
    assert!(!fs.exists(&holder_file(&store)).unwrap());
    drop(take_shared(fs.clone(), &store, &mut never()).unwrap());
    assert!(!fs.exists(&holder_file(&store)).unwrap());
    ExclusiveLock::take(fs.clone(), &store).expect("free after both let go");
}

#[test]
fn test_the_lock_is_a_file_beside_the_store() {
    let fs = Arc::new(Rooted::temp().unwrap());
    let store = store(&fs);
    let _held = take_shared(fs.clone(), &store, &mut never()).unwrap();
    assert_eq!(lock_file(&store), store.with_file_name("state.db.lock"));
    assert!(fs.exists(&lock_file(&store)).unwrap());
}
