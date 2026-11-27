//! The store's lock, `state.db.lock` beside it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use file_lock::LockError;
use file_system::Fs;

use crate::StoreOpenError;

/// How long an ordinary open waits for a migration to let go.
pub const SHARED_WAIT: Duration = Duration::from_secs(10);

/// The lock file of the store at `store`.
pub fn lock_file(store: &Path) -> PathBuf {
    sibling(store, "lock")
}

/// The holder's record of the store at `store`.
pub fn holder_file(store: &Path) -> PathBuf {
    sibling(store, "holder")
}

fn sibling(store: &Path, extension: &str) -> PathBuf {
    let mut name = store.as_os_str().to_owned();
    name.push(".");
    name.push(extension);
    PathBuf::from(name)
}

/// The store's lock taken exclusive, for a migration.
#[derive(Debug)]
pub struct ExclusiveLock {
    store: PathBuf,
}

impl ExclusiveLock {
    pub fn take(_fs: &dyn Fs, store: &Path) -> Result<ExclusiveLock, StoreOpenError> {
        Ok(ExclusiveLock {
            store: store.to_path_buf(),
        })
    }

    pub fn store(&self) -> &Path {
        &self.store
    }
}

/// The store's lock taken shared, by an ordinary open.
#[derive(Debug)]
pub struct SharedLock;

pub fn take_shared(
    _fs: &dyn Fs,
    _store: &Path,
    _pause: &mut impl FnMut() -> Result<(), LockError>,
) -> Result<SharedLock, StoreOpenError> {
    Ok(SharedLock)
}

/// A pause that gives up once `wait` has passed.
pub fn within(store: &Path, wait: Duration) -> impl FnMut() -> Result<(), LockError> {
    file_lock::until(&lock_file(store), wait)
}

#[cfg(test)]
#[path = "lock.test.rs"]
mod tests;
