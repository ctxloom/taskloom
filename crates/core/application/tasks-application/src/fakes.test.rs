//! Fakes for the ports, and a store that fails on cue, for the use cases'
//! tests. Every future they return is ready at once, so [`ready`] runs a use
//! case to its end without a runtime.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

use angzarr_diagnostic::Report;
use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::angzarr_ports::Cause;
use angzarr_embedded::testing::FaultyStore;
use angzarr_embedded::v1::{CommandBook, Cover};
use angzarr_embedded::{
    CommandBookExt, EventStore, Kernel, Purge, Sampled, Slice, StoreError, StreamRef, Watch,
};
use ports::{
    HeldToken, Names, Project, ProjectIdentity, TaskStores, Timestamp, Uuid, WatchedStores,
};

/// Run a future whose every await is ready at once: the test support's.
pub use angzarr_embedded::testing::block_on_ready as ready;
/// How a write or a purge goes wrong, and where another process's doings
/// run: angzarr-ports' faulty store's.
pub use angzarr_embedded::testing::{At, Fault};
/// The clock, ids and delay: the ports' stand-ins.
pub use ports::testing::{FakeClock, FakeDelay};

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

impl FakeIdentity {
    /// A work root whose project is bound already: [`project`].
    pub fn bound() -> FakeIdentity {
        FakeIdentity {
            bound: Some(project()),
            ..FakeIdentity::default()
        }
    }
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

/// What another process does to a store.
pub type Meanwhile = angzarr_embedded::testing::Meanwhile<MemoryStore>;

/// Run `command` on `harp`'s stream in `store` directly, as another
/// process would: what a [`Meanwhile`] does to change a task.
pub fn run(store: &MemoryStore, harp: &str, command: tasks::TaskCommand) {
    let registry = crate::registry();
    let sampled = Sampled {
        now: Timestamp {
            seconds: 1_700_000_000 + 20 * 86_400,
            nanos: 0,
        },
        command_id: Uuid::from_bytes([0xee; 16]),
    };
    ready(
        Kernel::new(&registry)
            .on_retryable(kernel_runner::KERNEL_RETRY)
            .handle::<tasks::TaskAggregate, _>(
                store,
                CommandBook::command_of(crate::task_cover(harp), &command),
                &sampled,
            ),
    )
    .unwrap_or_else(|failure| panic!("{failure:?}"));
}

/// Declare `tags` on `store`'s schema stream directly, as a store holding
/// them already does: what an old store's schema, or one a test begins
/// from, holds.
pub fn declared(store: &MemoryStore, tags: &[&str]) {
    let registry = crate::registry();
    let sampled = Sampled {
        now: Timestamp {
            seconds: 1_700_000_000,
            nanos: 0,
        },
        command_id: Uuid::from_bytes([0xdd; 16]),
    };
    let declare = tasks::schema::Declare {
        add: tags
            .iter()
            .map(|t| tagma_core::Tag::parse(t).unwrap().into())
            .collect(),
        remove: Vec::new(),
    };
    let cover = crate::schema::schema_stream();
    ready(
        Kernel::new(&registry)
            .on_retryable(kernel_runner::KERNEL_RETRY)
            .handle::<tasks::schema::SchemaAggregate, _>(
                store,
                CommandBook::command_of(cover, &tasks::schema::SchemaCommand::from(declare)),
                &sampled,
            ),
    )
    .unwrap_or_else(|failure| panic!("{failure:?}"));
}

/// Purge `harp`'s stream in `store` directly, as another process would.
pub fn purge(store: &MemoryStore, harp: &str) -> u64 {
    let cover = crate::task_cover(harp);
    let stream = StreamRef::try_from(&cover).unwrap();
    let head = ready(EventStore::head(store, stream)).unwrap();
    ready(Purge::purge(store, stream, head.next_sequence)).unwrap()
}

/// angzarr-ports' in-memory store, failing what a test says: its writes
/// and purges in order (`fail`), with another process's doings run just
/// before a call (`before`, `before_nth`), reading hollow (`hollow`); the
/// attempts, the read units begun and their pages counted.
pub type Flaky = FaultyStore<MemoryStore>;

/// A store whose next writes and purges fail as `faults` say.
pub fn failing(faults: &[Fault]) -> Flaky {
    let flaky = Flaky::default();
    flaky.fail(faults.iter().cloned());
    flaky
}

/// The stream `cover` names in `store` from `from`, as a reader outside any
/// use case reads it: `None` when it holds no events.
pub async fn read_from(
    store: &Flaky,
    cover: &Cover,
    from: u64,
) -> Result<Option<Slice>, StoreError> {
    let stream = StreamRef::try_from(cover)?;
    let slice = store.inner().read(stream, from, None).await?;
    Ok((slice.next_sequence > 0).then_some(slice))
}

fn unavailable() -> StoreError {
    StoreError::Unavailable(Cause::new("fake-busy", []))
}

/// One store, whichever project is asked for, but a project in `others`,
/// which has its own: absent until a write opens it, if `absent`. The
/// projects opened to write (`opened`) and to read (`looked`) are
/// recorded. Its tokens are the in-memory store's under `handle`, which a
/// test changes to stand for the store opened again; `before_token` is
/// done to the store as a token is next asked for, before it is read. With
/// `open_fails`, every open fails.
#[derive(Default)]
pub struct FakeStores {
    pub store: Flaky,
    pub open_fails: bool,
    pub absent: bool,
    pub opened: Vec<String>,
    pub looked: Vec<String>,
    pub others: std::collections::BTreeMap<String, Flaky>,
    pub handle: u64,
    pub before_token: Option<Meanwhile>,
}

impl FakeStores {
    /// Stores whose store was made permitting every tag: its schema stream
    /// holds the seed's rules and, for its permits, `tagma.permit:"*:*"`,
    /// so a test of anything but the permits writes whatever tags it names
    /// (every store is an allowlist, F6). With [`FakeIdentity::bound`],
    /// whose project it is.
    pub fn permitting_every_tag() -> FakeStores {
        let stores = FakeStores::default();
        let mut tags: Vec<&str> = crate::SEEDED_SCHEMA
            .iter()
            .copied()
            .filter(|spelled| !spelled.starts_with("tagma.permit:"))
            .collect();
        tags.push(r#"tagma.permit:"*:*""#);
        declared(stores.store.inner(), &tags);
        stores
    }
}

impl WatchedStores for FakeStores {
    async fn change_token(&mut self, project: &Project) -> Result<Option<HeldToken>, StoreError> {
        let handle = self.handle;
        let before = self.before_token.take();
        let Some(store) = self.open_existing(project).await? else {
            return Ok(None);
        };
        if let Some(meanwhile) = before {
            meanwhile(store.inner());
        }
        let change = Watch::change_token(store.inner()).await?;
        Ok(Some(HeldToken { handle, change }))
    }
}

impl TaskStores for FakeStores {
    type Store = Flaky;

    async fn open(&mut self, project: &Project) -> Result<&Flaky, StoreError> {
        self.opened.push(project.id.clone());
        if self.open_fails {
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
        if self.open_fails {
            return Err(unavailable());
        }
        if let Some(other) = self.others.get(&project.id) {
            return Ok(Some(other));
        }
        Ok((!self.absent).then_some(&self.store))
    }
}

/// Task ids in the order given; project ids are never asked for here. A
/// key's task id at a position is `seeded`'s for it; else, at position 0,
/// the next in order, kept for the key from then on, and past it one of its
/// own (`spare<position>-<key>`).
#[derive(Default)]
pub struct FakeNames {
    pub task_ids: VecDeque<String>,
    pub seeded: BTreeMap<(String, u64), String>,
    /// Each key and position asked for, in order.
    pub probed: Vec<(String, u64)>,
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

    /// `key`'s task ids, position 0 first.
    pub fn seed(&mut self, key: &str, ids: &[&str]) {
        for (position, id) in (0..).zip(ids) {
            self.seeded
                .insert((key.to_string(), position), id.to_string());
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

    fn task_id_for(&mut self, request_id: &str, position: u64) -> String {
        let at = (request_id.to_string(), position);
        self.probed.push(at.clone());
        if let Some(id) = self.seeded.get(&at) {
            return id.clone();
        }
        if position > 0 {
            return format!("spare{position}-{request_id}");
        }
        let id = self
            .task_ids
            .pop_front()
            .or_else(|| self.then.clone())
            .expect("a task id scripted");
        self.seeded.insert(at, id.clone());
        id
    }

    fn project_id(&mut self) -> String {
        unreachable!("the application mints no project id")
    }
}

/// The code of one of tag-rules' diagnostics, by its suffix (`tag-blank`):
/// the codes follow the crate that defines them (the owner's ruling,
/// 2026-11-03), so a test asks that crate's samples, never spells its name.
pub fn tag_code(suffix: &str) -> &'static str {
    let found = tag_rules::diagnostic_samples()
        .iter()
        .map(|sample| sample.code().to_string())
        .find(|code| code.strip_suffix(suffix).is_some_and(|p| p.ends_with('-')))
        .unwrap_or_else(|| panic!("no tag-rules diagnostic {suffix}"));
    Box::leak(found.into_boxed_str())
}
