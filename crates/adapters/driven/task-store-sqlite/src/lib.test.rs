//! Opening a project's store on a filesystem of the test's own
//! (`Rooted::temp`, which SQLite reaches through the seam's real path), on
//! a tokio runtime as the binary runs it, and checking a database for
//! damage. Fixtures are written with `SqliteStore` itself, never through
//! the opener under test.

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
use ports::{Checked, Project, StoreCheck, TaskStores, WatchedStores};
use prost_types::{Any, Timestamp};

use super::SqliteStores;

#[path = "damage.test.rs"]
pub(crate) mod damage;

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
    fn stores(&self) -> SqliteStores {
        SqliteStores::new(Arc::clone(&self.rooted) as Arc<dyn Fs>)
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

    /// Remove the database at `path` from under whatever holds it open,
    /// where the host allows it: unix does; Windows refuses to remove a
    /// file SQLite holds open (it shares no delete access), so there it is
    /// left, and `false` says so.
    fn remove_held(&self, path: &Path) -> bool {
        if os::host().flavor() != os::Flavor::Unix {
            return false;
        }
        self.rooted.remove_file(path).unwrap();
        true
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
async fn handle(stores: &mut SqliteStores, project: &Project) -> u64 {
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

        let stale = store.purge(stream(1), 1).await.unwrap_err();
        assert!(
            matches!(stale, StoreError::Conflict { actual: 2, .. }),
            "a head that moved since it was read is a conflict: {stale}"
        );
        assert_eq!(
            store.purge(stream(1), 2).await.unwrap(),
            2,
            "events deleted"
        );
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

// ------------------------------------------------------------ opening, failing

/// A filesystem that cannot say where anything is on disk (as a real path
/// a Windows prefix leaves relative cannot be), and is otherwise `Rooted`.
#[derive(Debug)]
struct Unplaceable(Rooted);

impl Fs for Unplaceable {
    fn cwd(&self) -> Result<&Path, file_system::CwdError> {
        self.0.cwd()
    }
    fn os(&self) -> &dyn os::Os {
        self.0.os()
    }
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.0.read(path)
    }
    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        self.0.read_to_string(path)
    }
    fn write_atomic(&self, path: &Path, bytes: &[u8], perms: Perms) -> std::io::Result<()> {
        self.0.write_atomic(path, bytes, perms)
    }
    fn open(&self, path: &Path, how: file_system::Open) -> std::io::Result<std::fs::File> {
        self.0.open(path, how)
    }
    fn create_dir_all(&self, path: &Path, mode: Mode) -> std::io::Result<()> {
        self.0.create_dir_all(path, mode)
    }
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        self.0.remove_file(path)
    }
    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        self.0.remove_dir_all(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        self.0.rename(from, to)
    }
    fn set_permissions(&self, path: &Path, mode: Mode) -> std::io::Result<()> {
        self.0.set_permissions(path, mode)
    }
    fn read_dir(&self, path: &Path) -> std::io::Result<Vec<file_system::Entry>> {
        self.0.read_dir(path)
    }
    fn metadata(&self, path: &Path) -> std::io::Result<file_system::Meta> {
        self.0.metadata(path)
    }
    fn symlink_metadata(&self, path: &Path) -> std::io::Result<file_system::Meta> {
        self.0.symlink_metadata(path)
    }
    fn exists(&self, path: &Path) -> std::io::Result<bool> {
        self.0.exists(path)
    }
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        self.0.canonicalize(path)
    }
    fn symlink_dir(&self, target: &Path, link: &Path) -> std::io::Result<()> {
        self.0.symlink_dir(target, link)
    }
    fn read_link(&self, link: &Path) -> std::io::Result<PathBuf> {
        self.0.read_link(link)
    }
    fn make_temp_dir(&self, parent: &Path, prefix: &str) -> std::io::Result<PathBuf> {
        self.0.make_temp_dir(parent, prefix)
    }
    fn real_path(&self, _: &Path) -> std::io::Result<PathBuf> {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no place on disk",
        ))
    }
    fn same_file(&self, file: &std::fs::File, path: &Path) -> std::io::Result<bool> {
        self.0.same_file(file, path)
    }
}

/// A database older than this build: one table (`older`) committed, and
/// none of angzarr-store-sqlite's migrations applied. Made with Python's
/// sqlite3: `PRAGMA page_size=512; CREATE TABLE older (x)`.
const OLDER: &[u8] = include_bytes!("../testdata/older.db");

#[test]
fn test_a_directory_that_cannot_be_made_fails_naming_it() {
    let home = Home::new();
    let blocker = home.path().join("projects");
    home.write(&blocker, b"a file where the directory goes");
    let path = blocker.join("jolly-mute-magma").join("state.db");
    let mut stores = home.stores();
    let error = block_on(stores.open(&project("jolly-mute-magma", path)))
        .map(drop)
        .unwrap_err();
    let args = keyed(&error, "task-store-sqlite-create-dir").expect("refused naming it");
    assert_eq!(args[0].0, "path");
    assert!(
        args[0].1.contains(&blocker.display().to_string()),
        "{error}"
    );
}

#[test]
fn test_a_database_that_does_not_open_fails_with_the_stores_error() {
    let home = Home::new();
    // A directory where the database file goes.
    let path = home.path().join("state.db");
    home.mkdir(&path);
    let mut stores = home.stores();
    let error = block_on(stores.open(&project("jolly-mute-magma", path)))
        .map(drop)
        .unwrap_err();
    assert!(
        matches!(&error, StoreError::Unavailable(cause) if cause.code == "angzarr-store-sqlite-unavailable"),
        "the store's: {error}"
    );
}

#[test]
fn test_a_store_the_filesystem_cannot_place_is_refused_naming_it() {
    let mut stores = SqliteStores::new(Arc::new(Unplaceable(Rooted::temp().unwrap())));
    let path = PathBuf::from("/home/u/state.db");
    let project = project("jolly-mute-magma", path.clone());
    let unplaced = |error: StoreError| {
        let args = keyed(&error, "task-store-sqlite-unplaced").expect("refused naming it");
        assert_eq!(
            args,
            [
                ("path", "/home/u/state.db".to_string()),
                ("ext_err_txt", "no place on disk".to_string())
            ]
        );
    };
    unplaced(block_on(stores.open(&project)).map(drop).unwrap_err());
    unplaced(
        block_on(stores.open_existing(&project))
            .map(drop)
            .unwrap_err(),
    );
    unplaced(
        block_on(stores.change_token(&project))
            .map(drop)
            .unwrap_err(),
    );
    let checked = stores.check(&path).unwrap_err();
    assert_eq!(checked.code(), "task-store-sqlite-unplaced");
}

#[test]
fn test_reading_an_existing_database_opens_it_read_only() {
    let home = Home::new();
    let path = store_in(home.path(), "jolly-mute-magma");
    let project = project("jolly-mute-magma", path);
    block_on(async {
        home.stores().open(&project).await.unwrap();
        let mut stores = home.stores();
        let store = stores
            .open_existing(&project)
            .await
            .unwrap()
            .expect("found");
        assert!(store.read(stream(1), 0, None).await.is_ok());
        assert!(!stores.held.as_ref().unwrap().writable);
    });
}

#[test]
fn test_a_store_held_to_write_serves_a_read() {
    let home = Home::new();
    let path = store_in(home.path(), "jolly-mute-magma");
    let project = project("jolly-mute-magma", path.clone());
    let mut stores = home.stores();
    block_on(async {
        stores.open(&project).await.unwrap();
        home.remove_held(&path);
        let found = stores.open_existing(&project).await.unwrap();
        assert!(found.is_some(), "the held store, though the file is gone");
    });
    assert_eq!(stores.opened, 1, "the held store, not opened again to read");
    assert!(stores.held.as_ref().unwrap().writable);
}

#[test]
fn test_reading_another_project_does_not_serve_the_held_one() {
    let home = Home::new();
    let mut stores = home.stores();
    block_on(async {
        let first = project(
            "jolly-mute-magma",
            store_in(home.path(), "jolly-mute-magma"),
        );
        stores.open(&first).await.unwrap();
        let other = project("tpyo", store_in(home.path(), "tpyo"));
        assert!(stores.open_existing(&other).await.unwrap().is_none());
    });
}

#[test]
fn test_reading_a_database_not_yet_migrated_is_refused_and_leaves_it_unmigrated() {
    // A read never migrates: only a write does. A file holding something
    // committed is no file a writer is making, so it is refused at once.
    let home = Home::new();
    let path = home.path().join("state.db");
    home.write(&path, OLDER);
    let mut stores = home.stores();
    let error = block_on(stores.open_existing(&project("jolly-mute-magma", path.clone())))
        .map(|found| found.is_some())
        .unwrap_err();
    assert_eq!(
        keyed(&error, "angzarr-store-sqlite-unmigrated"),
        Some(vec![("pending", "4".to_string())]),
        "{error}"
    );
    assert!(stores.held.is_none());
    assert_eq!(home.read(&path), OLDER, "the file as it was");
    assert!(
        !home.exists(&home.path().join("state.db-wal")),
        "not moved into WAL"
    );
}

#[test]
fn test_a_database_that_cannot_be_read_fails_a_read() {
    let home = Home::new();
    let path = home.path().join("state.db");
    home.write(&path, b"not a database");
    let mut stores = home.stores();
    let error = block_on(stores.open_existing(&project("jolly-mute-magma", path)))
        .map(|found| found.is_some())
        .unwrap_err();
    assert!(matches!(error, StoreError::Unavailable(_)), "{error:?}");
}

#[test]
fn test_a_read_that_fails_fails_with_the_read_only_opens_error() {
    // A directory where the database goes: the read-only open fails, and
    // its error is passed through as it is.
    let home = Home::new();
    let path = home.path().join("state.db");
    home.mkdir(&path);
    let mut stores = home.stores();
    let error = block_on(stores.open_existing(&project("jolly-mute-magma", path.clone())))
        .map(|found| found.is_some())
        .unwrap_err();
    let read_only = block_on(SqliteStore::open_existing(&home.real(&path)))
        .map(|found| found.is_some())
        .unwrap_err();
    assert_eq!(error.to_string(), read_only.to_string());
}

#[test]
fn test_a_token_is_read_through_the_store_the_next_read_is_through() {
    let home = Home::new();
    let path = store_in(home.path(), "jolly-mute-magma");
    let project = project("jolly-mute-magma", path.clone());
    let mut stores = home.stores();
    block_on(async {
        home.stores().open(&project).await.unwrap();
        stores
            .change_token(&project)
            .await
            .unwrap()
            .expect("a store");
        home.remove_held(&path);
        let held = stores.open_existing(&project).await.unwrap();
        assert!(
            held.is_some(),
            "the handle that answered, though the file is gone"
        );
    });
    assert_eq!(stores.opened, 1, "the handle that answered, not another");
}

#[test]
fn test_a_page_beyond_reading_is_damage_not_a_failure_to_check() {
    let home = Home::new();
    let (path, copy) = (store_in(home.path(), "p"), store_in(home.path(), "copy"));
    written(&home, &path);
    damaged_copy(&home, &path, &copy, |page| page.fill(0xff));
    let Ok(Checked::Damaged(damage)) = home.stores().check(&copy) else {
        panic!("the damage went unseen");
    };
    assert_eq!(damage.code(), "angzarr-store-sqlite-damaged");
}
