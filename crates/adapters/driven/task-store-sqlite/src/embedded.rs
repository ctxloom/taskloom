//! A project's task store on angzarr-store-sqlite's [`SqliteStore`] (step
//! 3b of taskloom-on-angzarr-embedded.plan.md), beside the annals one
//! ([`crate::SqliteStores`]) until the application moves (step 4), when it
//! replaces it behind the ports.
//!
//! The annals opener's behaviour, on the new store:
//!
//! - The path is the caller's, as before: a project's
//!   ([`Project::store`], `~/.ctxloom/projects/<project-id>/state.db`), on
//!   the [`Fs`] the stores are given, SQLite opening the file where the
//!   seam puts it on disk.
//! - [`EmbeddedStores::open`] opens it to write, creating its directory
//!   and the file and migrating it ([`SqliteStore::open`]);
//!   [`EmbeddedStores::open_existing`] opens it read-only and never
//!   creates, migrates or writes ([`SqliteStore::open_existing`]), finding
//!   no store where there is no file. Each holds the store it opened, for
//!   its project, until another is asked for.
//! - A store written before angzarr-embedded (annals-store-sql's, holding
//!   only its first migration) is refused by both, keyed
//!   `task-store-sqlite-predates-embedded` with its `path` and the
//!   `backup` to move it to (`state.db.pre-embedded`), and left exactly
//!   as it was: never migrated in place (the owner's ruling D2).
//! - The stores take `&self` (angzarr-ports'), so the opener lends the
//!   store shared: a purge is the store's own [`Purge`](angzarr_ports::Purge),
//!   its own call, and a watch is its own [`Watch`](angzarr_ports::Watch)
//!   token, paired with which opening of the store answered it
//!   ([`Token`]), since tokens compare only from one connection.
//! - [`EmbeddedStores::check`] checks a database whole
//!   ([`SqliteStore::integrity_check`]), opened afresh and read-only.
//!   Damage is a key and its args, never English: the store's
//!   `angzarr-store-sqlite-damaged` (its `first` problem and how many
//!   `more`), or `angzarr-store-sqlite-damaged-file` for a file too damaged
//!   to open (SQLite's `errno`, and its own text as `ext_err_txt`). A
//!   failure to run the check is the store's error, a third party's text
//!   as `ext_err_txt` there too. No database is no damage.
//! - Errors are angzarr-ports' [`StoreError`]; this crate's own refusals
//!   travel in it as a cause, its code and args.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use angzarr_embedded_diagnostic::Report;
use angzarr_ports::{ChangeToken, StoreError, Watch};
use angzarr_store_sqlite::{Integrity, SqlError, SqliteStore};
use file_system::Fs;
use os::Mode;
use ports::Project;

use crate::StoreOpenError;

/// How many of angzarr-store-sqlite's migrations a store from before
/// angzarr-embedded lacks: annals-store-sql applied only the first, which
/// angzarr-store-sqlite's first is blob-identical to, and the pinned
/// angzarr-store-sqlite has four. A store read-only with exactly these
/// pending is annals', not an older angzarr-embedded build's. A re-pin that
/// adds a migration raises it (the D2 test fails until it does).
const PENDING_SINCE_ANNALS: u64 = 3;

/// The SQLite store of the project last asked for, held until another is.
pub struct EmbeddedStores {
    fs: Arc<dyn Fs>,
    held: Option<Held>,
    /// How many stores have been opened: the last one's handle.
    opened: u64,
}

/// A store held open, and for what.
struct Held {
    path: PathBuf,
    /// Opened to write (created and migrated if need be), not read-only.
    writable: bool,
    /// Which opening this is, for its tokens ([`Token::handle`]).
    handle: u64,
    store: SqliteStore,
}

/// A held store's change token, and which opening of it answered: tokens
/// compare only from one connection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Token {
    /// Which opening of the store answered: one per store opened.
    pub handle: u64,
    pub change: ChangeToken,
}

/// What checking a project's database found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Checked {
    /// No database is there: nothing to check, and no damage.
    Absent,
    /// The database reported no problem.
    Intact,
    /// The database reported problems in its own file, or is too damaged
    /// to open: the store's diagnostic of them, its code and args.
    Damaged(Report),
}

impl EmbeddedStores {
    /// The stores on `fs`, none opened yet.
    pub fn new(fs: Arc<dyn Fs>) -> EmbeddedStores {
        EmbeddedStores {
            fs,
            held: None,
            opened: 0,
        }
    }

    /// `store`, just opened, held for `path` under a handle of its own.
    fn hold(&mut self, path: &Path, writable: bool, store: SqliteStore) -> &mut Held {
        self.opened += 1;
        self.held.insert(Held {
            path: path.to_path_buf(),
            writable,
            handle: self.opened,
            store,
        })
    }

    /// The store at `project`'s path, opened to write: its directory and
    /// file created and migrated if need be. Held after.
    pub async fn open(&mut self, project: &Project) -> Result<&SqliteStore, StoreError> {
        let path = &project.store;
        let held = self.held.take().filter(|h| &h.path == path && h.writable);
        if let Some(held) = held {
            return Ok(&self.held.insert(held).store);
        }
        create_parent(&*self.fs, path).map_err(unavailable)?;
        let real = real(&*self.fs, path).map_err(unavailable)?;
        // A read first: a store from before angzarr-embedded is refused
        // before the open to write would migrate it.
        if let Err(error) = SqliteStore::open_existing(&real).await {
            refuse_if_annals(&error, path)?;
        }
        let store = SqliteStore::open(&real).await?;
        Ok(&self.hold(path, true, store).store)
    }

    /// The store at `project`'s path, held or opened read-only; `None` if
    /// there is no file there, and nothing is made.
    pub async fn open_existing(
        &mut self,
        project: &Project,
    ) -> Result<Option<&SqliteStore>, StoreError> {
        Ok(self.hold_existing(project).await?.map(|held| &held.store))
    }

    /// The store held for `project`, or the one at its path opened
    /// read-only and held; `None` if there is none.
    async fn hold_existing(&mut self, project: &Project) -> Result<Option<&mut Held>, StoreError> {
        let path = &project.store;
        if let Some(held) = self.held.take().filter(|h| &h.path == path) {
            return Ok(Some(self.held.insert(held)));
        }
        let real = real(&*self.fs, path).map_err(unavailable)?;
        match SqliteStore::open_existing(&real).await {
            Ok(Some(store)) => Ok(Some(self.hold(path, false, store))),
            Ok(None) => Ok(None),
            Err(error) => {
                refuse_if_annals(&error, path)?;
                Err(error)
            }
        }
    }

    /// The change token of `project`'s store, read through the store held
    /// for it (or one opened read-only and held); `None` where there is no
    /// database.
    pub async fn change_token(&mut self, project: &Project) -> Result<Option<Token>, StoreError> {
        let Some(held) = self.hold_existing(project).await? else {
            return Ok(None);
        };
        Ok(Some(Token {
            handle: held.handle,
            change: held.store.change_token().await?,
        }))
    }

    /// The database at `store`, opened afresh and read-only, checked whole.
    /// Synchronous, as the project commands are, on a runtime of its own
    /// on a thread of its own, since its caller may be on one already.
    pub fn check(&mut self, store: &Path) -> Result<Checked, Report> {
        let store = &real(&*self.fs, store).map_err(|e| ported(&e.cause()))?;
        std::thread::scope(|scope| {
            let checking = scope.spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .map_err(|e| {
                        ported(
                            &StoreOpenError::NoRuntime {
                                detail: e.to_string(),
                            }
                            .cause(),
                        )
                    })?;
                runtime.block_on(check(store)).map_err(|e| report(&e))
            });
            checking
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }
}

/// The database at `path`, checked read-only; absent if there is none. A
/// file too damaged to open is damage, not a failure to check.
async fn check(path: &Path) -> Result<Checked, StoreError> {
    let damaged_file = SqlError::DamagedFile {
        errno: 0,
        detail: String::new(),
    }
    .code();
    let mut store = match SqliteStore::open_existing(path).await {
        Ok(Some(store)) => store,
        Ok(None) => return Ok(Checked::Absent),
        Err(StoreError::Unavailable(cause)) if cause.code == damaged_file => {
            return Ok(Checked::Damaged(ported(&cause)));
        }
        Err(error) => return Err(error),
    };
    Ok(match store.integrity_check().await? {
        Integrity::Intact => Checked::Intact,
        Integrity::Damaged(damage) => Checked::Damaged(ported(&angzarr_ports::Cause::new(
            damage.code(),
            damage.args(),
        ))),
    })
}

/// `error`, if it says the store at `path` is annals-store-sql's, refused
/// as a store from before angzarr-embedded (ruling D2), naming its backup.
fn refuse_if_annals(error: &StoreError, path: &Path) -> Result<(), StoreError> {
    let annals = SqlError::Unmigrated {
        pending: PENDING_SINCE_ANNALS as usize,
    }
    .cause();
    if !matches!(error, StoreError::Unavailable(cause) if *cause == annals) {
        return Ok(());
    }
    let mut backup = path.as_os_str().to_owned();
    backup.push(".pre-embedded");
    Err(unavailable(StoreOpenError::PredatesEmbedded {
        path: path.display().to_string(),
        backup: PathBuf::from(backup).display().to_string(),
    }))
}

/// `error` as angzarr-embedded's diagnostics report it: its code, and its
/// args with each cause nested.
fn report(error: &StoreError) -> Report {
    ported(&angzarr_ports::Cause::new(error.code(), error.args()))
}

/// angzarr-ports' cause as angzarr-embedded-diagnostic's report.
fn ported(cause: &angzarr_ports::Cause) -> Report {
    cause
        .args
        .iter()
        .fold(Report::new(cause.code), |report, (name, arg)| match arg {
            angzarr_ports::Arg::Uint(n) => report.arg(name, *n),
            angzarr_ports::Arg::Str(s) => report.arg(name, s.as_str()),
            angzarr_ports::Arg::Cause(c) => report.arg(
                name,
                angzarr_embedded_diagnostic::Arg::Nested(Box::new(ported(c))),
            ),
        })
}

/// Where the database at `path` is on disk, for SQLite to open.
fn real(fs: &dyn Fs, path: &Path) -> Result<PathBuf, StoreOpenError> {
    fs.real_path(path).map_err(|e| StoreOpenError::Unplaced {
        path: path.display().to_string(),
        detail: e.to_string(),
    })
}

/// Make the directory the database at `path` goes in, on `fs`.
fn create_parent(fs: &dyn Fs, path: &Path) -> Result<(), StoreOpenError> {
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    // As std's create_dir_all makes them: 0777 before the umask.
    fs.create_dir_all(dir, Mode::from_bits(0o777))
        .map_err(|e| StoreOpenError::CreateDir {
            path: dir.display().to_string(),
            detail: e.to_string(),
        })
}

fn unavailable(error: StoreOpenError) -> StoreError {
    StoreError::Unavailable(error.cause())
}

#[cfg(test)]
#[path = "embedded.test.rs"]
mod tests;
