//! A project's task store: angzarr-store-sqlite's [`SqliteStore`] at the
//! path the project names (`~/.ctxloom/projects/<project-id>/state.db`),
//! opened on first use and held after, behind the [`TaskStores`] port.
//!
//! - The path is the caller's, on the [`Fs`] the stores are given: its
//!   directory is made through the seam, and SQLite opens the file where
//!   the seam says it is on disk ([`Fs::real_path`]), so a re-rooted
//!   filesystem holds the database (and its write-ahead log) too.
//! - [`SqliteStores::open`] opens it to write, creating its directory and
//!   the file ([`SqliteStore::open`]); `open_existing` opens it read-only
//!   ([`SqliteStore::open_existing`]) and never creates it, finding no
//!   store where there is no file. Each holds the store it opened, for its
//!   project, until another is asked for; a store held to write serves a
//!   read.
//! - Both migrate a store older than the build: angzarr-store-sqlite's
//!   every open does (the owner's ruling, 2026-10-31: both read and write
//!   migrate on startup), through angzarr-ports' `Migrations`, backing the
//!   store up beside itself first through its `Backup`
//!   (`state.db.pre-<applied level>-<UTC time>`), and refusing, untouched,
//!   a store newer than the build or one a migration failed or changed in
//!   (`angzarr-ports-store-newer-than-build`, `-migration-failed`,
//!   `-migration-changed`). ctxloom writes no SQL, and takes no backup of
//!   its own.
//! - A store an older generation wrote (its event pages in an angzarr wire
//!   this build does not decode) is an archive: both opens and the check
//!   refuse it, keyed [`StoreOpenError::OlderGeneration`] with its `path` and
//!   the `archive` name to move it to, before anything is opened, so
//!   nothing is migrated, backed up or made beside it, and the file and its
//!   directory are left byte for byte as they were.
//! - A store written before angzarr-embedded (annals-store-sql's, its
//!   ledger only its first migration) is refused by both, keyed
//!   [`StoreOpenError::PredatesEmbedded`] with its `path` and the
//!   `backup` to move it to (`state.db.pre-embedded`), and left exactly
//!   as it was: never migrated in place (the owner's ruling D2). Its
//!   ledger is read before either open ([`SqliteStore::ledger_at`]), which
//!   migrates nothing.
//! - The stores take `&self` (angzarr-ports'), so the opener lends the
//!   store shared: a purge is the store's own [`Purge`](angzarr_ports::Purge),
//!   and a watch is its own [`Watch`] token, paired with which opening of
//!   the store answered it ([`WatchedStores`]), since tokens compare only
//!   from one connection.
//! - [`StoreCheck`] checks a database whole
//!   ([`SqliteStore::integrity_check`]), opened afresh and read-only.
//!   Damage is a key and its args, never English: the store's
//!   `angzarr-store-sqlite-damaged` (its `first` problem and how many
//!   `more`), or `angzarr-store-sqlite-damaged-file` for a file too damaged
//!   to open (SQLite's `errno`, and its own text as `ext_err_txt`). A
//!   failure to run the check is the store's error, a third party's text
//!   as `ext_err_txt` there too. No database is no damage.
//! - Errors are angzarr-ports' [`StoreError`]; this crate's own refusals
//!   travel in it as a cause, its code and args.

mod error;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use angzarr_diagnostic::Report;
use angzarr_embedded::angzarr_store_sqlite::{Integrity, SqlError, SqliteStore};
use angzarr_ports::angzarr_proto::v1::EventPage;
use angzarr_ports::{StoreError, Watch};
use file_system::Fs;
use os::Mode;
use ports::{Checked, HeldToken, Project, StoreCheck, TaskStores, WatchedStores};
use prost::Message;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};

#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{StoreOpenError, EN_US};

/// The SQLite store of the project last asked for, held until another is.
pub struct SqliteStores {
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
    /// Which opening this is, for its tokens ([`HeldToken::handle`]).
    handle: u64,
    store: SqliteStore,
}

impl SqliteStores {
    /// The stores on `fs`, none opened yet.
    pub fn new(fs: Arc<dyn Fs>) -> SqliteStores {
        SqliteStores {
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

    /// The store held for `project`, or the one at its path opened
    /// read-only (and migrated if need be) and held; `None` if there is
    /// none.
    async fn hold_existing(&mut self, project: &Project) -> Result<Option<&mut Held>, StoreError> {
        let path = &project.store;
        if let Some(held) = self.held.take().filter(|h| &h.path == path) {
            return Ok(Some(self.held.insert(held)));
        }
        let real = real(&*self.fs, path).map_err(unavailable)?;
        refuse_if_unusable(&*self.fs, &real, path)
            .await
            .map_err(unavailable)?;
        Ok(SqliteStore::open_existing(&real)
            .await?
            .map(|store| self.hold(path, false, store)))
    }
}

impl TaskStores for SqliteStores {
    type Store = SqliteStore;

    /// The store at `project`'s path, opened to write: its directory and
    /// file created and migrated if need be. Held after.
    async fn open(&mut self, project: &Project) -> Result<&SqliteStore, StoreError> {
        let path = &project.store;
        let held = self.held.take().filter(|h| &h.path == path && h.writable);
        if let Some(held) = held {
            return Ok(&self.held.insert(held).store);
        }
        create_parent(&*self.fs, path).map_err(unavailable)?;
        let real = real(&*self.fs, path).map_err(unavailable)?;
        refuse_if_unusable(&*self.fs, &real, path)
            .await
            .map_err(unavailable)?;
        let store = SqliteStore::open(&real).await?;
        Ok(&self.hold(path, true, store).store)
    }

    /// The store at `project`'s path, held or opened read-only; `None` if
    /// there is no file there, and nothing is made. One with migrations
    /// pending is backed up and migrated by the open.
    async fn open_existing(
        &mut self,
        project: &Project,
    ) -> Result<Option<&SqliteStore>, StoreError> {
        Ok(self.hold_existing(project).await?.map(|held| &held.store))
    }
}

impl WatchedStores for SqliteStores {
    /// The change token of `project`'s store, read through the store held
    /// for it (or one opened read-only and held); `None` where there is no
    /// database.
    async fn change_token(&mut self, project: &Project) -> Result<Option<HeldToken>, StoreError> {
        let Some(held) = self.hold_existing(project).await? else {
            return Ok(None);
        };
        Ok(Some(HeldToken {
            handle: held.handle,
            change: held.store.change_token().await?,
        }))
    }
}

impl StoreCheck for SqliteStores {
    /// The database at `store`, opened afresh and read-only, checked whole.
    /// Synchronous, as the project commands are, on a runtime of its own
    /// on a thread of its own, since its caller may be on one already.
    fn check(&mut self, logical: &Path) -> Result<Checked, Report> {
        let fs = &*self.fs;
        let store = &real(fs, logical).map_err(|e| ported(&e.cause()))?;
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
                runtime.block_on(async {
                    // An older generation's store is an archive: no open of
                    // it, however read-only, is made.
                    refuse_if_unusable(fs, store, logical)
                        .await
                        .map_err(|e| ported(&e.cause()))?;
                    check(store).await.map_err(|e| report(&e))
                })
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

/// The store at `real` (`path` as the caller names it), judged before any
/// open, which would migrate it, back it up or make files beside it, and
/// refused, untouched, if this build cannot use it:
///
/// - annals-store-sql's, a store from before angzarr-embedded (ruling D2),
///   naming its backup: its ledger holds only the first migration, which
///   annals-store-sql's first is blob-identical to;
/// - one an older generation wrote: its first event page does not decode
///   as this build's `EventPage` ([`StoreOpenError::OlderGeneration`]).
///   The migrations are the same for both generations, so the ledger is no
///   mark of it; the page is: `payload` was field 3 (length-delimited),
///   which is `sequence` (a varint) now, so every page the older wire wrote
///   fails to decode, and this build's own never do.
///
/// The store is read once, on a connection that changes nothing, without
/// the library's open: `immutable`, so SQLite makes no `-wal` or `-shm`
/// beside a store in WAL mode and takes no lock, while no log holds
/// anything (a store closed cleanly has none). Where a log holds
/// commits, it is read as SQLite reads it (the files exist, so none is
/// made). What cannot be read at all (no database, damage) is the
/// store's own to refuse when it is opened.
async fn refuse_if_unusable(fs: &dyn Fs, real: &Path, path: &Path) -> Result<(), StoreOpenError> {
    if !fs.exists(path).unwrap_or(true) {
        return Ok(());
    }
    let logged = fs
        .metadata(&sibling(path, "-wal"))
        .is_ok_and(|log| log.len > 0);
    let Some(Probed { versions, page }) = probe(real, !logged).await else {
        return Ok(());
    };
    if versions == [1] {
        return Err(StoreOpenError::PredatesEmbedded {
            path: path.display().to_string(),
            backup: sibling(path, ".pre-embedded").display().to_string(),
        });
    }
    if page.is_some_and(|bytes| EventPage::decode(bytes.as_slice()).is_err()) {
        return Err(StoreOpenError::OlderGeneration {
            path: path.display().to_string(),
            archive: sibling(path, ".archive-<date>").display().to_string(),
        });
    }
    Ok(())
}

/// `path` with `suffix` appended to its file name.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// What the store file holds that the judgement reads.
struct Probed {
    /// The ledger's versions, in order (none if it has no ledger).
    versions: Vec<i64>,
    /// The first event page's bytes; none if no event is stored.
    page: Option<Vec<u8>>,
}

/// The ledger and an event page of the database at `real`, read-only; with
/// `immutable` it is read as the file stands, touching nothing beside it.
/// `None` if it cannot be opened or read at all.
async fn probe(real: &Path, immutable: bool) -> Option<Probed> {
    let mut conn = SqliteConnectOptions::new()
        .filename(real)
        .read_only(true)
        .immutable(immutable)
        .connect()
        .await
        .ok()?;
    // A table that is not there, or a file that is no database, answers
    // nothing: it is the store's to refuse when it is opened.
    let versions = sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
        .fetch_all(&mut conn)
        .await
        .unwrap_or_default();
    let page = sqlx::query_scalar("SELECT page FROM events LIMIT 1")
        .fetch_optional(&mut conn)
        .await
        .ok()
        .flatten();
    let _ = conn.close().await;
    Some(Probed { versions, page })
}

/// `error` as angzarr-embedded's diagnostics report it: its code, and its
/// args with each cause nested.
fn report(error: &StoreError) -> Report {
    ported(&angzarr_ports::Cause::new(error.code(), error.args()))
}

/// angzarr-ports' cause as angzarr-diagnostic's report.
fn ported(cause: &angzarr_ports::Cause) -> Report {
    cause
        .args
        .iter()
        .fold(Report::new(cause.code), |report, (name, arg)| match arg {
            angzarr_ports::Arg::Uint(n) => report.arg(*name, *n),
            angzarr_ports::Arg::Str(s) => report.arg(*name, s.as_str()),
            angzarr_ports::Arg::Cause(c) => {
                report.arg(*name, angzarr_diagnostic::Arg::Nested(Box::new(ported(c))))
            }
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
#[path = "lib.test.rs"]
mod tests;
