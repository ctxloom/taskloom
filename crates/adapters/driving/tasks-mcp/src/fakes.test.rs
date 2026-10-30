//! The real use cases over fake ports, for the server's tests: a store
//! whose first commits conflict, kept from one call's service to the
//! next; a wait between retries that can be held open; and an identity
//! that records the thread it was asked on.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::Duration;

use angzarr_embedded_diagnostic::Report;
use angzarr_ports::angzarr_proto::v1::{Cover, EventPage, Snapshot};
use angzarr_ports::memory::MemoryStore;
use angzarr_ports::{
    ChangeToken, EventStore, PositionKey, PositionStore, Purge, ReadStore, Slice, SnapshotStore,
    StoreError, StreamRef, Watch, Write, Written,
};
use ports::{
    Clock, CommandIds, Delay, Jitter, Names, Project, ProjectIdentity, TaskStores, Timestamp, Uuid,
};
use tasks_application::Service;
use tokio::sync::oneshot;

pub fn project() -> Project {
    Project {
        id: "jolly-mute-magma".to_string(),
        root: Some("/work/tree".to_string()),
        store: PathBuf::from("/home/.ctxloom/projects/jolly-mute-magma/state.db"),
    }
}

/// A work root with a project, or none; or no work root at all, in
/// `rootless`, the one project there is found only by reading every
/// project. Every thread it was asked on is recorded.
pub struct FakeIdentity {
    pub bound: bool,
    pub rootless: Option<String>,
    pub asked_on: Arc<Mutex<Vec<ThreadId>>>,
}

impl FakeIdentity {
    fn asked(&self) {
        self.asked_on
            .lock()
            .unwrap()
            .push(std::thread::current().id());
    }
}

impl ProjectIdentity for FakeIdentity {
    fn for_write(&mut self, _pinned: Option<&str>) -> Result<Project, Report> {
        self.asked();
        Ok(project())
    }

    fn for_read(&mut self, _pinned: Option<&str>) -> Result<Option<Project>, Report> {
        self.asked();
        Ok(self.bound.then(project))
    }

    fn every_project(&mut self) -> Result<Vec<Project>, Report> {
        self.asked();
        Ok(vec![project()])
    }

    fn rootless_directory(&mut self) -> Result<Option<String>, Report> {
        self.asked();
        Ok(self.rootless.clone())
    }
}

/// An in-memory store whose first `conflicts` writes are refused, as when
/// another writer appended first.
pub struct Contended {
    inner: MemoryStore,
    conflicts: AtomicU32,
}

impl EventStore for Contended {
    async fn write(&self, write: Write<'_>) -> Result<Written, StoreError> {
        let left = self.conflicts.load(Ordering::SeqCst);
        if left > 0 {
            self.conflicts.store(left - 1, Ordering::SeqCst);
            let stream = StreamRef::main("tasks", angzarr_ports::Uuid::from_bytes([0; 16]));
            return Err(StoreError::conflict(stream, 0, 1));
        }
        self.inner.write(write).await
    }

    async fn read(
        &self,
        stream: StreamRef<'_>,
        from: u64,
        to: Option<u64>,
    ) -> Result<Slice, StoreError> {
        self.inner.read(stream, from, to).await
    }

    async fn find_command(
        &self,
        stream: StreamRef<'_>,
        command_id: angzarr_ports::Uuid,
    ) -> Result<Vec<EventPage>, StoreError> {
        self.inner.find_command(stream, command_id).await
    }

    async fn covers(&self, domain: &str) -> Result<Vec<Cover>, StoreError> {
        self.inner.covers(domain).await
    }
}

impl SnapshotStore for Contended {
    async fn read_snapshot(&self, stream: StreamRef<'_>) -> Result<Option<Snapshot>, StoreError> {
        self.inner.read_snapshot(stream).await
    }

    async fn write_snapshot(
        &self,
        stream: StreamRef<'_>,
        snapshot: &Snapshot,
    ) -> Result<(), StoreError> {
        self.inner.write_snapshot(stream, snapshot).await
    }
}

impl PositionStore for Contended {
    async fn position(&self, key: PositionKey<'_>) -> Result<u64, StoreError> {
        self.inner.position(key).await
    }
}

impl ReadStore for Contended {
    type Reads<'s> = <MemoryStore as ReadStore>::Reads<'s>;

    async fn begin_read(&self) -> Result<Self::Reads<'_>, StoreError> {
        self.inner.begin_read().await
    }
}

impl Purge for Contended {
    async fn purge(&self, stream: StreamRef<'_>, expected: u64) -> Result<u64, StoreError> {
        self.inner.purge(stream, expected).await
    }
}

impl Watch for Contended {
    async fn change_token(&self) -> Result<ChangeToken, StoreError> {
        self.inner.change_token().await
    }
}

/// One store, made on a write's open; none for a read before one. It is
/// taken from `home` when the service is made and put back when it is
/// dropped, so the next call's service finds what this one wrote.
pub struct FakeStores {
    store: Option<Contended>,
    conflicts: u32,
    home: Arc<Mutex<Option<Contended>>>,
}

impl Drop for FakeStores {
    fn drop(&mut self) {
        if let Some(store) = self.store.take() {
            *self.home.lock().unwrap() = Some(store);
        }
    }
}

impl TaskStores for FakeStores {
    type Store = Contended;

    async fn open(&mut self, _project: &Project) -> Result<&Contended, StoreError> {
        let conflicts = self.conflicts;
        Ok(self.store.get_or_insert_with(|| Contended {
            inner: MemoryStore::new(),
            conflicts: AtomicU32::new(conflicts),
        }))
    }

    async fn open_existing(
        &mut self,
        _project: &Project,
    ) -> Result<Option<&Contended>, StoreError> {
        Ok(self.store.as_ref())
    }
}

/// Harps minted in turn across every call's service: `swift-amber`,
/// then `swift-amber-2`, and so on.
pub struct FakeNames {
    minted: Arc<AtomicU32>,
}

impl Names for FakeNames {
    fn task_id(&mut self) -> String {
        match self.minted.fetch_add(1, Ordering::Relaxed) {
            0 => "swift-amber".to_string(),
            n => format!("swift-amber-{}", n + 1),
        }
    }

    fn project_id(&mut self) -> String {
        unreachable!("the application mints no project id")
    }
}

/// A fixed instant, and command ids unique across every call's service,
/// as the system's are: a command id seen again is a redelivery.
pub struct FakeClock {
    ids: Arc<AtomicU32>,
}

impl Clock for FakeClock {
    fn now(&mut self) -> Timestamp {
        Timestamp {
            seconds: 1_781_058_920,
            nanos: 837_009_562,
        }
    }
}

impl CommandIds for FakeClock {
    fn command_id(&mut self) -> Uuid {
        let id = self.ids.fetch_add(1, Ordering::Relaxed) + 1;
        let mut bytes = [0; 16];
        bytes[..4].copy_from_slice(&id.to_be_bytes());
        Uuid::from_bytes(bytes)
    }
}

/// The wait before a retry, held open when `hold` is set: it says it has
/// begun on `hold.0`, then waits for `hold.1`.
#[derive(Default)]
pub struct HeldDelay {
    pub hold: Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>,
}

impl Delay for HeldDelay {
    async fn sleep(&mut self, _duration: Duration) {
        if let Some((begun, release)) = self.hold.take() {
            let _ = begun.send(());
            let _ = release.await;
        }
    }
}

impl Jitter for HeldDelay {
    fn up_to(&mut self, ceiling: Duration) -> Duration {
        ceiling
    }
}

pub type FakeService = Service<FakeIdentity, FakeStores, FakeNames, FakeClock, HeldDelay>;

/// What each call's service is made with. The first service made takes
/// the hold, if one is set.
#[derive(Default)]
pub struct World {
    pub bound: bool,
    pub rootless: Option<String>,
    pub conflicts: u32,
    pub store: Arc<Mutex<Option<Contended>>>,
    pub hold: Mutex<Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>>,
    pub asked_on: Arc<Mutex<Vec<ThreadId>>>,
    pub made: Mutex<u32>,
    pub minted: Arc<AtomicU32>,
    pub ids: Arc<AtomicU32>,
}

impl World {
    /// A fresh service, as the composition root makes one per call.
    pub fn service(&self) -> FakeService {
        *self.made.lock().unwrap() += 1;
        let identity = FakeIdentity {
            bound: self.bound,
            rootless: self.rootless.clone(),
            asked_on: Arc::clone(&self.asked_on),
        };
        let stores = FakeStores {
            store: self.store.lock().unwrap().take(),
            conflicts: self.conflicts,
            home: Arc::clone(&self.store),
        };
        let delay = HeldDelay {
            hold: self.hold.lock().unwrap().take(),
        };
        let names = FakeNames {
            minted: Arc::clone(&self.minted),
        };
        let clock = FakeClock {
            ids: Arc::clone(&self.ids),
        };
        Service::new(identity, stores, names, clock, delay)
    }
}
