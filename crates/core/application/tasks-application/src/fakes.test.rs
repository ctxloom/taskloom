//! Fakes for the ports, and a store that fails on cue, for the use cases'
//! tests. Every future they return is ready at once, so [`ready`] runs a use
//! case to its end without a runtime.

use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use angzarr_embedded::next::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::next::v1::{Cover, EventPage, Snapshot, Uuid};
use angzarr_embedded::next::{
    ChangeToken, EventStore, Kernel, MemoryDeadLetters, PositionKey, PositionStore, Purge,
    ReadStore, ReadUnit, Sampled, Slice, SnapshotStore, StoreError, StreamRef, Watch, Write,
    Written,
};
use angzarr_embedded_diagnostic::Report;
use angzarr_ports::Cause;
use ports::{
    Clock, CommandIds, Delay, HeldToken, Jitter, Names, Project, ProjectIdentity, TaskStores,
    Timestamp, WatchedStores,
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
        root: Some("/work/tree".to_string()),
        store: PathBuf::from("/home/.ctxloom/projects/jolly-mute-magma/state.db"),
    }
}

/// A work root with a project, or none yet. A write binds one; a pin
/// resolves, as an explicit id always does; the pin a call gave is
/// recorded.
/// Every project is `every`; the working directory is `rootless` when
/// it lies in no work root.
#[derive(Default)]
pub struct FakeIdentity {
    pub bound: Option<Project>,
    /// Resolutions for a write: each may bind a project.
    pub writes: u32,
    pub pins: Vec<Option<String>>,
    pub fails: Option<Report>,
    pub every: Vec<Project>,
    pub rootless: Option<String>,
}

impl ProjectIdentity for FakeIdentity {
    fn for_write(&mut self, pinned: Option<&str>) -> Result<Project, Report> {
        self.pins.push(pinned.map(str::to_string));
        self.writes += 1;
        if let Some(report) = &self.fails {
            return Err(report.clone());
        }
        Ok(self.bound.get_or_insert_with(project).clone())
    }

    fn for_read(&mut self, pinned: Option<&str>) -> Result<Option<Project>, Report> {
        self.pins.push(pinned.map(str::to_string));
        if let Some(report) = &self.fails {
            return Err(report.clone());
        }
        Ok(self.bound.clone().or_else(|| pinned.map(|_| project())))
    }

    fn every_project(&mut self) -> Result<Vec<Project>, Report> {
        if let Some(report) = &self.fails {
            return Err(report.clone());
        }
        Ok(self.every.clone())
    }

    fn rootless_directory(&mut self) -> Result<Option<String>, Report> {
        if let Some(report) = &self.fails {
            return Err(report.clone());
        }
        Ok(self.rootless.clone())
    }
}

/// How one write (or purge) goes wrong.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// It fails and nothing lands: the store could not say.
    Unavailable,
    /// It lands, then reports failure: the answer was lost.
    LandsThenFails,
    /// It is refused: someone appended first.
    Conflict,
    /// It fails in a way no retry mends.
    Discontinuous,
    /// Nothing goes wrong: a write before the one meant to fail.
    Sound,
}

/// What another process does to a store.
pub type Meanwhile = Box<dyn FnOnce(&MemoryStore) + Send>;

/// Run `command` on `harp`'s stream in `store` directly, as another
/// process would: what a [`Meanwhile`] does to change a task.
pub fn run(store: &MemoryStore, harp: &str, command: tasks::TaskCommand) {
    let registry = crate::registry();
    let dead_letters = MemoryDeadLetters::new();
    let sampled = Sampled {
        now: Timestamp {
            seconds: 1_700_000_000 + 20 * 86_400,
            nanos: 0,
        },
        command_id: Uuid::from_bytes([0xee; 16]),
    };
    ready(
        Kernel::new(&registry, &dead_letters)
            .on_retryable(crate::KERNEL_RETRY)
            .handle::<tasks::TaskAggregate, _, _>(
                store,
                crate::book(harp, command),
                &sampled,
                &mut (),
            ),
    )
    .unwrap_or_else(|failure| panic!("{failure:?}"));
}

/// Purge `harp`'s stream in `store` directly, as another process would.
pub fn purge(store: &MemoryStore, harp: &str) -> u64 {
    let cover = crate::cover(harp);
    ready(Purge::purge(store, StreamRef::try_from(&cover).unwrap())).unwrap()
}

/// angzarr-ports' in-memory store, whose writes and purges fail on cue, in
/// order, with another process's doings run before the next attempt.
#[derive(Default)]
pub struct Flaky {
    pub inner: MemoryStore,
    faults: Mutex<VecDeque<Fault>>,
    /// Writes and purges tried.
    begun: AtomicU32,
    /// Fails the next open.
    pub open_fails: bool,
    /// Done to the store before its next stream read, write or purge: what
    /// another process does between a use case's earlier read (a listing)
    /// and the attempt that writes.
    meanwhile: Mutex<Option<Meanwhile>>,
    /// Every stream reads back with no events, outside a read-only unit and
    /// in one: what a stream purged mid-read can look like.
    pub hollow: bool,
    /// Read-only units begun: how often every stream was read.
    reads: AtomicU32,
    /// Done to the store just before its next purge lands: another
    /// process's purge and mint between a prune's read and its purge.
    before_purge: Mutex<Option<Meanwhile>>,
}

impl Flaky {
    pub fn failing(faults: &[Fault]) -> Flaky {
        let flaky = Flaky::default();
        flaky.fail(faults.iter().copied());
        flaky
    }

    /// The next writes and purges fail as `faults` say, in order.
    pub fn fail(&self, faults: impl IntoIterator<Item = Fault>) {
        *self.faults.lock().unwrap() = faults.into_iter().collect();
    }

    /// Writes and purges tried.
    pub fn begun(&self) -> u32 {
        self.begun.load(Ordering::SeqCst)
    }

    /// Read-only units begun.
    pub fn reads(&self) -> u32 {
        self.reads.load(Ordering::SeqCst)
    }

    /// Run `meanwhile` before the next attempt.
    pub fn set_meanwhile(&self, meanwhile: Meanwhile) {
        *self.meanwhile.lock().unwrap() = Some(meanwhile);
    }

    /// Run `meanwhile` just before the next purge lands.
    pub fn set_before_purge(&self, meanwhile: Meanwhile) {
        *self.before_purge.lock().unwrap() = Some(meanwhile);
    }

    /// Whether what was set to run meanwhile has run.
    pub fn meanwhile_ran(&self) -> bool {
        self.meanwhile.lock().unwrap().is_none()
    }

    fn meanwhile(&self) {
        let meanwhile = self.meanwhile.lock().unwrap().take();
        if let Some(meanwhile) = meanwhile {
            meanwhile(&self.inner);
        }
    }

    /// The stream `cover` names from `from`, as a reader outside any use
    /// case reads it: `None` when it holds no events.
    pub async fn read_from(&self, cover: &Cover, from: u64) -> Result<Option<Slice>, StoreError> {
        let stream = StreamRef::try_from(cover)?;
        let slice = self.inner.read(stream, from, None).await?;
        Ok((slice.next_sequence > 0).then_some(slice))
    }

    /// The next fault, counting the attempt.
    fn fault(&self) -> Option<Fault> {
        self.begun.fetch_add(1, Ordering::SeqCst);
        self.faults.lock().unwrap().pop_front()
    }
}

fn unavailable() -> StoreError {
    StoreError::Unavailable(Cause::new("fake-busy", []))
}

fn some_stream() -> StreamRef<'static> {
    StreamRef::main("tasks", angzarr_ports::Uuid::from_bytes([0; 16]))
}

/// `slice`, emptied of its events where the store reads hollow.
fn hollowed(hollow: bool, mut slice: Slice) -> Slice {
    if hollow {
        slice.pages.clear();
    }
    slice
}

impl EventStore for Flaky {
    async fn write(&self, write: Write<'_>) -> Result<Written, StoreError> {
        self.meanwhile();
        match self.fault() {
            None | Some(Fault::Sound) => self.inner.write(write).await,
            Some(Fault::Unavailable) => Err(unavailable()),
            Some(Fault::LandsThenFails) => {
                self.inner.write(write).await?;
                Err(unavailable())
            }
            Some(Fault::Conflict) => Err(StoreError::conflict(some_stream(), 0, 1)),
            Some(Fault::Discontinuous) => Err(StoreError::discontinuous(some_stream(), 0, 3)),
        }
    }

    async fn read(
        &self,
        stream: StreamRef<'_>,
        from: u64,
        to: Option<u64>,
    ) -> Result<Slice, StoreError> {
        self.meanwhile();
        let slice = self.inner.read(stream, from, to).await?;
        Ok(hollowed(self.hollow, slice))
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

impl SnapshotStore for Flaky {
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

impl PositionStore for Flaky {
    async fn position(&self, key: PositionKey<'_>) -> Result<u64, StoreError> {
        self.inner.position(key).await
    }
}

/// A read-only unit over a [`Flaky`] store: as hollow as the store is.
pub struct FlakyRead<'s> {
    inner: <MemoryStore as ReadStore>::Reads<'s>,
    hollow: bool,
}

impl ReadStore for Flaky {
    type Reads<'s> = FlakyRead<'s>;

    async fn begin_read(&self) -> Result<FlakyRead<'_>, StoreError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(FlakyRead {
            inner: self.inner.begin_read().await?,
            hollow: self.hollow,
        })
    }
}

impl ReadUnit for FlakyRead<'_> {
    async fn covers(&mut self, domain: &str) -> Result<Vec<Cover>, StoreError> {
        self.inner.covers(domain).await
    }

    async fn read(
        &mut self,
        stream: StreamRef<'_>,
        from: u64,
        to: Option<u64>,
    ) -> Result<Slice, StoreError> {
        let slice = self.inner.read(stream, from, to).await?;
        Ok(hollowed(self.hollow, slice))
    }

    async fn read_snapshot(
        &mut self,
        stream: StreamRef<'_>,
    ) -> Result<Option<Snapshot>, StoreError> {
        self.inner.read_snapshot(stream).await
    }
}

impl Purge for Flaky {
    async fn purge(&self, stream: StreamRef<'_>) -> Result<u64, StoreError> {
        self.meanwhile();
        let before = self.before_purge.lock().unwrap().take();
        if let Some(before) = before {
            before(&self.inner);
        }
        match self.fault() {
            None | Some(Fault::Sound) => self.inner.purge(stream).await,
            Some(Fault::Unavailable) => Err(unavailable()),
            Some(Fault::LandsThenFails) => {
                self.inner.purge(stream).await?;
                Err(unavailable())
            }
            Some(Fault::Conflict) => Err(StoreError::conflict(some_stream(), 0, 1)),
            Some(Fault::Discontinuous) => Err(StoreError::discontinuous(some_stream(), 0, 3)),
        }
    }
}

impl Watch for Flaky {
    async fn change_token(&self) -> Result<ChangeToken, StoreError> {
        self.inner.change_token().await
    }
}

/// One store, whichever project is asked for, but a project in `others`,
/// which has its own: absent until a write opens it, if `absent`. The
/// projects opened to write (`opened`) and to read (`looked`) are
/// recorded. Its tokens are the in-memory store's under `handle`, which a
/// test changes to stand for the store opened again; `before_token` is
/// done to the store as a token is next asked for, before it is read.
#[derive(Default)]
pub struct FakeStores {
    pub store: Flaky,
    pub absent: bool,
    pub opened: Vec<String>,
    pub looked: Vec<String>,
    pub others: std::collections::BTreeMap<String, Flaky>,
    pub handle: u64,
    pub before_token: Option<Meanwhile>,
}

impl WatchedStores for FakeStores {
    async fn change_token(&mut self, project: &Project) -> Result<Option<HeldToken>, StoreError> {
        let handle = self.handle;
        let before = self.before_token.take();
        let Some(store) = self.open_existing(project).await? else {
            return Ok(None);
        };
        if let Some(meanwhile) = before {
            meanwhile(&store.inner);
        }
        let change = Watch::change_token(&store.inner).await?;
        Ok(Some(HeldToken { handle, change }))
    }
}

impl TaskStores for FakeStores {
    type Store = Flaky;

    async fn open(&mut self, project: &Project) -> Result<&Flaky, StoreError> {
        self.opened.push(project.id.clone());
        if self.store.open_fails {
            return Err(unavailable());
        }
        if let Some(other) = self.others.get(&project.id) {
            return Ok(other);
        }
        self.absent = false;
        Ok(&self.store)
    }

    async fn open_existing(&mut self, project: &Project) -> Result<Option<&Flaky>, StoreError> {
        self.looked.push(project.id.clone());
        if self.store.open_fails {
            return Err(unavailable());
        }
        if let Some(other) = self.others.get(&project.id) {
            return Ok(Some(other));
        }
        Ok((!self.absent).then_some(&self.store))
    }
}

/// Task ids in the order given; project ids are never asked for here.
#[derive(Default)]
pub struct FakeNames {
    pub task_ids: VecDeque<String>,
    /// Given forever once `task_ids` runs out, if set.
    pub then: Option<String>,
    pub minted: u32,
}

impl FakeNames {
    pub fn of(ids: &[&str]) -> FakeNames {
        FakeNames {
            task_ids: ids.iter().map(|s| s.to_string()).collect(),
            ..FakeNames::default()
        }
    }
}

impl Names for FakeNames {
    fn task_id(&mut self) -> String {
        self.minted += 1;
        self.task_ids
            .pop_front()
            .or_else(|| self.then.clone())
            .expect("a task id scripted")
    }

    fn project_id(&mut self) -> String {
        unreachable!("the application mints no project id")
    }
}

/// A clock one second on at each reading, and command ids numbered from 1.
#[derive(Default)]
pub struct FakeClock {
    pub seconds: i64,
    pub ids: Vec<Uuid>,
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
        let n = u8::try_from(self.ids.len() + 1).expect("few ids");
        let id = Uuid::from_bytes([n; 16]);
        self.ids.push(id.clone());
        id
    }
}

/// Waits recorded, never waited; jitter is a fixed fraction (in
/// thousandths) of the ceiling, or a fixed duration when `over` is set.
pub struct FakeDelay {
    pub slept: Vec<Duration>,
    pub ceilings: Vec<Duration>,
    pub permille: u32,
    pub over: Option<Duration>,
}

impl Default for FakeDelay {
    fn default() -> Self {
        FakeDelay {
            slept: Vec::new(),
            ceilings: Vec::new(),
            permille: 1000,
            over: None,
        }
    }
}

impl Delay for FakeDelay {
    async fn sleep(&mut self, duration: Duration) {
        self.slept.push(duration);
    }
}

impl Jitter for FakeDelay {
    fn up_to(&mut self, ceiling: Duration) -> Duration {
        self.ceilings.push(ceiling);
        self.over.unwrap_or(ceiling * self.permille / 1000)
    }
}
