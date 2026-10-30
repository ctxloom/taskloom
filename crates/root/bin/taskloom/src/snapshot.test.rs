//! Acceptance: a list reads one moment of a project's SQLite store, though
//! another process writes and purges between the list's `covers` and its
//! reads of each stream. The interleaving is forced: the list's store
//! runs the other process as soon as its `covers` has answered.

use std::sync::Mutex;

use angzarr_ports::angzarr_proto::v1::{Cover, EventPage, Snapshot};
use angzarr_ports::{
    ChangeToken, EventStore, PositionKey, PositionStore, Purge, ReadStore, ReadUnit, Slice,
    SnapshotStore, StoreError, StreamRef, Watch, Write, Written,
};
use angzarr_store_sqlite::{SqliteRead, SqliteStore};
use harp_names::HarpNames;
use ports::{Project, TaskStores};
use project_registry::{Environment, ProjectRegistry};
use system::System;
use tasks_application::{ListQuery, Reach, Service, TaskService};

use super::{one_document, Env};

/// What the other process does, run once, between a read's `covers` and
/// its reads.
type Meanwhile<'e> = Box<dyn FnOnce() + Send + 'e>;

/// The project's SQLite store, opened by the list's own handle, running
/// `meanwhile` once a read-only unit's `covers` has answered.
struct Interleaved<'e> {
    store: SqliteStore,
    meanwhile: Mutex<Option<Meanwhile<'e>>>,
}

/// A read-only unit over [`Interleaved`]'s store.
struct InterleavedRead<'s, 'e> {
    reads: SqliteRead<'s>,
    meanwhile: &'s Mutex<Option<Meanwhile<'e>>>,
}

fn run(meanwhile: &Mutex<Option<Meanwhile<'_>>>) {
    let meanwhile = meanwhile.lock().unwrap().take();
    if let Some(meanwhile) = meanwhile {
        meanwhile();
    }
}

impl EventStore for Interleaved<'_> {
    async fn write(&self, write: Write<'_>) -> Result<Written, StoreError> {
        self.store.write(write).await
    }

    async fn read(
        &self,
        stream: StreamRef<'_>,
        from: u64,
        to: Option<u64>,
    ) -> Result<Slice, StoreError> {
        self.store.read(stream, from, to).await
    }

    async fn find_command(
        &self,
        stream: StreamRef<'_>,
        command_id: angzarr_ports::Uuid,
    ) -> Result<Vec<EventPage>, StoreError> {
        self.store.find_command(stream, command_id).await
    }

    async fn covers(&self, domain: &str) -> Result<Vec<Cover>, StoreError> {
        self.store.covers(domain).await
    }
}

impl SnapshotStore for Interleaved<'_> {
    async fn read_snapshot(&self, stream: StreamRef<'_>) -> Result<Option<Snapshot>, StoreError> {
        self.store.read_snapshot(stream).await
    }

    async fn write_snapshot(
        &self,
        stream: StreamRef<'_>,
        snapshot: &Snapshot,
    ) -> Result<(), StoreError> {
        self.store.write_snapshot(stream, snapshot).await
    }
}

impl PositionStore for Interleaved<'_> {
    async fn position(&self, key: PositionKey<'_>) -> Result<u64, StoreError> {
        self.store.position(key).await
    }
}

impl Purge for Interleaved<'_> {
    async fn purge(&self, stream: StreamRef<'_>) -> Result<u64, StoreError> {
        self.store.purge(stream).await
    }
}

impl Watch for Interleaved<'_> {
    async fn change_token(&self) -> Result<ChangeToken, StoreError> {
        self.store.change_token().await
    }
}

impl<'e> ReadStore for Interleaved<'e> {
    type Reads<'s>
        = InterleavedRead<'s, 'e>
    where
        Self: 's;

    async fn begin_read(&self) -> Result<InterleavedRead<'_, 'e>, StoreError> {
        Ok(InterleavedRead {
            reads: self.store.begin_read().await?,
            meanwhile: &self.meanwhile,
        })
    }
}

impl ReadUnit for InterleavedRead<'_, '_> {
    async fn covers(&mut self, domain: &str) -> Result<Vec<Cover>, StoreError> {
        let covers = self.reads.covers(domain).await;
        run(self.meanwhile);
        covers
    }

    async fn read(
        &mut self,
        stream: StreamRef<'_>,
        from: u64,
        to: Option<u64>,
    ) -> Result<Slice, StoreError> {
        self.reads.read(stream, from, to).await
    }

    async fn read_snapshot(
        &mut self,
        stream: StreamRef<'_>,
    ) -> Result<Option<Snapshot>, StoreError> {
        self.reads.read_snapshot(stream).await
    }
}

/// The one project's store, opened read-only on first ask, with the
/// other process's turn armed.
struct Stores<'e> {
    held: Option<Interleaved<'e>>,
    meanwhile: Option<Meanwhile<'e>>,
}

impl<'e> TaskStores for Stores<'e> {
    type Store = Interleaved<'e>;

    async fn open(&mut self, project: &Project) -> Result<&Interleaved<'e>, StoreError> {
        Ok(self.open_existing(project).await?.expect("a seeded store"))
    }

    async fn open_existing(
        &mut self,
        project: &Project,
    ) -> Result<Option<&Interleaved<'e>>, StoreError> {
        if self.held.is_none() {
            let Some(store) = SqliteStore::open_existing(&project.store).await? else {
                return Ok(None);
            };
            self.held = Some(Interleaved {
                store,
                meanwhile: Mutex::new(self.meanwhile.take()),
            });
        }
        Ok(self.held.as_ref())
    }
}

/// Add `args` as a task through the binary; its harp.
fn add(env: &Env, args: &[&str]) -> String {
    let added = env.run(&[&["add"][..], args, &["--json"]].concat());
    assert!(added.ok, "{}", added.stderr);
    one_document(&added.stdout)["harp_id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn test_a_list_reads_one_moment_though_another_process_writes_and_purges_meanwhile() {
    let env = Env::new("snapshot-harp");
    let moving = add(&env, &["Moving"]);
    let pruned = add(&env, &["Pruned", "--status", "Done"]);
    let moved = moving.clone();
    let meanwhile: Meanwhile<'_> = Box::new(|| {
        let ran = env.run(&["prune", "--older-than", "1ns", "--yes", "--json"]);
        assert!(ran.ok, "{}", ran.stderr);
        assert_eq!(one_document(&ran.stdout)["tasks_pruned"], 1);
        let ran = env.run(&["status", &moved, "In Progress", "--json"]);
        assert!(ran.ok, "{}", ran.stderr);
        add(&env, &["Added meanwhile"]);
    });
    let environment = Environment {
        home: Some(env.home.path().to_path_buf()),
        root: None,
        project_id: None,
    };
    let stores = Stores {
        held: None,
        meanwhile: Some(meanwhile),
    };
    let mut service = Service::new(
        ProjectRegistry::new(
            std::sync::Arc::new(file_system::Os::new(os::host(), env.project.path()).unwrap()),
            environment,
            HarpNames,
        ),
        stores,
        HarpNames,
        System,
        System,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let all = ListQuery {
        all: true,
        ..ListQuery::default()
    };
    let listing = runtime.block_on(service.list(all)).unwrap();
    assert!(matches!(listing.reach, Reach::Project(_)));
    assert!(
        service
            .stores
            .held
            .as_ref()
            .unwrap()
            .meanwhile
            .lock()
            .unwrap()
            .is_none(),
        "the other process ran between the list's covers and its reads"
    );
    let seen: Vec<(&str, &str)> = listing
        .tasks()
        .map(|t| (t.harp_id.as_str(), t.status.as_str()))
        .collect();
    assert_eq!(
        seen,
        [(moving.as_str(), "To Do"), (pruned.as_str(), "Done")],
        "the store as the list's covers saw it: nothing moved, purged or added since"
    );

    let listed = env.run(&["list", "--all", "--json"]);
    assert!(listed.ok, "{}", listed.stderr);
    let after: Vec<String> = one_document(&listed.stdout)
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            format!(
                "{} {}",
                t["text"].as_str().unwrap(),
                t["status"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(after, ["Moving In Progress", "Added meanwhile To Do"]);
}
