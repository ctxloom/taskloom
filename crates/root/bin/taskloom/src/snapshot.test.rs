//! Acceptance: a list reads one moment of a project's SQLite store, though
//! another process writes and purges between the list's `covers` and its
//! reads of each stream. The interleaving is forced: the list's store
//! runs the other process as soon as its `covers` has answered.

use std::sync::Arc;

use angzarr_embedded::angzarr_store_sqlite::SqliteStore;
use angzarr_embedded::testing::{At, FaultyStore};
use angzarr_ports::StoreError;
use harp_names::HarpNames;
use ports::{Project, TaskStores};
use project_registry::{Environment, ProjectRegistry};
use system::System;
use tasks_application::{ListQuery, Reach, Service, TaskService};

use super::{one_document, Env};

/// What the other process does, run once, between a read's `covers` and
/// its reads.
type Meanwhile = Box<dyn FnOnce(&SqliteStore) + Send>;

/// The one project's SQLite store, opened read-only on first ask, by the
/// list's own handle, with the other process's turn armed to run just
/// before a read unit's first stream read: once its `covers` has answered
/// (angzarr-ports' FaultyStore, `At::UnitRead`).
struct Stores {
    held: Option<FaultyStore<SqliteStore>>,
    meanwhile: Option<Meanwhile>,
}

impl TaskStores for Stores {
    type Store = FaultyStore<SqliteStore>;

    async fn open(&mut self, project: &Project) -> Result<&FaultyStore<SqliteStore>, StoreError> {
        Ok(self.open_existing(project).await?.expect("a seeded store"))
    }

    async fn open_existing(
        &mut self,
        project: &Project,
    ) -> Result<Option<&FaultyStore<SqliteStore>>, StoreError> {
        if self.held.is_none() {
            let Some(store) = SqliteStore::open_existing(&project.store).await? else {
                return Ok(None);
            };
            let store = FaultyStore::new(store);
            if let Some(meanwhile) = self.meanwhile.take() {
                store.before(At::UnitRead, meanwhile);
            }
            self.held = Some(store);
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
    let env = Arc::new(Env::new("snapshot-harp"));
    let moving = add(&env, &["Moving"]);
    let pruned = add(&env, &["Pruned", "--status", "Done"]);
    let moved = moving.clone();
    let other = Arc::clone(&env);
    let meanwhile: Meanwhile = Box::new(move |_: &SqliteStore| {
        let env = &other;
        let ran = env.run(&["prune", "--older-than", "1ns", "--yes", "--json"]);
        assert!(ran.ok, "{}", ran.stderr);
        assert_eq!(one_document(&ran.stdout)["tasks_pruned"], 1);
        let ran = env.run(&["status", &moved, "In Progress", "--json"]);
        assert!(ran.ok, "{}", ran.stderr);
        add(env, &["Added meanwhile"]);
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
        service.stores.held.as_ref().unwrap().ran(At::UnitRead),
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
