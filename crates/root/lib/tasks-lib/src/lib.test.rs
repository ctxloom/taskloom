//! The wiring's contract: how the binary (one service per command), the
//! MCP server and a gRPC server (one per call, on threads and spawned
//! futures) and ctxloom build the use cases from it, over a home and
//! project on a filesystem of the test's own (`Rooted`), never a real one.

use std::ffi::OsString;
use std::fs::File;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use annals_diagnostic::Diagnostic;
use file_system::{CwdError, Entry, Meta, Mode, Open, Perms, Rooted};
use tasks_application::{
    Changed, ImportRequest, ImportService, ListQuery, NewTask, PruneService, Reach, TaskService,
    WatchRequest, WatchService,
};

use super::*;

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(future)
}

/// The home, on the test's filesystem.
const HOME: &str = "/home/u";

/// The project directory, on the test's filesystem.
const PROJECT: &str = "/project";

/// An isolated home and project directory: `/home/u` and `/project` on a
/// filesystem of the test's own, removed when this is dropped.
struct Dirs {
    owner: Rooted,
}

impl Dirs {
    fn new() -> Dirs {
        let owner = Rooted::temp().unwrap();
        for dir in [HOME, PROJECT] {
            owner.create_dir_all(Path::new(dir), Mode::DIR).unwrap();
        }
        Dirs { owner }
    }

    /// The test's filesystem, working in `dir` (the interim rule: a
    /// filesystem given works in the builder's working directory).
    fn fs_in(&self, dir: &str) -> Arc<dyn Fs> {
        Arc::new(Rooted::new(self.owner.root(), Path::new(dir)).unwrap())
    }

    /// The wiring in `dir`, under the home, on the test's filesystem, and
    /// nothing else.
    fn builder_in(&self, dir: &str) -> Builder {
        Builder::new(dir).home(HOME).fs(self.fs_in(dir))
    }

    /// The wiring in the project directory.
    fn taskloom(&self) -> Taskloom {
        self.builder_in(PROJECT).build().unwrap()
    }

    /// `bytes` written at `path` on the test's filesystem, its directory
    /// made, with `mode`.
    fn write(&self, path: &str, bytes: &[u8], mode: Mode) {
        let path = Path::new(path);
        self.owner
            .create_dir_all(path.parent().unwrap(), Mode::DIR)
            .unwrap();
        self.owner
            .write_atomic(path, bytes, Perms::Exact(mode))
            .unwrap();
    }
}

/// A filesystem that records every call made on it, then makes it on
/// `inner`.
#[derive(Debug)]
struct Recording {
    inner: Rooted,
    calls: Mutex<Vec<String>>,
}

impl Recording {
    fn new(inner: Rooted) -> Recording {
        Recording {
            inner,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn record(&self, call: &str, path: &Path) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{call} {}", path.display()));
    }

    /// Every call made so far, in order.
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Fs for Recording {
    // Not recorded: it asks the OS nothing, and is what the filesystem was
    // made with.
    fn cwd(&self) -> Result<&Path, CwdError> {
        self.inner.cwd()
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.record("read", path);
        self.inner.read(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.record("read_to_string", path);
        self.inner.read_to_string(path)
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8], perms: Perms) -> io::Result<()> {
        self.record("write_atomic", path);
        self.inner.write_atomic(path, bytes, perms)
    }

    fn open(&self, path: &Path, how: Open) -> io::Result<File> {
        self.record("open", path);
        self.inner.open(path, how)
    }

    fn create_dir_all(&self, path: &Path, mode: Mode) -> io::Result<()> {
        self.record("create_dir_all", path);
        self.inner.create_dir_all(path, mode)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.record("remove_file", path);
        self.inner.remove_file(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        self.record("remove_dir_all", path);
        self.inner.remove_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.record("rename", from);
        self.inner.rename(from, to)
    }

    fn set_permissions(&self, path: &Path, mode: Mode) -> io::Result<()> {
        self.record("set_permissions", path);
        self.inner.set_permissions(path, mode)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<Entry>> {
        self.record("read_dir", path);
        self.inner.read_dir(path)
    }

    fn metadata(&self, path: &Path) -> io::Result<Meta> {
        self.record("metadata", path);
        self.inner.metadata(path)
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<Meta> {
        self.record("symlink_metadata", path);
        self.inner.symlink_metadata(path)
    }

    fn exists(&self, path: &Path) -> io::Result<bool> {
        self.record("exists", path);
        self.inner.exists(path)
    }

    fn absolute(&self, path: &Path) -> io::Result<PathBuf> {
        self.record("absolute", path);
        self.inner.absolute(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.record("canonicalize", path);
        self.inner.canonicalize(path)
    }

    fn symlink_dir(&self, target: &Path, link: &Path) -> io::Result<()> {
        self.record("symlink_dir", link);
        self.inner.symlink_dir(target, link)
    }

    fn read_link(&self, link: &Path) -> io::Result<PathBuf> {
        self.record("read_link", link);
        self.inner.read_link(link)
    }

    fn make_temp_dir(&self, parent: &Path, prefix: &str) -> io::Result<PathBuf> {
        self.record("make_temp_dir", parent);
        self.inner.make_temp_dir(parent, prefix)
    }

    fn real_path(&self, path: &Path) -> io::Result<PathBuf> {
        self.record("real_path", path);
        self.inner.real_path(path)
    }

    fn same_file(&self, file: &File, path: &Path) -> io::Result<bool> {
        self.record("same_file", path);
        self.inner.same_file(file, path)
    }
}

fn new_task(text: &str) -> NewTask {
    NewTask {
        text: text.to_string(),
        ..NewTask::default()
    }
}

fn texts(listing: &tasks_application::Listing) -> Vec<String> {
    let mut texts: Vec<String> = listing.tasks().map(|t| t.text.clone()).collect();
    texts.sort();
    texts
}

/// What a server's factory must be: a maker of services that crosses
/// threads and outlives the wiring, each service every use case a frontend
/// calls, its futures `Send`. MCP's `TaskTools::new` and a gRPC server ask
/// no more.
fn served<F, S>(factory: F) -> F
where
    F: Fn() -> S + Send + Sync + 'static,
    S: TaskService + PruneService + WatchService + Send + 'static,
{
    factory
}

// The binary's path: one service per command, each resolving the project
// the working directory is, so two commands share a store.
#[test]
fn test_a_task_added_by_one_service_is_listed_by_the_next() {
    let dirs = Dirs::new();
    let taskloom = dirs.taskloom();
    let written = block_on(taskloom.tasks().add(new_task("Ship it"))).unwrap();
    let listing = block_on(taskloom.tasks().list(ListQuery::default())).unwrap();
    assert_eq!(texts(&listing), ["Ship it"]);
    assert_eq!(listing.reach, Reach::Project(written.project));
}

// MCP's path: each call on a service the factory makes for it, on a
// blocking thread of its own; the factory outlives the wiring it came from.
#[test]
fn test_the_factory_makes_a_service_per_call_on_threads_of_their_own() {
    let dirs = Dirs::new();
    let factory = Arc::new(served(dirs.taskloom().services()));
    let calls: Vec<_> = (0..4)
        .map(|n| {
            let factory = Arc::clone(&factory);
            std::thread::spawn(move || block_on(factory().add(new_task(&format!("call {n}")))))
        })
        .collect();
    for call in calls {
        call.join().unwrap().unwrap();
    }
    let listing = block_on(factory().list(ListQuery::default())).unwrap();
    assert_eq!(texts(&listing), ["call 0", "call 1", "call 2", "call 3"]);
}

// A gRPC server's path: a call's future is spawned on the server's
// runtime, so it must be `Send` and own its service.
#[test]
fn test_a_calls_future_can_be_spawned_on_a_runtime() {
    let dirs = Dirs::new();
    let factory = served(dirs.taskloom().services());
    let added = block_on(async move {
        tokio::spawn(async move { factory().add(new_task("spawned")).await })
            .await
            .expect("the call ran")
    });
    assert_eq!(added.unwrap().task.text, "spawned");
}

// A change event is a typed value: a watch on one service sees what
// another service wrote.
#[test]
fn test_a_watch_sees_what_another_service_writes() {
    let dirs = Dirs::new();
    let taskloom = dirs.taskloom();
    let mut watcher = taskloom.tasks();
    assert!(
        block_on(watcher.watch(WatchRequest::default()))
            .unwrap()
            .is_none(),
        "no project yet: a watch binds none"
    );
    block_on(taskloom.tasks().add(new_task("first"))).unwrap();
    let mut watch = block_on(watcher.watch(WatchRequest::default()))
        .unwrap()
        .expect("the project the add minted");
    block_on(taskloom.tasks().add(new_task("second"))).unwrap();
    let changes = block_on(watcher.poll(&mut watch)).unwrap();
    let texts: Vec<_> = changes
        .iter()
        .map(|c| match c {
            Changed::Task(task) => task.text.clone(),
            Changed::Removed { harp_id } => format!("removed {harp_id}"),
        })
        .collect();
    assert_eq!(texts, ["second"]);
}

// CTXLOOM_PROJECT_ID, given to the builder, beats the working directory's
// project, as it does in the binary and the MCP server.
#[test]
fn test_a_pinned_project_beats_the_working_directorys() {
    let dirs = Dirs::new();
    let there = block_on(dirs.taskloom().tasks().add(new_task("there")))
        .unwrap()
        .project;
    dirs.owner
        .create_dir_all(Path::new("/elsewhere"), Mode::DIR)
        .unwrap();
    let pinned = dirs
        .builder_in("/elsewhere")
        .project_id(there.id.clone())
        .build()
        .unwrap();
    let written = block_on(pinned.tasks().add(new_task("pinned"))).unwrap();
    assert_eq!(written.project.id, there.id);
}

/// A task tagged `triage:level=7`, out of the default schema's range
/// (1-5), added on `taskloom`.
fn add_level_seven(
    taskloom: &Taskloom,
) -> Result<tasks_application::Written, tasks_application::AppError> {
    block_on(taskloom.tasks().add(NewTask {
        tags: vec!["triage:level=7".to_string()],
        ..new_task("Ship it")
    }))
}

// The variables given are the configuration's environment layer.
#[test]
fn test_the_configuration_reads_the_variables_given() {
    let dirs = Dirs::new();
    assert!(
        add_level_seven(&dirs.taskloom()).is_err(),
        "the default range, 1-5"
    );
    let wide = dirs
        .builder_in(PROJECT)
        .vars(vec![(
            "TASKLOOM_CONFIG_TAG_SCHEMA".into(),
            r#"tagma.range:"triage:level"="1,9""#.into(),
        )])
        .build()
        .unwrap();
    assert!(add_level_seven(&wide).is_ok());
}

// The `--config-set` overrides, bound once at composition, are the
// configuration's last layer.
#[test]
fn test_the_configuration_applies_the_overrides_given() {
    let dirs = Dirs::new();
    assert!(
        add_level_seven(&dirs.taskloom()).is_err(),
        "the default range, 1-5"
    );
    let wide = dirs
        .builder_in(PROJECT)
        .config_set(vec![
            r#"tag_schema=tagma.range:"triage:level"="1,9""#.to_string()
        ])
        .build()
        .unwrap();
    assert!(add_level_seven(&wide).is_ok());
}

// Building the tasks use cases does no I/O: no work root is searched for
// and no project resolved until a call needs one, so `version` and
// `loadout` touch nothing.
#[test]
fn test_building_the_tasks_use_cases_touches_no_file() {
    let dirs = Dirs::new();
    let recording = Arc::new(Recording::new(
        Rooted::new(dirs.owner.root(), Path::new(PROJECT)).unwrap(),
    ));
    let fs: Arc<dyn Fs> = Arc::clone(&recording) as Arc<dyn Fs>;
    let taskloom = Builder::new(PROJECT).home(HOME).fs(fs).build().unwrap();
    let _tasks = taskloom.tasks();
    assert_eq!(recording.calls(), Vec::<String>::new());
}

// Import only on request, over the home's JSONL task logs, into the stores
// the tasks use cases read.
#[test]
fn test_the_importer_reads_the_homes_logs_into_the_stores_tasks_reads() {
    let dirs = Dirs::new();
    dirs.write(
        "/home/u/.ctxloom/tasks/jolly-mute-magma.jsonl",
        concat!(
            r#"{"op":"add","task":"brisk-heron","text":"Write the tour","status":"To Do","#,
            r#""session":"calm-otter","ts":"2026-03-01T10:00:00.123456789Z"}"#,
            "\n"
        )
        .as_bytes(),
        Mode::FILE,
    );
    let taskloom = dirs.taskloom();
    let report = block_on(taskloom.importer().import(ImportRequest::default())).unwrap();
    assert!(report.complete(), "{report:?}");
    let listing = block_on(taskloom.tasks().list(ListQuery {
        project: Some("jolly-mute-magma".to_string()),
        ..ListQuery::default()
    }))
    .unwrap();
    assert_eq!(texts(&listing), ["Write the tour"]);
}

// The project use cases run over the same registry and stores.
#[test]
fn test_projects_shows_the_project_an_add_minted() {
    let dirs = Dirs::new();
    let taskloom = dirs.taskloom();
    let written = block_on(taskloom.tasks().add(new_task("Ship it"))).unwrap();
    let shown = taskloom.projects().show(None).unwrap();
    assert_eq!(shown.store, Some(written.project.store));
}

// A registration's command is resolved on the PATH among the variables
// given, and on no other.
#[test]
fn test_registrations_resolve_the_server_on_the_path_given() {
    let dirs = Dirs::new();
    dirs.write("/bin/taskloom", b"#!/bin/sh\n", Mode::from_bits(0o755));
    let on_path = dirs
        .builder_in(PROJECT)
        .vars(vec![("PATH".into(), "/bin".into())])
        .build()
        .unwrap();
    assert!(
        on_path
            .registrations()
            .check(Path::new(PROJECT))
            .unwrap()
            .resolvable
    );
    assert!(
        !dirs
            .taskloom()
            .registrations()
            .check(Path::new(PROJECT))
            .unwrap()
            .resolvable,
        "no PATH among the variables given: the process's is not read"
    );
}

// The filesystem given is the one the wiring holds.
#[test]
fn test_the_filesystem_given_is_the_one_the_wiring_holds() {
    let dirs = Dirs::new();
    let given = dirs.fs_in(PROJECT);
    let taskloom = Builder::new(PROJECT)
        .home(HOME)
        .fs(Arc::clone(&given))
        .build()
        .unwrap();
    assert!(Arc::ptr_eq(taskloom.fs(), &given));
    assert!(
        Arc::ptr_eq(taskloom.clone().fs(), &given),
        "a clone shares it"
    );
}

// With no filesystem given, the real one, which refuses a working
// directory that is not absolute: a diagnostic of this crate's, which the
// catalog renders.
#[test]
fn test_a_relative_working_directory_is_refused_as_a_catalogued_diagnostic() {
    let refused = Builder::new("relative").build().unwrap_err();
    assert_eq!(
        refused,
        BuildError::RelativeCwd {
            cwd: "relative".to_string()
        }
    );
    assert_eq!(refused.code(), "tasks-lib-relative-cwd");
    assert_eq!(
        catalog(&[]).unwrap().render(&refused).unwrap(),
        "the working directory is not an absolute path: relative"
    );
}

// A working directory that cannot be read (it was deleted) fails only
// what needs it: the wiring is built, `manage` and `import` run, and
// resolving the working directory's project is refused with the
// catalogued error. The filesystem holds that it has none.
#[test]
fn test_an_unreadable_working_directory_fails_only_what_needs_it() {
    let dirs = Dirs::new();
    dirs.write(
        "/home/u/.ctxloom/tasks/jolly-mute-magma.jsonl",
        concat!(
            r#"{"op":"add","task":"brisk-heron","text":"Write the tour","status":"To Do","#,
            r#""session":"calm-otter","ts":"2026-03-01T10:00:00.123456789Z"}"#,
            "\n"
        )
        .as_bytes(),
        Mode::FILE,
    );
    let gone = io::Error::from_raw_os_error(2);
    let fs: Arc<dyn Fs> = Arc::new(Rooted::without_cwd(dirs.owner.root(), gone).unwrap());
    let taskloom = Builder::new(PROJECT)
        .home(HOME)
        .fs(fs)
        .build()
        .expect("built with no working directory");
    taskloom
        .registrations()
        .check(Path::new(PROJECT))
        .expect("manage needs no working directory");
    let report = block_on(taskloom.importer().import(ImportRequest::default())).unwrap();
    assert!(report.complete(), "{report:?}");
    let refused = block_on(taskloom.tasks().list(ListQuery::default())).unwrap_err();
    assert_eq!(refused.code(), "project-registry-no-cwd");
    assert_eq!(
        catalog(&[]).unwrap().render(&refused).unwrap(),
        "the working directory cannot be read: No such file or directory (os error 2)"
    );
}

// The variables are taken as given: PATH is read without loss, though it
// is not UTF-8.
#[cfg(unix)]
#[test]
fn test_a_path_that_is_not_utf8_is_read_without_loss() {
    use std::os::unix::ffi::OsStringExt;

    let dirs = Dirs::new();
    let bin = OsString::from_vec(b"/b\xffn".to_vec());
    let server = PathBuf::from(&bin).join("taskloom");
    dirs.owner
        .create_dir_all(Path::new(&bin), Mode::DIR)
        .unwrap();
    dirs.owner
        .write_atomic(
            &server,
            b"#!/bin/sh\n",
            Perms::Exact(Mode::from_bits(0o755)),
        )
        .unwrap();
    let on_path = dirs
        .builder_in(PROJECT)
        .vars(vec![("PATH".into(), bin)])
        .build()
        .unwrap();
    assert!(
        on_path
            .registrations()
            .check(Path::new(PROJECT))
            .unwrap()
            .resolvable
    );
}

// The configuration reads the UTF-8 variables; one that is not is
// skipped, not refused, and the rest still read.
#[cfg(unix)]
#[test]
fn test_the_configuration_skips_a_variable_that_is_not_utf8() {
    use std::os::unix::ffi::OsStringExt;

    let dirs = Dirs::new();
    let wide = dirs
        .builder_in(PROJECT)
        .vars(vec![
            (
                "TASKLOOM_CONFIG_NOT_UTF8".into(),
                OsString::from_vec(b"\xff".to_vec()),
            ),
            (
                "TASKLOOM_CONFIG_TAG_SCHEMA".into(),
                r#"tagma.range:"triage:level"="1,9""#.into(),
            ),
        ])
        .build()
        .unwrap();
    assert!(add_level_seven(&wide).is_ok());
}

/// Every diagnostic the wiring can raise: its own, annals', the use
/// cases', and every driven and platform adapter's.
fn wired_samples() -> Vec<Box<dyn Diagnostic>> {
    [
        crate::diagnostic_samples(),
        annals::diagnostic_samples(),
        annals_store_sql::diagnostic_samples(),
        tasks_application::diagnostic_samples(),
        project_application::diagnostic_samples(),
        registration_application::diagnostic_samples(),
        project_registry::diagnostic_samples(),
        task_store_sqlite::diagnostic_samples(),
        jsonl_task_log::diagnostic_samples(),
        tasks_config::diagnostic_samples(),
        agent_configs::diagnostic_samples(),
        file_lock::diagnostic_samples(),
        file_system::diagnostic_samples(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[test]
fn test_the_catalog_renders_every_diagnostic_the_wiring_can_raise() {
    let catalog = catalog(&[]).unwrap();
    for sample in wired_samples() {
        assert!(
            catalog.render(&*sample).is_ok(),
            "{} has no message",
            sample.code()
        );
    }
}

// A frontend's own messages are its to add; the wiring knows none.
#[test]
fn test_a_frontend_adds_its_own_messages_to_the_catalog() {
    let samples = project_cli::diagnostic_samples();
    let bare = catalog(&[]).unwrap();
    assert!(samples.iter().all(|s| bare.render(&**s).is_err()));
    let with_cli = catalog(&[project_cli::EN_US]).unwrap();
    for sample in &samples {
        assert!(with_cli.render(&**sample).is_ok(), "{}", sample.code());
    }
}
