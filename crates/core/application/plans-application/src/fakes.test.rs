//! Fakes for the ports the plans tests need: the plan files in memory, the
//! tasks and rules, the mint and the configuration; and, for the `Tasks`
//! port over the real tasks use cases, one project and one in-memory store.
//! Every future is ready at once, so [`ready`] runs a use case to its end
//! without a runtime.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use angzarr_diagnostic::Report;
use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::testing::FaultyStore;
use angzarr_embedded::StoreError;
use ports::{Configuration, Mint, PlanFiles, Project, ProjectIdentity, TaskStores, Tasks};

/// Run a future whose every await is ready at once: the test support's.
pub use angzarr_embedded::testing::block_on_ready as ready;
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

/// The in-memory store.
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

/// The plan files, in memory: a map of absolute paths to text, and the
/// work root `/tree`.
#[derive(Default)]
pub struct MemFiles {
    pub files: RefCell<BTreeMap<String, String>>,
    /// Paths whose next replace fails.
    pub deny: RefCell<Vec<String>>,
}

impl MemFiles {
    pub fn with(files: &[(&str, &str)]) -> MemFiles {
        MemFiles {
            files: RefCell::new(
                files
                    .iter()
                    .map(|(p, t)| (p.to_string(), t.to_string()))
                    .collect(),
            ),
            deny: RefCell::default(),
        }
    }

    pub fn text(&self, path: &str) -> String {
        self.files.borrow()[path].clone()
    }
}

impl PlanFiles for MemFiles {
    fn work_root(&self) -> Result<String, Report> {
        Ok("/tree".to_string())
    }

    fn read(&self, path: &str) -> Option<String> {
        self.files.borrow().get(path).cloned()
    }

    fn replace(&self, path: &str, text: &str) -> Result<(), Report> {
        if self.deny.borrow().iter().any(|d| d == path) {
            return Err(Report::new("x-denied"));
        }
        self.files
            .borrow_mut()
            .insert(path.to_string(), text.to_string());
        Ok(())
    }

    fn create(&self, path: &str, text: &str) -> Result<bool, Report> {
        let mut files = self.files.borrow_mut();
        if files.contains_key(path) {
            return Ok(false);
        }
        files.insert(path.to_string(), text.to_string());
        Ok(true)
    }

    fn scan(&self, root: &str) -> Vec<String> {
        let prefix = format!("{}/", root.trim_end_matches('/'));
        self.files
            .borrow()
            .keys()
            .filter(|p| p.starts_with(&prefix) && p.ends_with(".plan.md"))
            .cloned()
            .collect()
    }
}

/// The project's tasks and rules: the ids it holds, and the rules it
/// states (the seed's where none).
#[derive(Default)]
pub struct MemTasks {
    pub tasks: Vec<&'static str>,
    pub rules: Option<Vec<String>>,
    /// How many times the rules were asked for.
    pub asked: usize,
}

impl Tasks for MemTasks {
    async fn exists(&mut self, harp_id: &str) -> Result<bool, Report> {
        Ok(self.tasks.contains(&harp_id))
    }

    async fn rules(&mut self) -> Result<Vec<String>, Report> {
        self.asked += 1;
        Ok(self
            .rules
            .clone()
            .unwrap_or_else(|| tag_rules::StoreRules::default().export()))
    }
}

/// Ids in the order given, then `last-one-left` for ever.
pub struct Words(pub RefCell<Vec<&'static str>>);

impl Mint for Words {
    fn mint(&self) -> String {
        let mut words = self.0.borrow_mut();
        match words.is_empty() {
            true => "last-one-left".to_string(),
            false => words.remove(0).to_string(),
        }
    }
}

pub fn words(ids: &[&'static str]) -> Words {
    Words(RefCell::new(ids.to_vec()))
}

/// A configuration that sets `plan_roots` as given.
#[derive(Default)]
pub struct Roots(pub Option<Vec<String>>);

impl Configuration for Roots {
    fn tag_schema(&mut self) -> Result<Vec<String>, Report> {
        Ok(Vec::new())
    }

    fn plan_roots(&mut self) -> Result<Option<Vec<String>>, Report> {
        Ok(self.0.clone())
    }
}
