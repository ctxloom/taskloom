//! Opening a project's store on a filesystem of the test's own
//! (`Rooted::temp`, which SQLite reaches through the seam's real path), on
//! a tokio runtime as the binary runs it, and checking a database for
//! damage.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use annals::next::v1::event_meta::Causation;
use annals::next::v1::{Cover, EventMeta, EventPage, Uuid};
use annals::next::{Any, AsyncEventStore};
use annals_diagnostic::Diagnostic;
use file_system::{Fs, Mode, Os, Perms, Rooted};
use ports::{Checked, Project, StoreCheck, TaskStores, WatchedStores};

use annals_store_sql::sqlite::SqliteStore;

use super::SqliteStores;

#[path = "damage.test.rs"]
mod damage;

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
        root: "/work/tree".to_string(),
        store,
    }
}

fn some_stream() -> Cover {
    Cover {
        domain: "tasks".to_string(),
        root: None,
        edition: None,
    }
}

/// Whether `store` answers a read: it is a working, migrated store.
async fn answers(stores: &mut SqliteStores, project: &Project) -> bool {
    let store = stores.open(project).await.expect("opens");
    store.read_from(&some_stream(), 0).await.is_ok()
}

fn store_in(dir: &Path, id: &str) -> PathBuf {
    dir.join(".ctxloom/projects").join(id).join("state.db")
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
            path: PathBuf::from("/home/u"),
        };
        home.mkdir(&home.path);
        home
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn fs(&self) -> Arc<dyn Fs> {
        Arc::clone(&self.rooted) as Arc<dyn Fs>
    }

    /// The stores on the home's filesystem.
    fn stores(&self) -> SqliteStores {
        SqliteStores::new(self.fs())
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

    fn remove(&self, path: &Path) {
        self.rooted.remove_file(path).unwrap();
    }

    /// Where SQLite finds `path`.
    fn real(&self, path: &Path) -> PathBuf {
        self.rooted.real_path(path).unwrap()
    }
}

#[test]
fn test_the_database_is_where_the_filesystem_puts_it() {
    let home = Home::new();
    let path = store_in(home.path(), "jolly-mute-magma");
    let mut stores = home.stores();
    assert!(block_on(answers(
        &mut stores,
        &project("jolly-mute-magma", path.clone())
    )));
    let real = home
        .rooted
        .root()
        .join("home/u/.ctxloom/projects/jolly-mute-magma/state.db");
    assert_eq!(home.real(&path), real);
    let os = Os::new(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    assert!(os.exists(&real).unwrap(), "under the filesystem's root");
    assert!(
        !os.exists(&path).unwrap(),
        "never at the caller's path on the real filesystem"
    );
}

#[test]
fn test_opening_creates_the_directory_and_a_migrated_database() {
    let home = Home::new();
    let path = store_in(home.path(), "jolly-mute-magma");
    let mut stores = home.stores();
    assert!(block_on(answers(
        &mut stores,
        &project("jolly-mute-magma", path.clone())
    )));
    assert!(home.exists(&path), "the database is on disk");
}

#[test]
fn test_the_store_is_opened_once_and_held_for_its_project() {
    let home = Home::new();
    let path = store_in(home.path(), "jolly-mute-magma");
    let project = project("jolly-mute-magma", path.clone());
    let mut stores = home.stores();
    block_on(async {
        assert!(answers(&mut stores, &project).await);
        home.remove(&path);
        assert!(answers(&mut stores, &project).await);
    });
    assert!(!home.exists(&path), "the held store, not a new file");
}

#[test]
fn test_another_project_opens_its_own_store() {
    let home = Home::new();
    let (first, second) = (
        store_in(home.path(), "jolly-mute-magma"),
        store_in(home.path(), "high-tight-gulf"),
    );
    let mut stores = home.stores();
    block_on(async {
        assert!(answers(&mut stores, &project("jolly-mute-magma", first.clone())).await);
        assert!(answers(&mut stores, &project("high-tight-gulf", second.clone())).await);
    });
    assert!(home.exists(&first) && home.exists(&second));
}

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
    let annals::next::StoreError::Unavailable(report) = &error else {
        panic!("unavailable: {error:?}");
    };
    assert_eq!(report.code(), "task-store-sqlite-create-dir");
    assert!(
        report.to_string().contains(&blocker.display().to_string()),
        "{report}"
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
    let annals::next::StoreError::Unavailable(report) = &error else {
        panic!("unavailable: {error:?}");
    };
    assert_eq!(report.code(), "annals-store-sql-unavailable");
}

#[test]
fn test_the_openers_futures_are_send_so_a_server_can_spawn_them() {
    fn is_send<T: Send>(_: &T) {}
    let home = Home::new();
    let path = PathBuf::from("/nowhere/state.db");
    let project = project("jolly-mute-magma", path.clone());
    let mut stores = home.stores();
    is_send(&stores.open(&project));
    is_send(&stores.open_existing(&project));
    is_send(&SqliteStore::open(&path));
}

/// A filesystem that cannot say where anything is on disk (as a real path
/// a Windows prefix leaves relative cannot be), and is otherwise `Rooted`.
#[derive(Debug)]
struct Unplaceable(Rooted);

impl Fs for Unplaceable {
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
    fn absolute(&self, path: &Path) -> std::io::Result<PathBuf> {
        self.0.absolute(path)
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

#[test]
fn test_a_store_the_filesystem_cannot_place_is_refused_naming_it() {
    let mut stores = SqliteStores::new(Arc::new(Unplaceable(Rooted::temp().unwrap())));
    let path = PathBuf::from("/home/u/state.db");
    let project = project("jolly-mute-magma", path.clone());
    let unplaced = |error: annals::next::StoreError| {
        let annals::next::StoreError::Unavailable(report) = &error else {
            panic!("unavailable: {error:?}");
        };
        assert_eq!(report.code(), "task-store-sqlite-unplaced");
        assert!(report.to_string().contains("/home/u/state.db"), "{report}");
        assert!(report.to_string().contains("no place on disk"), "{report}");
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

// ------------------------------------------------------------ reads

#[test]
fn test_reading_a_project_with_no_database_finds_none_and_creates_nothing() {
    let home = Home::new();
    let path = store_in(home.path(), "tpyo");
    let mut stores = home.stores();
    let found = block_on(stores.open_existing(&project("tpyo", path))).unwrap();
    assert!(found.is_none());
    assert!(
        !home.exists(&home.path().join(".ctxloom")),
        "no directory, no file"
    );
    assert!(stores.held.is_none());
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
        assert!(store.read_from(&some_stream(), 0).await.is_ok());
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
        home.remove(&path);
        let found = stores.open_existing(&project).await.unwrap();
        assert!(found.is_some(), "the held store, though the file is gone");
    });
    assert!(stores.held.as_ref().unwrap().writable);
}

#[test]
fn test_a_store_held_to_read_is_opened_again_to_write() {
    let home = Home::new();
    let path = store_in(home.path(), "jolly-mute-magma");
    let project = project("jolly-mute-magma", path);
    let mut stores = home.stores();
    block_on(async {
        home.stores().open(&project).await.unwrap();
        stores
            .open_existing(&project)
            .await
            .unwrap()
            .expect("found");
        stores.open(&project).await.unwrap();
    });
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

/// A database older than this build: one table (`older`) committed, and
/// none of annals-store-sql's migrations applied. Made with Python's
/// sqlite3: `PRAGMA page_size=512; CREATE TABLE older (x)`.
const OLDER: &[u8] = include_bytes!("../testdata/older.db");

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
    let annals::next::StoreError::Unavailable(report) = &error else {
        panic!("unavailable: {error:?}");
    };
    assert_eq!(report.code(), "annals-store-sql-unmigrated");
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
    assert!(
        matches!(error, annals::next::StoreError::Unavailable(_)),
        "{error:?}"
    );
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

// ------------------------------------------------------------ watching

/// A stream of its own for each `n`.
fn stream(n: u8) -> Cover {
    Cover {
        domain: "tasks".to_string(),
        root: Some(Uuid::from_bytes([n; 16])),
        edition: None,
    }
}

#[test]
fn test_a_token_where_there_is_no_database_is_none_and_creates_nothing() {
    let home = Home::new();
    let mut stores = home.stores();
    let project = project("tpyo", store_in(home.path(), "tpyo"));
    let token = block_on(stores.change_token(&project)).unwrap();
    assert_eq!(token, None);
    assert!(!home.exists(&home.path().join(".ctxloom")), "nothing made");
    assert!(stores.held.is_none());
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
        assert!(!watcher.held.as_ref().unwrap().writable, "opened to read");
        let again = watcher.change_token(&project).await.unwrap();
        assert_eq!(again, Some(first), "nothing committed");

        let store = writer.open(&project).await.unwrap();
        store.append(&stream(1), 0, &[event(0, 1)]).await.unwrap();
        let moved = watcher.change_token(&project).await.unwrap().unwrap();
        assert_ne!(moved, first, "another handle's commit moves it");
        assert_eq!(moved.handle, first.handle, "through the one handle");
        let still = watcher.change_token(&project).await.unwrap();
        assert_eq!(still, Some(moved));
    });
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
        home.remove(&path);
        let held = stores.open_existing(&project).await.unwrap();
        assert!(
            held.is_some(),
            "the handle that answered, though the file is gone"
        );
    });
}

#[test]
fn test_a_store_opened_again_answers_a_token_of_another_handle() {
    let home = Home::new();
    let project = project(
        "jolly-mute-magma",
        store_in(home.path(), "jolly-mute-magma"),
    );
    let mut stores = home.stores();
    block_on(async {
        home.stores().open(&project).await.unwrap();
        let read = stores.change_token(&project).await.unwrap().unwrap();
        // Opened again to write: a new connection, counting afresh.
        stores.open(&project).await.unwrap();
        let written = stores.change_token(&project).await.unwrap().unwrap();
        assert_ne!(read.handle, written.handle);
        assert_ne!(read, written);
        let held = stores.change_token(&project).await.unwrap().unwrap();
        assert_eq!(held.handle, written.handle, "held after");
    });
}

#[test]
fn test_the_token_future_is_send_so_a_server_can_spawn_it() {
    fn is_send<T: Send>(_: &T) {}
    let home = Home::new();
    let project = project("jolly-mute-magma", PathBuf::from("/nowhere/state.db"));
    let mut stores = home.stores();
    is_send(&stores.change_token(&project));
}

// ------------------------------------------------------------ the check

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
            rules_version: "v1".to_string(),
            ..EventMeta::default()
        }),
        sequence,
    }
}

/// A database at `path` holding one stream of two events, closed, so the
/// file holds everything (no write-ahead log left beside it). The store's
/// connection closes on its own thread after the store is dropped, and the
/// last connection's close checkpoints the log into the file and removes
/// it: that removal is waited for, up to a deadline, as the sign the file
/// is whole.
fn written(home: &Home, path: &Path) {
    let stream = Cover {
        domain: "tasks".to_string(),
        root: Some(Uuid::from_bytes([1; 16])),
        edition: None,
    };
    let mut stores = home.stores();
    block_on(async {
        let store = stores
            .open(&project("p", path.to_path_buf()))
            .await
            .unwrap();
        store
            .append(&stream, 0, &[event(0, 1), event(1, 2)])
            .await
            .unwrap();
    });
    drop(stores);
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
fn test_a_damaged_copy_is_reported_as_the_stores_damage_and_left_as_it_was() {
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
    assert_eq!(damage.code(), "annals-store-sql-damaged");
    let args = damage.args();
    let first = args
        .iter()
        .find(|(name, _)| *name == "first")
        .map(|(_, a)| a);
    assert!(
        matches!(first, Some(annals_diagnostic::Arg::Str(p)) if p.contains("events_by_command")),
        "{args:?}"
    );
    assert_eq!(home.read(&copy), before, "never repaired");
    assert_eq!(
        home.stores().check(&path),
        Ok(Checked::Intact),
        "the original is sound"
    );
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
    assert_eq!(damage.code(), "annals-store-sql-damaged");
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

#[test]
fn test_a_check_that_cannot_run_fails_with_the_stores_error() {
    let home = Home::new();
    // A directory where the database file goes.
    let path = home.path().join("state.db");
    home.mkdir(&path);
    let failed = home.stores().check(&path).unwrap_err();
    assert_eq!(failed.code(), "annals-next-store-unavailable");
    let cause = failed.args().into_iter().find(|(name, _)| *name == "error");
    assert!(
        matches!(&cause, Some((_, annals_diagnostic::Arg::Nested(c))) if c.code() == "annals-store-sql-unavailable"),
        "{cause:?}"
    );
}

// ------------------------------------------------------------ purging

#[test]
fn test_the_store_purges_a_stream_in_a_unit_of_work_as_a_prune_needs() {
    use annals::next::{AsyncPurge, AsyncUnitOfWork};
    use ports::PurgingStore;

    let home = Home::new();
    let project = project("p", store_in(home.path(), "p"));
    let stream = Cover {
        domain: "tasks".to_string(),
        root: Some(Uuid::from_bytes([1; 16])),
        edition: None,
    };
    let mut stores = home.stores();
    block_on(async {
        let store = stores.open(&project).await.unwrap();
        store
            .append(&stream, 0, &[event(0, 1), event(1, 2)])
            .await
            .unwrap();
        let mut unit = store.begin_purging().await;
        assert_eq!(unit.purge(&stream).await.unwrap(), 2, "events deleted");
        unit.commit().await.unwrap();
        assert_eq!(store.read_from(&stream, 0).await.unwrap(), None);
        assert_eq!(
            store.append(&stream, 0, &[event(0, 3)]).await.unwrap(),
            1,
            "the stream starts again from expected 0"
        );
    });
}
