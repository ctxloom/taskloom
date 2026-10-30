//! A project's task store on angzarr-store-sqlite's [`SqliteStore`] (step
//! 3b of taskloom-on-angzarr-embedded.plan.md), beside the annals one
//! ([`crate::SqliteStores`]) until the application moves (step 4), when it
//! replaces it behind the ports.
//!
//! RED: every body is `todo!()`. The proposed shape, the annals opener's
//! behaviour on the new store:
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

use std::path::Path;
use std::sync::Arc;

use angzarr_embedded_diagnostic::Report;
use angzarr_ports::{ChangeToken, StoreError};
use angzarr_store_sqlite::SqliteStore;
use file_system::Fs;
use ports::Project;

/// The SQLite store of the project last asked for, held until another is.
pub struct EmbeddedStores {}

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
        let _ = fs;
        todo!("3b: the stores on a filesystem")
    }

    /// The store at `project`'s path, opened to write: its directory and
    /// file created and migrated if need be. Held after.
    pub async fn open(&mut self, project: &Project) -> Result<&SqliteStore, StoreError> {
        let _ = project;
        todo!("3b: open a project's store to write")
    }

    /// The store at `project`'s path, held or opened read-only; `None` if
    /// there is no file there, and nothing is made.
    pub async fn open_existing(
        &mut self,
        project: &Project,
    ) -> Result<Option<&SqliteStore>, StoreError> {
        let _ = project;
        todo!("3b: open a project's store to read")
    }

    /// The change token of `project`'s store, read through the store held
    /// for it (or one opened read-only and held); `None` where there is no
    /// database.
    pub async fn change_token(&mut self, project: &Project) -> Result<Option<Token>, StoreError> {
        let _ = project;
        todo!("3b: a project's store's change token")
    }

    /// The database at `store`, opened afresh and read-only, checked whole.
    /// Synchronous, as the project commands are, on a runtime of its own,
    /// since its caller may be on one already.
    pub fn check(&mut self, store: &Path) -> Result<Checked, Report> {
        let _ = store;
        todo!("3b: check a database for damage")
    }
}

#[cfg(test)]
#[path = "embedded.test.rs"]
mod tests;
