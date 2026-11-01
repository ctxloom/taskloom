//! Fakes for the ports the plan service needs besides the ones `lib.test.rs`
//! writes (the work tree, the tasks, the mint): one project, one in-memory
//! store whose next write can be preceded by another process's, a clock, a
//! delay that waits for nothing. Every future is ready at once, so
//! [`ready`] runs a use case to its end without a runtime, and [`Service`]
//! runs the use cases as plain calls.

use std::cell::RefCell;
use std::path::PathBuf;

use angzarr_diagnostic::Report;
use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::testing::FaultyStore;
use angzarr_embedded::StoreError;
use ports::{Mint, PlanFiles, Project, ProjectIdentity, TaskStores, Tasks};

use crate::{
    CheckReport, ImportReport, Link, NewPlan, PlanError, PlanQuery, PlanService, PlanStatus,
    PlanText, PlanView, Plans,
};

/// Run a future whose every await is ready at once: the test support's.
pub use angzarr_embedded::testing::block_on_ready as ready;
/// How a write goes wrong, and where another process's doings run:
/// angzarr-ports' faulty store's.
pub use angzarr_embedded::testing::{At, Fault};
/// The clock, ids and delay: the ports' stand-ins.
pub use ports::testing::{FakeClock, FakeDelay};

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

/// The in-memory store, failing what a test says: its next writes
/// (`fail`), and what another process does just before the next write
/// (`before(At::Write, ..)`), the race between a use case's read and its
/// write, forced. Its writes are recorded (`writes`).
pub type Store = FaultyStore<MemoryStore>;

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
