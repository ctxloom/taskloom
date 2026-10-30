//! The wiring's contract: how the binary (one service per command), a
//! server (one per call, on threads and spawned futures) and ctxloom
//! build the use cases from it, over a home and
//! project on a filesystem of the test's own (`Rooted`), never a real one.

use std::ffi::OsString;
use std::fs::File;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

crate::english_for!(&[crate::EN_US.1]);

use angzarr_embedded_diagnostic::Diagnostic;
use file_system::{CwdError, Entry, Meta, Open, Perms, Rooted};
use os::Mode;
use tasks_application::{
    ImportRequest, ImportService, ListQuery, NewTask, PermitChange, PermitService, Reach,
    TaskService, WatchRequest, WatchService,
};

use super::*;

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(future)
}

/// The home, on the test's filesystem (each of these is spelled from the
/// host's top by [`os::fixture`]).
const HOME: &str = "/home/u";

/// The project directory, on the test's filesystem.
const PROJECT: &str = "/project";

/// What the host says of a file that is not there (os error 2).
fn no_such_file() -> &'static str {
    match os::host().flavor() {
        os::Flavor::Unix => "No such file or directory (os error 2)",
        os::Flavor::Windows => "The system cannot find the file specified. (os error 2)",
    }
}

/// An isolated home and project directory: `/home/u` and `/project` on a
/// filesystem of the test's own, removed when this is dropped.
struct Dirs {
    owner: Rooted,
}

impl Dirs {
    fn new() -> Dirs {
        let owner = Rooted::temp().unwrap();
        for dir in [HOME, PROJECT] {
            owner.create_dir_all(&os::fixture(dir), Mode::DIR).unwrap();
        }
        Dirs { owner }
    }

    /// The test's filesystem, working in `dir` (the interim rule: a
    /// filesystem given works in the builder's working directory).
    fn fs_in(&self, dir: &str) -> Arc<dyn Fs> {
        Arc::new(Rooted::new(self.owner.root(), &os::fixture(dir)).unwrap())
    }

    /// The wiring in `dir`, under the home, on the test's filesystem, and
    /// nothing else.
    fn builder_in(&self, dir: &str) -> Builder {
        Builder::on(self.fs_in(dir)).home(os::fixture(HOME))
    }

    /// The wiring in the project directory.
    fn taskloom(&self) -> Taskloom {
        self.builder_in(PROJECT).build().unwrap()
    }

    /// `bytes` written at `path` on the test's filesystem, its directory
    /// made, with `mode`.
    fn write(&self, path: &str, bytes: &[u8], mode: Mode) {
        let path = &os::fixture(path);
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

    // Not recorded: the host's syscalls, not a filesystem call.
    fn os(&self) -> &dyn os::Os {
        self.inner.os()
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

// A watch on one service sees what another service wrote.
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
    assert!(!block_on(watcher.poll(&mut watch)).unwrap(), "nothing yet");
    block_on(taskloom.tasks().add(new_task("second"))).unwrap();
    assert!(block_on(watcher.poll(&mut watch)).unwrap(), "the add seen");
    assert!(!block_on(watcher.poll(&mut watch)).unwrap(), "once");
}

// CTXLOOM_PROJECT_ID, given to the builder, beats the working directory's
// project, as it does in the binary.
#[test]
fn test_a_pinned_project_beats_the_working_directorys() {
    let dirs = Dirs::new();
    let there = block_on(dirs.taskloom().tasks().add(new_task("there")))
        .unwrap()
        .project;
    dirs.owner
        .create_dir_all(&os::fixture("/elsewhere"), Mode::DIR)
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
        Rooted::new(dirs.owner.root(), &os::fixture(PROJECT)).unwrap(),
    ));
    let fs: Arc<dyn Fs> = Arc::clone(&recording) as Arc<dyn Fs>;
    let taskloom = Builder::on(fs).home(os::fixture(HOME)).build().unwrap();
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

// The filesystem given is the one the wiring holds.
#[test]
fn test_the_filesystem_given_is_the_one_the_wiring_holds() {
    let dirs = Dirs::new();
    let given = dirs.fs_in(PROJECT);
    let taskloom = Builder::on(Arc::clone(&given))
        .home(os::fixture(HOME))
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
// what needs it: the wiring is built, `import` runs, and
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
    let taskloom = Builder::on(fs)
        .home(os::fixture(HOME))
        .build()
        .expect("built with no working directory");
    let report = block_on(taskloom.importer().import(ImportRequest::default())).unwrap();
    assert!(report.complete(), "{report:?}");
    let refused = block_on(taskloom.tasks().list(ListQuery::default())).unwrap_err();
    assert_eq!(refused.code(), "project-registry-no-cwd");
    assert_eq!(
        catalog(&[]).unwrap().render(&refused).unwrap(),
        format!("the working directory cannot be read: {}", no_such_file())
    );
}

// The configuration reads the UTF-8 variables; one that is not is
// skipped, not refused, and the rest still read.
#[test]
fn test_the_configuration_skips_a_variable_that_is_not_utf8() {
    let dirs = Dirs::new();
    let wide = dirs
        .builder_in(PROJECT)
        .vars(vec![
            ("TASKLOOM_CONFIG_NOT_UTF8".into(), os::not_unicode("")),
            (
                "TASKLOOM_CONFIG_TAG_SCHEMA".into(),
                r#"tagma.range:"triage:level"="1,9""#.into(),
            ),
        ])
        .build()
        .unwrap();
    assert!(add_level_seven(&wide).is_ok());
}

/// Every diagnostic the wiring can raise: its own, the runtime's, the use
/// cases', and every driven and platform adapter's.
fn wired_samples() -> Vec<Box<dyn Diagnostic>> {
    [
        crate::diagnostic_samples(),
        angzarr_embedded::diagnostic_samples(),
        tasks_application::diagnostic_samples(),
        project_application::diagnostic_samples(),
        project_registry::diagnostic_samples(),
        task_store_sqlite::diagnostic_samples(),
        jsonl_task_log::diagnostic_samples(),
        tasks_config::diagnostic_samples(),
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

// The catalogs move with the runtime: the wiring's holds
// angzarr-embedded's, the angzarr-ports errors it forwards, and
// angzarr-store-sqlite's, so every key the runtime and its store raise
// renders: the store's causes nested in angzarr-ports' errors, and its
// damage report.

/// angzarr-ports' args as angzarr-embedded-diagnostic's report holds them.
fn ported(
    code: &'static str,
    args: Vec<(&'static str, angzarr_ports::Arg)>,
) -> angzarr_embedded_diagnostic::Report {
    args.into_iter().fold(
        angzarr_embedded_diagnostic::Report::new(code),
        |report, (name, arg)| match arg {
            angzarr_ports::Arg::Uint(n) => report.arg(name, n),
            angzarr_ports::Arg::Str(s) => report.arg(name, s),
            angzarr_ports::Arg::Cause(cause) => report.arg(
                name,
                angzarr_embedded_diagnostic::Arg::nested(&ported(cause.code, cause.args)),
            ),
        },
    )
}

#[test]
fn test_the_catalog_renders_every_key_angzarr_embedded_and_its_store_raise() {
    let catalog = catalog(&[]).unwrap();
    let runtime = angzarr_embedded::diagnostic_samples();
    let store = angzarr_store_sqlite::causes()
        .into_iter()
        .map(|cause| ported(cause.code, cause.args));
    let damage = angzarr_store_sqlite::damage_sample();
    let damage = ported(damage.code(), damage.args());
    let mut unrendered: Vec<String> = runtime
        .iter()
        .map(|d| angzarr_embedded_diagnostic::Report::of(&**d))
        .chain(store.chain([damage]))
        .filter_map(|d| {
            catalog
                .render(&d)
                .err()
                .map(|e| format!("{}: {e:?}", d.code()))
        })
        .collect();
    unrendered.dedup();
    assert!(unrendered.is_empty(), "no message: {unrendered:#?}");
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

// CTXLOOM_ROOT, given to the builder, is the work root: its config is the
// one read, wherever the working directory is.
#[test]
fn test_a_root_given_is_the_work_root_the_configuration_reads() {
    let dirs = Dirs::new();
    dirs.write(
        "/project/.taskloom/config.yaml",
        b"tag_schema:\n  - 'tagma.range:\"triage:level\"=\"1,9\"'\n",
        Mode::FILE,
    );
    dirs.owner
        .create_dir_all(&os::fixture("/elsewhere"), Mode::DIR)
        .unwrap();
    assert!(
        add_level_seven(&dirs.builder_in("/elsewhere").build().unwrap()).is_err(),
        "elsewhere's work root has no config"
    );
    let rooted = dirs
        .builder_in("/elsewhere")
        .root(os::fixture(PROJECT))
        .build()
        .unwrap();
    assert!(add_level_seven(&rooted).is_ok());
}

// What the process gives: HOME, CTXLOOM_ROOT and CTXLOOM_PROJECT_ID from
// its variables, which are kept as given; and a working directory that
// cannot be read is a filesystem that says so, not a refusal.
#[test]
fn test_the_process_is_read_from_its_variables_and_working_directory() {
    let vars: Vec<(OsString, OsString)> = vec![
        (
            os::host().flavor().home_var().into(),
            os::fixture("/h").into(),
        ),
        ("CTXLOOM_ROOT".into(), os::fixture("/r").into()),
        ("CTXLOOM_PROJECT_ID".into(), "jolly-mute-magma".into()),
        ("PATH".into(), os::fixture("/bin").into()),
    ];
    let builder = Builder::from_env(Ok(os::fixture(PROJECT)), vars.clone());
    assert_eq!(builder.home, Some(os::fixture("/h")));
    assert_eq!(builder.root, Some(os::fixture("/r")));
    assert_eq!(builder.project_id.as_deref(), Some("jolly-mute-magma"));
    assert_eq!(builder.vars, vars);
    let taskloom = builder.build().unwrap();
    assert_eq!(taskloom.fs().cwd(), Ok(os::fixture(PROJECT).as_path()));
    let gone = Builder::from_env(Err(io::Error::from_raw_os_error(2)), Vec::new())
        .build()
        .unwrap();
    assert_eq!(
        gone.fs().cwd(),
        Err(CwdError {
            reason: no_such_file().to_string()
        })
    );
}

// Tag permits, wired: the work root's config refuses a tag it does not
// permit; `permit` writes it there, keeping the human's comment; and the
// next service, reading the file again, admits it.
#[test]
fn test_a_projects_permits_refuse_a_tag_until_it_is_permitted() {
    let dirs = Dirs::new();
    dirs.write(
        "/project/.taskloom/config.yaml",
        b"# Our vocabulary.\ntag_permits:\n  tags:\n    - urgent\n",
        Mode::FILE,
    );
    let taskloom = dirs.taskloom();
    let task = || NewTask {
        tags: vec!["area:cli".to_string()],
        ..new_task("Ship it")
    };
    let error = block_on(taskloom.tasks().add(task())).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-not-permitted");
    let permitted = block_on(taskloom.tasks().permit(PermitChange {
        permit: "area:cli".to_string(),
    }))
    .unwrap();
    assert!(permitted.changed);
    let file = dirs
        .owner
        .read_to_string(&os::fixture("/project/.taskloom/config.yaml"))
        .unwrap();
    assert!(file.starts_with("# Our vocabulary.\n"), "{file}");
    assert!(file.contains("area:"), "{file}");
    block_on(taskloom.tasks().add(task())).unwrap();
}

/// taskloom's proposed tag permits (tag-vocabulary.proposal.md, section 5,
/// adapted to the rulings): a fixture, not this repository's live config.
const TASKLOOM_PERMITS: &[u8] = include_bytes!("../fixtures/taskloom-tag-permits.yaml");

// Tag permits end to end, under taskloom's own proposed vocabulary: an
// invented area (the agent's `area:host-hardening`, the problem permits
// exist to fix) is refused, naming the command a person would run; a
// declared area is admitted, beside a closed namespace's key, a factual
// flag from the closed triage: list, an open namespace's key, and the
// schema's declared targets (blind-gate among them), none of which the
// fixture lists. The read-only list says what the file says.
#[test]
fn test_taskloom_s_proposed_permits_refuse_an_invented_area_and_admit_a_declared_one() {
    let dirs = Dirs::new();
    dirs.write(
        "/project/.taskloom/config.yaml",
        TASKLOOM_PERMITS,
        Mode::FILE,
    );
    let taskloom = dirs.taskloom();
    let tagged = |tags: &[&str]| NewTask {
        tags: tags.iter().map(|t| t.to_string()).collect(),
        ..new_task("Harden the host")
    };
    let error = block_on(taskloom.tasks().add(tagged(&["area:host-hardening"]))).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-not-permitted");
    assert!(
        catalog(&[])
            .unwrap()
            .message(&error)
            .contains("`taskloom permit area:host-hardening`"),
        "{error}"
    );
    let written = block_on(taskloom.tasks().add(tagged(&[
        "area:platform",
        "platform:windows",
        "triage:security",
        "relates:crispy-woof",
        "triage:kind=defect",
        "triage:level=2",
        "triage:blind-gate=ci",
    ])))
    .unwrap();
    assert_eq!(written.task.tags.len(), 7);
    let listed = block_on(taskloom.tasks().permits()).unwrap();
    let permits = listed.permits.expect("the fixture declares permits");
    assert_eq!(permits.namespaces.len(), 11);
    assert!(permits.flat.is_empty());
    assert!(listed
        .schema_targets
        .contains(&"triage:blind-gate".to_string()));
}
