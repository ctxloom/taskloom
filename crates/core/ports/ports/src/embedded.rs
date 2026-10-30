//! The ports on angzarr's types (step 4 of
//! taskloom-on-angzarr-embedded.plan.md), beside annals' until the
//! application moves, when they replace them under the same names.
//!
//! What changes: a store is angzarr-ports' (every base port, [`ReadStore`]'s
//! one-moment reads, [`Purge`] and [`Watch`]), lent shared, since its ports
//! take `&self`; there is no unit of work, so no purging unit either (a
//! purge is the store's own call); a store's error is angzarr-ports'
//! [`StoreError`]; a command id is angzarr's [`Uuid`]. The rest of the
//! ports (identity, names, the clock's instant, waiting, configuration)
//! keep their shapes.

use std::future::Future;

use angzarr_ports::angzarr_proto::v1::Uuid;
use angzarr_ports::{ChangeToken, Purge, ReadStore, Store, StoreError, Watch};

use crate::Project;

/// A project's store, opened where its project says.
pub trait TaskStores {
    /// A project's store: every base port, one-moment reads, a purge and a
    /// change token.
    type Store: Store + ReadStore + Purge + Watch + Send + Sync;

    /// The project's store, to write: opened (and created) on first use and
    /// held after.
    fn open(
        &mut self,
        project: &Project,
    ) -> impl Future<Output = Result<&Self::Store, StoreError>> + Send;

    /// The project's store, to read: `None` if it has none yet, and
    /// nothing is created. A store already held for a write serves it too.
    fn open_existing(
        &mut self,
        project: &Project,
    ) -> impl Future<Output = Result<Option<&Self::Store>, StoreError>> + Send;
}

/// What a project's store answers a watcher: angzarr-ports'
/// [`ChangeToken`], and which handle on the store answered it, since
/// tokens compare only from one handle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct HeldToken {
    /// The handle that answered: a new one each time a store is opened.
    pub handle: u64,
    /// What that handle answered.
    pub change: ChangeToken,
}

/// Task stores whose stores say cheaply whether anything was committed.
pub trait WatchedStores: TaskStores {
    /// The token of `project`'s store, read through the store held for it
    /// or one opened as [`TaskStores::open_existing`] opens one and held
    /// after; `None` while the project has no store.
    fn change_token(
        &mut self,
        project: &Project,
    ) -> impl Future<Output = Result<Option<HeldToken>, StoreError>> + Send;
}

/// Fresh command ids, and list instances: UUIDv7 (RFC 9562), as
/// angzarr-embedded expects of an application.
pub trait CommandIds {
    fn command_id(&mut self) -> Uuid;
}
