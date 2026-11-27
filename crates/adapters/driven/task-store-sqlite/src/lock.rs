//! The store's lock, `state.db.lock` beside it: rust-support's advisory file
//! lock (`flock(2)` on unix) through the filesystem seam, taken by
//! [`os::Os::try_lock`].
//!
//! - Every ordinary open of the store takes it SHARED ([`take_shared`]),
//!   waiting up to [`SHARED_WAIT`] for an exclusive holder to let go: many
//!   openers coexist.
//! - A migration takes it EXCLUSIVE ([`ExclusiveLock::take`]) and never
//!   waits: it is refused at once while any opener holds the store.
//! - A read-only open takes none and makes no file ([`crate::SqliteStores`]
//!   `open_existing`), so an archive stays byte for byte.
//! - `flock` conflicts between open file descriptions of ONE process, so a
//!   process holding the exclusive lock opens the store through its guard
//!   ([`crate::SqliteStores::open_through`]), never by taking the lock again.
//! - The operating system releases a lock with its process, so none outlives
//!   its holder. What can is the holder's record, `state.db.holder` (its pid,
//!   when it took the lock, its command line), which a refusal names. It is
//!   advisory and the most recent taker's: shared holders overwrite one
//!   another's, so a refusal may name a holder that has since gone while
//!   another still holds, or none. It never gates the lock.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use file_lock::{LockError, Locked};
use file_system::{Fs, Open, Perms};
use os::{LockKind, Mode};

use crate::StoreOpenError;

/// How long an ordinary open waits for an exclusive holder to let go.
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

/// The store's lock, exclusive, held until dropped: a migration's. Open the
/// store through it ([`crate::SqliteStores::open_through`]).
#[derive(Debug)]
pub struct ExclusiveLock {
    store: PathBuf,
    _taken: Taken,
}

impl ExclusiveLock {
    /// Take the lock of the store at `store`, or be refused at once
    /// ([`StoreOpenError::Held`]) with the holder it names.
    pub fn take(fs: Arc<dyn Fs>, store: &Path) -> Result<ExclusiveLock, StoreOpenError> {
        let lock = lock_file(store);
        let locked = file_lock::lock_within(&*fs, &lock, Duration::ZERO)
            .map_err(|e| refused(&*fs, store, e))?;
        Ok(ExclusiveLock {
            store: store.to_path_buf(),
            _taken: Taken::new(locked, fs, store),
        })
    }

    /// The store this lock holds.
    pub fn store(&self) -> &Path {
        &self.store
    }
}

/// The store's lock, shared, held until dropped: an ordinary open's.
#[derive(Debug)]
pub struct SharedLock {
    _taken: Taken,
}

/// Take the lock of the store at `store` shared. `pause` is what the taker
/// does between attempts while an exclusive holder has it, and gives up with
/// a timeout ([`within`]); the lock is [`StoreOpenError::Held`] then.
pub fn take_shared(
    fs: Arc<dyn Fs>,
    store: &Path,
    pause: &mut impl FnMut() -> Result<(), LockError>,
) -> Result<SharedLock, StoreOpenError> {
    let lock = lock_file(store);
    let locked = file_lock::take_current(&*fs, &lock, || shared_once(&*fs, &lock, pause))
        .map_err(|e| refused(&*fs, store, e))?;
    Ok(SharedLock {
        _taken: Taken::new(locked, fs, store),
    })
}

/// A pause that gives up once `wait` has passed.
pub fn within(store: &Path, wait: Duration) -> impl FnMut() -> Result<(), LockError> {
    file_lock::until(&lock_file(store), wait)
}

/// The lock file opened (made, 0600, if missing) and locked shared, pausing
/// while an exclusive holder has it.
fn shared_once(
    fs: &dyn Fs,
    lock: &Path,
    pause: &mut impl FnMut() -> Result<(), LockError>,
) -> Result<File, LockError> {
    let io = |e: std::io::Error| LockError::Io {
        path: lock.display().to_string(),
        detail: e.to_string(),
    };
    let file = match fs.open(lock, Open::Read) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs.open(lock, Open::Create(Mode::FILE))
        }
        opened => opened,
    }
    .map_err(io)?;
    loop {
        if fs.os().try_lock(&file, LockKind::Shared).map_err(io)? {
            return Ok(file);
        }
        pause()?;
    }
}

/// Why `error` refused the lock of `store`: another holds it, or it could
/// not be taken at all.
fn refused(fs: &dyn Fs, store: &Path, error: LockError) -> StoreOpenError {
    let lock = lock_file(store).display().to_string();
    match error {
        LockError::Timeout { .. } => StoreOpenError::Held {
            lock,
            holder: read_holder(fs, store),
        },
        other => StoreOpenError::Lock {
            path: lock,
            detail: other.to_string(),
        },
    }
}

/// A lock held, and this process's record of holding it, removed on drop.
#[derive(Debug)]
struct Taken {
    _locked: Locked,
    fs: Arc<dyn Fs>,
    record: PathBuf,
}

impl Taken {
    fn new(locked: Locked, fs: Arc<dyn Fs>, store: &Path) -> Taken {
        let record = holder_file(store);
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let command = std::env::args().collect::<Vec<_>>().join(" ");
        let text = format!("{}\t{started}\t{command}", std::process::id());
        // Advisory: a record that cannot be written leaves the lock held
        // and a refusal naming no holder.
        let _ = fs.write_atomic(&record, text.as_bytes(), Perms::Exact(Mode::FILE));
        Taken {
            _locked: locked,
            fs,
            record,
        }
    }
}

impl Drop for Taken {
    /// Removes the record only if it is this process's: another holder may
    /// have written its own since.
    fn drop(&mut self) {
        let ours = std::process::id().to_string();
        let mine = self
            .fs
            .read_to_string(&self.record)
            .is_ok_and(|text| text.split('\t').next() == Some(ours.as_str()));
        if mine {
            let _ = self.fs.remove_file(&self.record);
        }
    }
}

/// The holder the store's record names, as `<pid> <command line>`.
fn read_holder(fs: &dyn Fs, store: &Path) -> Option<String> {
    let text = fs.read_to_string(&holder_file(store)).ok()?;
    let mut fields = text.splitn(3, '\t');
    let (pid, _started, command) = (fields.next()?, fields.next()?, fields.next()?);
    Some(format!("{pid} {command}"))
}

#[cfg(test)]
#[path = "lock.test.rs"]
mod tests;
