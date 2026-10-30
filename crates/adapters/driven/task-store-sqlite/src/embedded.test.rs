//! RED, step 3b of taskloom-on-angzarr-embedded.plan.md: a project's store
//! on angzarr-store-sqlite's `SqliteStore`, opened on a filesystem of the
//! test's own (`Rooted::temp`, which SQLite reaches through the seam's real
//! path), on a tokio runtime as the binary runs it. Fixtures are written
//! with `SqliteStore` itself, never through the opener under test.
//!
//! The owner's ruling D2's test (a store from before angzarr-embedded is
//! refused, naming its backup) moved here from `lib.test.rs`: it is this
//! opener's to pass.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};
use angzarr_ports::angzarr_proto::v1::event_meta::Causation;
use angzarr_ports::angzarr_proto::v1::{EventMeta, EventPage, Uuid};
use angzarr_ports::{
    EventStore, PositionKey, PositionStore, Purge, StoreError, StreamRef, Watch, Write,
};
use angzarr_store_sqlite::SqliteStore;
use file_system::{Fs, Os, Perms, Rooted};
use os::Mode;
use ports::Project;
use prost_types::{Any, Timestamp};

use super::{Checked, EmbeddedStores};

use crate::tests::damage;

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a runtime")
        .block_on(future)
}

fn project(id: &str, store: PathBuf) -> Project {
    Project {
        id: id.to_string(),
        root: Some("/work/tree".to_string()),
        store,
    }
}

/// A project's store as the path policy places it:
/// `<home>/.ctxloom/projects/<id>/state.db`.
fn store_in(home: &Path, id: &str) -> PathBuf {
    home.join(".ctxloom/projects").join(id).join("state.db")
}

/// A home, `/home/u`, on a filesystem of its own.
struct Home {
    rooted: Arc<Rooted>,
    path: PathBuf,
}

impl Home {
    fn new() -> Home {
        let home = Home {
            rooted: Arc::new(Rooted::temp().unwrap()),
            path: os::fixture("/home/u"),
        };
        home.mkdir(&home.path);
        home
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// The stores on the home's filesystem.
    fn stores(&self) -> EmbeddedStores {
        EmbeddedStores::new(Arc::clone(&self.rooted) as Arc<dyn Fs>)
    }

    fn exists(&self, path: &Path) -> bool {
        self.rooted.exists(path).unwrap()
    }

    fn mkdir(&self, path: &Path) {
        self.rooted.create_dir_all(path, Mode::DIR).unwrap();
    }

    fn write(&self, path: &Path, bytes: &[u8]) {
        self.rooted
            .write_atomic(path, bytes, Perms::Exact(Mode::FILE))
            .unwrap();
    }

    fn read(&self, path: &Path) -> Vec<u8> {
        self.rooted.read(path).unwrap()
    }

    /// Where SQLite finds `path`.
    fn real(&self, path: &Path) -> PathBuf {
        self.rooted.real_path(path).unwrap()
    }
}

/// The task stream rooted at `[n; 16]`.
fn stream(n: u8) -> StreamRef<'static> {
    StreamRef::main("tasks", angzarr_ports::Uuid::from_bytes([n; 16]))
}

/// The event at `sequence` of a stream, produced by command `command`.
fn event(sequence: u64, command: u8) -> EventPage {
    let id = Uuid::from_bytes([command; 16]);
    EventPage {
        payload: Some(Any {
            type_url: "/test.v1.Event".to_string(),
            value: sequence.to_be_bytes().to_vec(),
        }),
        meta: Some(EventMeta {
            causation: Some(Causation::CommandId(id.clone())),
            correlation_id: Some(id),
            recorded_at: Some(Timestamp {
                seconds: 100 + sequence as i64,
                nanos: 0,
            }),
            rules_version: "markup-1".to_string(),
            ..EventMeta::default()
        }),
        sequence,
    }
}

/// `pages` appended to `stream` at `expected`, through `store`.
async fn append(store: &SqliteStore, stream: StreamRef<'_>, expected: u64, pages: &[EventPage]) {
    store
        .write(Write::append(stream, expected, pages))
        .await
        .unwrap();
}

/// What `store` holds of `stream`, from its start.
async fn pages(store: &SqliteStore, stream: StreamRef<'_>) -> Vec<EventPage> {
    store.read(stream, 0, None).await.unwrap().pages
}

/// A database on angzarr-embedded's schema at `path`, written by
/// `SqliteStore` itself (not the opener under test): one stream of two
/// events, closed, so the file holds everything. The store's connection
/// closes on its own thread after the store is dropped, and the last
/// connection's close checkpoints the log into the file and removes it:
/// that removal is waited for, up to a deadline, as the sign the file is
/// whole.
fn written(home: &Home, path: &Path) {
    home.mkdir(path.parent().unwrap());
    let real = home.real(path);
    block_on(async {
        let store = SqliteStore::open(&real).await.unwrap();
        append(&store, stream(1), 0, &[event(0, 1), event(1, 2)]).await;
    });
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while home.exists(Path::new(&wal)) {
        assert!(
            std::time::Instant::now() < deadline,
            "the store's connection never closed: its log is still beside the file"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// An arg's value as text, a nested cause as its code.
fn text(arg: &Arg) -> String {
    match arg {
        Arg::Int(n) => n.to_string(),
        Arg::Uint(n) => n.to_string(),
        Arg::Str(s) => s.clone(),
        Arg::Nested(cause) => cause.code().to_string(),
    }
}

/// A diagnostic's args, each as text, by name, in its order.
fn args(diagnostic: &dyn Diagnostic) -> Vec<(&'static str, String)> {
    diagnostic
        .args()
        .iter()
        .map(|(name, arg)| (*name, text(arg)))
        .collect()
}

/// The cause keyed `code` in `error`, or nested in one of its causes: its
/// args, each as text, by name; `None` if none is keyed `code`.
fn keyed(error: &StoreError, code: &str) -> Option<Vec<(&'static str, String)>> {
    fn within(
        args: &[(&'static str, angzarr_ports::Arg)],
        code: &str,
    ) -> Option<Vec<(&'static str, String)>> {
        args.iter().find_map(|(_, arg)| match arg {
            angzarr_ports::Arg::Cause(cause) if cause.code == code => Some(
                cause
                    .args
                    .iter()
                    .map(|(name, arg)| {
                        let value = match arg {
                            angzarr_ports::Arg::Uint(n) => n.to_string(),
                            angzarr_ports::Arg::Str(s) => s.clone(),
                            angzarr_ports::Arg::Cause(c) => c.code.to_string(),
                        };
                        (*name, value)
                    })
                    .collect(),
            ),
            angzarr_ports::Arg::Cause(cause) => within(&cause.args, code),
            _ => None,
        })
    }
    within(&error.args(), code)
}

// ------------------------------------------------------------ opening

#[test]
fn test_a_fresh_store_is_created_where_the_path_policy_puts_it_and_migrated() {
    let home = Home::new();
    let path = store_in(home.path(), "jolly-mute-magma");
    let project = project("jolly-mute-magma", path.clone());
    let mut stores = home.stores();
    block_on(async {
        let store = stores.open(&project).await.expect("opens");
        append(store, stream(1), 0, &[event(0, 1)]).await;
        assert_eq!(pages(store, stream(1)).await, [event(0, 1)]);
    });
    // The caller's top under the root: the root itself on unix, its `C`
    // directory on Windows (`C:\` re-rooted).
    let top = match os::host().flavor() {
        os::Flavor::Unix => home.rooted.root().to_path_buf(),
        os::Flavor::Windows => home.rooted.root().join("C"),
    };
    let real = [
        "home",
        "u",
        ".ctxloom",
        "projects",
        "jolly-mute-magma",
        "state.db",
    ]
    .iter()
    .fold(top, |path, part| path.join(part));
    assert_eq!(home.real(&path), real);
    let os = Os::new(os::host(), Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    assert!(os.exists(&real).unwrap(), "under the filesystem's root");
    // Migrated: a read-only open refuses a store with a migration pending.
    let read = block_on(SqliteStore::open_existing(&real)).unwrap();
    assert!(read.is_some(), "every migration applied");
}

#[test]
fn test_a_store_on_angzarr_embeddeds_schema_reopens_holding_what_was_written() {
    let home = Home::new();
    let path = store_in(home.path(), "jolly-mute-magma");
    written(&home, &path);
    let project = project("jolly-mute-magma", path);
    block_on(async {
        let mut stores = home.stores();
        let store = stores.open(&project).await.expect("reopens to write");
        assert_eq!(pages(store, stream(1)).await, [event(0, 1), event(1, 2)]);
        append(store, stream(1), 2, &[event(2, 3)]).await;
    });
    block_on(async {
        let mut stores = home.stores();
        let store = stores
            .open_existing(&project)
            .await
            .expect("reopens to read")
            .expect("found");
        assert_eq!(
            pages(store, stream(1)).await,
            [event(0, 1), event(1, 2), event(2, 3)]
        );
    });
}

#[test]
fn test_reading_a_project_with_no_database_finds_none_and_creates_nothing() {
    let home = Home::new();
    let path = store_in(home.path(), "tpyo");
    let mut stores = home.stores();
    let found = block_on(async {
        stores
            .open_existing(&project("tpyo", path))
            .await
            .map(|s| s.is_some())
    });
    assert_eq!(found, Ok(false));
    assert!(
        !home.exists(&home.path().join(".ctxloom")),
        "no directory, no file"
    );
}

/// The handle of the store `stores` holds for `project`, by its token.
async fn handle(stores: &mut EmbeddedStores, project: &Project) -> u64 {
    stores
        .change_token(project)
        .await
        .unwrap()
        .expect("a store")
        .handle
}

#[test]
fn test_the_store_is_opened_once_and_held_for_its_project() {
    let home = Home::new();
    let project = project(
        "jolly-mute-magma",
        store_in(home.path(), "jolly-mute-magma"),
    );
    let mut stores = home.stores();
    block_on(async {
        stores.open(&project).await.unwrap();
        stores.open(&project).await.unwrap();
        assert_eq!(handle(&mut stores, &project).await, 1, "the held store");
    });
}

#[test]
fn test_a_store_held_to_read_is_opened_again_to_write_under_a_new_handle() {
    let home = Home::new();
    let project = project(
        "jolly-mute-magma",
        store_in(home.path(), "jolly-mute-magma"),
    );
    let mut stores = home.stores();
    block_on(async {
        home.stores().open(&project).await.unwrap();
        stores
            .open_existing(&project)
            .await
            .unwrap()
            .expect("found");
        let read = stores.change_token(&project).await.unwrap().unwrap();
        let store = stores.open(&project).await.unwrap();
        append(store, stream(1), 0, &[event(0, 1)]).await;
        let written = stores.change_token(&project).await.unwrap().unwrap();
        assert_eq!((read.handle, written.handle), (1, 2));
        assert_ne!(read, written);
    });
}

#[test]
fn test_another_project_opens_its_own_store() {
    let home = Home::new();
    let first = project(
        "jolly-mute-magma",
        store_in(home.path(), "jolly-mute-magma"),
    );
    let second = project("high-tight-gulf", store_in(home.path(), "high-tight-gulf"));
    let mut stores = home.stores();
    block_on(async {
        let store = stores.open(&first).await.unwrap();
        append(store, stream(1), 0, &[event(0, 1)]).await;
        let other = stores.open(&second).await.unwrap();
        assert_eq!(pages(other, stream(1)).await, [], "not the first's");
    });
    assert!(home.exists(&second.store));
}

#[test]
fn test_the_openers_futures_are_send_so_a_server_can_spawn_them() {
    fn is_send<T: Send>(_: &T) {}
    let home = Home::new();
    let project = project("jolly-mute-magma", PathBuf::from("/nowhere/state.db"));
    let mut stores = home.stores();
    is_send(&stores.open(&project));
    is_send(&stores.open_existing(&project));
    is_send(&stores.change_token(&project));
}

// ------------------------------------- a store from before angzarr-embedded
//
// The owner's ruling D2 (taskloom-on-angzarr-embedded.plan.md): a store
// written before taskloom moved onto angzarr-embedded (annals-store-sql's,
// its events under annals' rules) is not migrated. Opened, it is refused,
// keyed `task-store-sqlite-predates-embedded` with its path and the path
// to back it up to (`state.db.pre-embedded`), so the user moves it aside
// and starts afresh; and it is left exactly as it was, never migrated in
// place (angzarr-store-sqlite's open would migrate it, one way). Its
// fixture is annals-store-sql's file, frozen in testdata.

/// The key a store from before angzarr-embedded is refused with.
const PREDATES: &str = "task-store-sqlite-predates-embedded";

/// A store as taskloom wrote one before angzarr-embedded: annals-store-
/// sql's (rev ed00923), holding one task stream of two events under
/// annals' rules (`v1`), its only migration annals' first. Frozen from what
/// annals-store-sql wrote (`SqliteStore::open`, then an append of
/// `lib.test.rs`'s `event(0, 1)` and `event(1, 2)` to a `tasks` stream
/// rooted at `[1; 16]`, closed so the file holds everything), so the test
/// needs no annals once the store leaves it.
const PRE_EMBEDDED: &[u8] = include_bytes!("../testdata/pre-embedded.db");

#[test]
fn test_a_store_that_predates_angzarr_embedded_is_refused_naming_its_backup() {
    let home = Home::new();
    let path = store_in(home.path(), "jolly-mute-magma");
    home.mkdir(path.parent().unwrap());
    home.write(&path, PRE_EMBEDDED);
    let before = home.read(&path);
    let project = project("jolly-mute-magma", path.clone());
    let mut backup = path.as_os_str().to_owned();
    backup.push(".pre-embedded");
    let expected = vec![
        ("backup", PathBuf::from(backup).display().to_string()),
        ("path", path.display().to_string()),
    ];
    let args = |refusal: Option<StoreError>| {
        let refusal = refusal.expect("refused, never opened");
        let mut args =
            keyed(&refusal, PREDATES).unwrap_or_else(|| panic!("keyed {PREDATES}: {refusal}"));
        args.sort();
        args
    };

    let mut stores = home.stores();
    let written = block_on(stores.open(&project)).map(drop).err();
    assert_eq!(args(written), expected, "to write");
    let read = block_on(stores.open_existing(&project)).map(drop).err();
    assert_eq!(args(read), expected, "to read");
    let token = block_on(stores.change_token(&project)).map(drop).err();
    assert_eq!(args(token), expected, "to watch");
    assert_eq!(home.read(&path), before, "the file as it was: not migrated");
}

// ------------------------------------------------------------ purging

#[test]
fn test_purge_removes_a_stream_and_every_position_on_it_and_it_starts_again_at_0() {
    let home = Home::new();
    let project = project("p", store_in(home.path(), "p"));
    let mut stores = home.stores();
    block_on(async {
        let store = stores.open(&project).await.unwrap();
        append(store, stream(1), 0, &[event(0, 1), event(1, 2)]).await;
        append(store, stream(2), 0, &[event(0, 3)]).await;
        let list = PositionKey {
            handler: "list",
            stream: stream(1),
        };
        store.write(Write::positions(&[(list, 2)])).await.unwrap();

        assert_eq!(store.purge(stream(1)).await.unwrap(), 2, "events deleted");
        let slice = store.read(stream(1), 0, None).await.unwrap();
        assert_eq!((slice.next_sequence, slice.pages), (0, Vec::new()));
        assert_eq!(store.position(list).await.unwrap(), 0, "its positions too");
        assert_eq!(pages(store, stream(2)).await, [event(0, 3)], "only it");
        let again = store
            .write(Write::append(stream(1), 0, &[event(0, 4)]))
            .await
            .unwrap();
        assert_eq!(again.next_sequence, Some(1), "from expected 0");
        assert_eq!(
            store.covers("tasks").await.unwrap().len(),
            2,
            "listed again"
        );
    });
}

// ------------------------------------------------------------ watching

#[test]
fn test_a_token_where_there_is_no_database_is_none_and_creates_nothing() {
    let home = Home::new();
    let mut stores = home.stores();
    let project = project("tpyo", store_in(home.path(), "tpyo"));
    assert_eq!(block_on(stores.change_token(&project)), Ok(None));
    assert!(!home.exists(&home.path().join(".ctxloom")), "nothing made");
}

#[test]
fn test_a_token_holds_until_another_handle_commits_and_then_holds_again() {
    let home = Home::new();
    let project = project(
        "jolly-mute-magma",
        store_in(home.path(), "jolly-mute-magma"),
    );
    let mut writer = home.stores();
    let mut watcher = home.stores();
    block_on(async {
        writer.open(&project).await.unwrap();
        let first = watcher
            .change_token(&project)
            .await
            .unwrap()
            .expect("a store");
        assert_eq!(watcher.change_token(&project).await, Ok(Some(first)));

        let store = writer.open(&project).await.unwrap();
        append(store, stream(1), 0, &[event(0, 1)]).await;
        let moved = watcher.change_token(&project).await.unwrap().unwrap();
        assert_ne!(moved, first, "another handle's commit moves it");
        assert_eq!(moved.handle, first.handle, "through the one handle");
        assert_eq!(watcher.change_token(&project).await, Ok(Some(moved)));
        let held = watcher
            .open_existing(&project)
            .await
            .unwrap()
            .expect("held");
        assert_eq!(
            held.change_token().await.unwrap(),
            moved.change,
            "the held store's own token"
        );
    });
}

// ------------------------------------------------------------ the check

/// A copy of the database at `path`, at `copy`, its command index's root
/// page changed by `tamper`.
fn damaged_copy(home: &Home, path: &Path, copy: &Path, tamper: impl FnOnce(&mut [u8])) {
    let file = damage::damaged(&home.read(path), "events_by_command", tamper);
    home.mkdir(copy.parent().unwrap());
    home.write(copy, &file);
}

#[test]
fn test_a_sound_database_checks_intact() {
    let home = Home::new();
    let path = store_in(home.path(), "p");
    written(&home, &path);
    assert_eq!(home.stores().check(&path), Ok(Checked::Intact));
}

#[test]
fn test_the_check_runs_from_inside_a_runtime_as_the_binary_calls_it() {
    let home = Home::new();
    let path = store_in(home.path(), "p");
    written(&home, &path);
    let checked = block_on(async { home.stores().check(&path) });
    assert_eq!(checked, Ok(Checked::Intact));
}

#[test]
fn test_damage_is_reported_as_the_stores_key_and_args_and_left_as_it_was() {
    let home = Home::new();
    let (path, copy) = (store_in(home.path(), "p"), store_in(home.path(), "copy"));
    written(&home, &path);
    // The page's last byte is the last entry's last key byte: the page
    // still reads, and the entry no longer matches its row.
    damaged_copy(&home, &path, &copy, |page| *page.last_mut().unwrap() ^= 1);
    let before = home.read(&copy);
    let Ok(Checked::Damaged(damage)) = home.stores().check(&copy) else {
        panic!("the damage went unseen");
    };
    assert_eq!(damage.code(), "angzarr-store-sqlite-damaged");
    let said = args(&damage);
    assert_eq!(
        said.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        ["first", "more"],
        "a key and its args: {damage}"
    );
    assert!(
        said[0].1.contains("events_by_command"),
        "SQLite's own: {said:?}"
    );
    assert!(
        damage
            .to_string()
            .starts_with("angzarr-store-sqlite-damaged first=\""),
        "its Display is its key and args, the problem quoted, never English: {damage}"
    );
    assert_eq!(home.read(&copy), before, "never repaired");
    assert_eq!(
        home.stores().check(&path),
        Ok(Checked::Intact),
        "the original is sound"
    );
}

#[test]
fn test_a_file_too_damaged_to_open_is_damage_with_sqlites_text_as_ext_err_txt() {
    let home = Home::new();
    let path = home.path().join("state.db");
    home.write(&path, b"not a database at all, as SQLite reads a file");
    let Ok(Checked::Damaged(damage)) = home.stores().check(&path) else {
        panic!("damage, not a failure to check");
    };
    assert_eq!(damage.code(), "angzarr-store-sqlite-damaged-file");
    assert_eq!(
        args(&damage),
        [
            ("errno", "26".to_string()),
            ("ext_err_txt", "file is not a database".to_string())
        ],
        "SQLITE_NOTADB, and SQLite's text verbatim: {damage}"
    );
}

#[test]
fn test_a_check_that_cannot_run_fails_with_the_stores_error_the_drivers_text_as_ext_err_txt() {
    let home = Home::new();
    // A directory where the database file goes.
    let path = home.path().join("state.db");
    home.mkdir(&path);
    let failed = home.stores().check(&path).unwrap_err();
    assert_eq!(failed.code(), "angzarr-ports-store-unavailable");
    let cause = failed.args().into_iter().find_map(|(name, arg)| match arg {
        Arg::Nested(cause) if name == "cause" => Some(cause),
        _ => None,
    });
    let cause = cause.unwrap_or_else(|| panic!("the store's cause: {failed}"));
    // As SQLite's read-only open fails it, here and now.
    let direct = block_on(SqliteStore::open_existing(&home.real(&path)))
        .map(|found| found.is_some())
        .unwrap_err();
    let direct = keyed(&direct, cause.code()).expect("the same cause");
    assert_eq!(cause.code(), "angzarr-store-sqlite-unavailable");
    assert_eq!(args(cause.as_ref()), direct);
    assert!(
        args(cause.as_ref())
            .iter()
            .any(|(name, text)| *name == "ext_err_txt" && !text.is_empty()),
        "the driver's text, verbatim: {cause}"
    );
}

#[test]
fn test_no_database_is_absent_not_damage_and_creates_nothing() {
    let home = Home::new();
    let path = store_in(home.path(), "p");
    assert_eq!(home.stores().check(&path), Ok(Checked::Absent));
    assert!(
        !home.exists(&home.path().join(".ctxloom")),
        "nothing created"
    );
}
