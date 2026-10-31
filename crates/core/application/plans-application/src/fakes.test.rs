//! Fakes for the ports the plan service needs besides the ones `lib.test.rs`
//! writes (the work tree, the tasks, the mint): one project, one in-memory
//! store whose next write can be preceded by another process's, a clock, a
//! delay that waits for nothing. Every future is ready at once, so
//! [`ready`] runs a use case to its end without a runtime, and [`Service`]
//! runs the use cases as plain calls.

use std::cell::RefCell;
use std::future::Future;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use angzarr_embedded::next::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::next::v1::{Cover, EventPage, Snapshot};
use angzarr_embedded::next::{
    ChangeToken, EventStore, PositionKey, PositionStore, Purge, ReadStore, Slice, SnapshotStore,
    StoreError, StreamRef, Watch, Write, Written,
};
use angzarr_embedded_diagnostic::Report;
use ports::{
    Clock, CommandIds, Delay, Jitter, Mint, PlanFiles, Project, ProjectIdentity, TaskStores, Tasks,
    Timestamp, Uuid,
};

use crate::{
    CheckReport, ImportReport, Link, NewPlan, PlanError, PlanQuery, PlanService, PlanStatus,
    PlanText, PlanView, Plans,
};

/// Run a future whose every await is ready at once.
pub fn ready<F: Future>(future: F) -> F::Output {
    let mut cx = Context::from_waker(Waker::noop());
    match pin!(future).poll(&mut cx) {
        Poll::Ready(out) => out,
        Poll::Pending => panic!("the fakes' futures are ready at once"),
    }
}

pub fn project() -> Project {
    Project {
        id: "jolly-mute-magma".to_string(),
        root: Some("/tree".to_string()),
        store: PathBuf::from("/home/.ctxloom/projects/jolly-mute-magma/state.db"),
    }
}

/// One project, for a write or a read; the pins asked for are recorded.
#[derive(Default)]
pub struct FakeIdentity {
    pub pins: Vec<Option<String>>,
}

impl ProjectIdentity for FakeIdentity {
    fn for_write(&mut self, pinned: Option<&str>) -> Result<Project, Report> {
        self.pins.push(pinned.map(str::to_string));
        Ok(project())
    }

    fn for_read(&mut self, pinned: Option<&str>) -> Result<Option<Project>, Report> {
        self.pins.push(pinned.map(str::to_string));
        Ok(Some(project()))
    }

    fn every_project(&mut self) -> Result<Vec<Project>, Report> {
        Ok(vec![project()])
    }

    fn rootless_directory(&mut self) -> Result<Option<String>, Report> {
        Ok(None)
    }
}

/// What another process does to the store.
pub type Meanwhile = Box<dyn FnOnce(&MemoryStore) + Send>;

/// The in-memory store, whose next write is preceded, once, by what
/// another process does meanwhile: the race between a use case's read and
/// its write, forced. Writes are counted.
#[derive(Default)]
pub struct Store {
    pub inner: MemoryStore,
    meanwhile: Mutex<Option<Meanwhile>>,
    writes: Mutex<u32>,
    /// Writes still to fail, the store unable to say.
    failing: Mutex<u32>,
}

impl Store {
    /// Run `meanwhile` just before the next write lands.
    pub fn before_next_write(&self, meanwhile: Meanwhile) {
        *self.meanwhile.lock().unwrap() = Some(meanwhile);
    }

    /// Fail the next `writes` writes, the store unable to say.
    pub fn fail_writes(&self, writes: u32) {
        *self.failing.lock().unwrap() = writes;
    }

    /// Writes tried.
    pub fn writes(&self) -> u32 {
        *self.writes.lock().unwrap()
    }
}

impl EventStore for Store {
    async fn write(&self, write: Write<'_>) -> Result<Written, StoreError> {
        *self.writes.lock().unwrap() += 1;
        {
            let mut failing = self.failing.lock().unwrap();
            if *failing > 0 {
                *failing -= 1;
                return Err(StoreError::Unavailable(
                    angzarr_embedded::next::angzarr_ports::Cause::new("fake-busy", []),
                ));
            }
        }
        let meanwhile = self.meanwhile.lock().unwrap().take();
        if let Some(meanwhile) = meanwhile {
            meanwhile(&self.inner);
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
        command_id: angzarr_embedded::next::angzarr_ports::Uuid,
    ) -> Result<Vec<EventPage>, StoreError> {
        self.inner.find_command(stream, command_id).await
    }

    async fn covers(&self, domain: &str) -> Result<Vec<Cover>, StoreError> {
        self.inner.covers(domain).await
    }
}

impl SnapshotStore for Store {
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

impl PositionStore for Store {
    async fn position(&self, key: PositionKey<'_>) -> Result<u64, StoreError> {
        self.inner.position(key).await
    }
}

impl ReadStore for Store {
    type Reads<'s> = <MemoryStore as ReadStore>::Reads<'s>;

    async fn begin_read(&self) -> Result<Self::Reads<'_>, StoreError> {
        self.inner.begin_read().await
    }
}

impl Purge for Store {
    async fn purge(&self, stream: StreamRef<'_>, expected: u64) -> Result<u64, StoreError> {
        self.inner.purge(stream, expected).await
    }
}

impl Watch for Store {
    async fn change_token(&self) -> Result<ChangeToken, StoreError> {
        self.inner.change_token().await
    }
}

/// The one store, whichever project is asked for.
#[derive(Default)]
pub struct FakeStores {
    pub store: Store,
}

impl TaskStores for FakeStores {
    type Store = Store;

    async fn open(&mut self, _: &Project) -> Result<&Store, StoreError> {
        Ok(&self.store)
    }

    async fn open_existing(&mut self, _: &Project) -> Result<Option<&Store>, StoreError> {
        Ok(Some(&self.store))
    }
}

/// A clock one second on at each reading, and command ids numbered from 1.
#[derive(Default)]
pub struct FakeClock {
    pub seconds: i64,
    pub ids: u32,
}

impl Clock for FakeClock {
    fn now(&mut self) -> Timestamp {
        self.seconds += 1;
        Timestamp {
            seconds: 1_700_000_000 + self.seconds,
            nanos: 0,
        }
    }
}

impl CommandIds for FakeClock {
    fn command_id(&mut self) -> Uuid {
        self.ids += 1;
        let mut bytes = [0u8; 16];
        bytes[12..].copy_from_slice(&self.ids.to_be_bytes());
        Uuid::from_bytes(bytes)
    }
}

/// Waits recorded, never waited; no jitter.
#[derive(Default)]
pub struct FakeDelay {
    pub slept: Vec<Duration>,
}

impl Delay for FakeDelay {
    async fn sleep(&mut self, duration: Duration) {
        self.slept.push(duration);
    }
}

impl Jitter for FakeDelay {
    fn up_to(&mut self, ceiling: Duration) -> Duration {
        ceiling
    }
}

/// The service the tests run.
pub type Real<F, T, M> = PlanService<FakeIdentity, FakeStores, F, T, M, FakeClock, FakeDelay>;

/// The plan service over `files`, `tasks` and `mint` and the fakes above,
/// its use cases run to their end as plain calls, each answer unwrapped
/// where the use case's own answer is not a `Result`.
pub struct Service<F, T, M>(pub RefCell<Real<F, T, M>>);

impl<F: PlanFiles, T: Tasks, M: Mint> Service<F, T, M> {
    pub fn new(files: F, tasks: T, mint: M) -> Self {
        Service(RefCell::new(PlanService::new(
            FakeIdentity::default(),
            FakeStores::default(),
            files,
            tasks,
            mint,
            FakeClock::default(),
            FakeDelay::default(),
        )))
    }

    pub fn add(&mut self, new: NewPlan) -> Result<PlanView, PlanError> {
        ready(self.0.get_mut().add(new))
    }

    pub fn set_status(&mut self, plan: &str, status: PlanStatus) -> Result<PlanView, PlanError> {
        ready(self.0.get_mut().set_status(plan, status))
    }

    pub fn link(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        ready(self.0.get_mut().link(plan, link))
    }

    pub fn unlink(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        ready(self.0.get_mut().unlink(plan, link))
    }

    pub fn list(&self, query: &PlanQuery) -> Vec<PlanView> {
        ready(self.0.borrow_mut().list(query)).unwrap()
    }

    pub fn show(&self, id_or_path: &str) -> Result<PlanText, PlanError> {
        ready(self.0.borrow_mut().show(id_or_path))
    }

    pub fn check(&self) -> CheckReport {
        ready(self.0.borrow_mut().check()).unwrap()
    }

    pub fn import(&mut self, dry_run: bool) -> ImportReport {
        ready(self.0.get_mut().import(dry_run)).unwrap()
    }
}
