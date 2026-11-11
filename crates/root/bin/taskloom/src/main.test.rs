//! Acceptance: the real `taskloom` binary, run as separate processes on one
//! project's SQLite database, loses no write and duplicates no task id.
//!
//! The tests live beside the code (no `tests/` directory), where cargo
//! does not build the binary for them; so the first test to need it builds
//! it, once, with the cargo running these tests, into the same target
//! directory and profile.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use serde_json::Value;

/// The `taskloom` binary, built once per test run.
fn binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        // This test executable is <target>/<profile>/deps/taskloom-<hash>.
        let exe = std::env::current_exe().expect("the test executable");
        let profile_dir = exe
            .parent()
            .and_then(Path::parent)
            .expect("<target>/<profile>");
        let target_dir = profile_dir.parent().expect("<target>");
        let mut build = Command::new(env!("CARGO"));
        build
            .args([
                "build",
                "--quiet",
                "--package",
                "taskloom",
                "--bin",
                "taskloom",
            ])
            .arg("--manifest-path")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .env("CARGO_TARGET_DIR", target_dir);
        if profile_dir.file_name().is_some_and(|p| p == "release") {
            build.arg("--release");
        }
        let status = build.status().expect("cargo runs");
        assert!(status.success(), "building taskloom failed");
        profile_dir.join("taskloom")
    })
}

/// A process with nothing of this one's environment but what the host
/// needs to run one at all ([`os::Flavor::startup_vars`]) and its home, in
/// the variable the host reads it from (HOME; USERPROFILE on Windows).
pub(crate) trait Isolated {
    fn isolated(&mut self, home: impl AsRef<std::ffi::OsStr>) -> &mut Self;

    /// Run in `dir`, which is its work root (CTXLOOM_ROOT), whatever is
    /// above it: a test's directories are in TMPDIR, which may be inside a
    /// checkout (a worktree's target/), and that checkout is no project of
    /// theirs. After [`Isolated::isolated`], which clears the environment.
    /// How a work root is found is project-registry's to test, on a
    /// filesystem of its own.
    fn working_in(&mut self, dir: impl AsRef<Path>) -> &mut Self;
}

impl Isolated for Command {
    fn isolated(&mut self, home: impl AsRef<std::ffi::OsStr>) -> &mut Command {
        let flavor = os::host().flavor();
        self.env_clear();
        for name in flavor.startup_vars() {
            if let Some(value) = std::env::var_os(name) {
                self.env(name, value);
            }
        }
        self.env(flavor.home_var(), home)
    }

    fn working_in(&mut self, dir: impl AsRef<Path>) -> &mut Command {
        let dir = dir.as_ref();
        self.current_dir(dir).env("CTXLOOM_ROOT", dir)
    }
}

/// A directory outside every work root whatever TMPDIR is: the host's top
/// (`/`, or `C:\` on Windows), with no `.git` above it, which a process
/// run there with no CTXLOOM_ROOT only reads.
pub(crate) fn outside_every_work_root() -> &'static Path {
    Path::new(os::host().flavor().top())
}

/// The real filesystem, through the seam: where the binary's processes
/// run, for what a test lays out for them and reads back. Every path a
/// test gives it is absolute; its working directory is this crate's.
pub(crate) fn os() -> std::sync::Arc<dyn file_system::Fs> {
    std::sync::Arc::new(
        file_system::Os::new(os::host(), Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap(),
    )
}

/// A new temporary directory on the real filesystem, for a process the
/// tests run to work in (git and the binary write real paths), removed
/// with everything in it when dropped. A `Rooted` owns it.
pub(crate) struct Temp(file_system::Rooted);

impl Temp {
    pub(crate) fn new() -> Temp {
        Temp(file_system::Rooted::temp().unwrap())
    }

    /// The directory, canonical.
    pub(crate) fn path(&self) -> &Path {
        self.0.root()
    }
}

/// What the tests do on the real filesystem, through the seam ([`os`]),
/// in the shapes `std::fs` gave them.
pub(crate) mod disk {
    use std::io;
    use std::path::{Path, PathBuf};

    use file_system::Perms;

    use os::Mode;

    use super::os;

    pub(crate) fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        os().read(path.as_ref())
    }

    pub(crate) fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        os().read_to_string(path.as_ref())
    }

    /// The file at `path` replaced by `bytes`, its mode kept (0644 when
    /// new); its directory must exist.
    pub(crate) fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        os().write_atomic(path.as_ref(), bytes.as_ref(), Perms::KeepOr(Mode::FILE))
    }

    /// `from`'s bytes written at `to`.
    pub(crate) fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        write(to, read(from)?)
    }

    pub(crate) fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        os().create_dir_all(path.as_ref(), Mode::DIR)
    }

    pub(crate) fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        os().remove_file(path.as_ref())
    }

    pub(crate) fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        os().remove_dir_all(path.as_ref())
    }

    pub(crate) fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        os().rename(from.as_ref(), to.as_ref())
    }

    pub(crate) fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
        os().canonicalize(path.as_ref())
    }

    /// The paths of the directory's entries, sorted.
    pub(crate) fn read_dir(path: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
        let dir = path.as_ref();
        Ok(os()
            .read_dir(dir)?
            .into_iter()
            .map(|entry| dir.join(entry.name))
            .collect())
    }

    /// Whether anything is at `path`, links followed; as `Path::exists`,
    /// what cannot be looked at (a path through a file) is not.
    pub(crate) fn exists(path: impl AsRef<Path>) -> bool {
        os().exists(path.as_ref()).unwrap_or(false)
    }
}

/// An isolated home and project directory.
struct Env {
    home: Temp,
    project: Temp,
    session: &'static str,
}

/// One finished process.
struct Ran {
    ok: bool,
    /// The exit code; -1 when a signal ended the process.
    code: i32,
    stdout: String,
    stderr: String,
}

impl Ran {
    fn of(output: Output) -> Ran {
        Ran {
            ok: output.status.success(),
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", self.stdout))
    }
}

impl Env {
    /// A fresh home and project directory: no registry, no marker, no
    /// store. A test that is not about first sight seeds the project with a
    /// real `add` before any concurrency.
    fn new(session: &'static str) -> Env {
        Env {
            home: Temp::new(),
            project: Temp::new(),
            session,
        }
    }

    /// Run `taskloom args` in the project directory, with only HOME, PATH
    /// and the session in its environment. Safe to call concurrently.
    fn run(&self, args: &[&str]) -> Ran {
        self.run_with(args, &[])
    }

    /// [`Env::run`], with `vars` in its environment too.
    fn run_with(&self, args: &[&str], vars: &[(&str, &str)]) -> Ran {
        self.run_in(self.project.path(), args, vars)
    }

    /// [`Env::run_with`], in `dir`, its work root ([`Isolated::working_in`]).
    fn run_in(&self, dir: &Path, args: &[&str], vars: &[(&str, &str)]) -> Ran {
        let mut command = Command::new(binary());
        command
            .args(args)
            .isolated(self.home.path())
            .working_in(dir);
        self.output(command, vars)
    }

    /// [`Env::run`], outside every work root ([`outside_every_work_root`]).
    fn run_outside(&self, args: &[&str]) -> Ran {
        let mut command = Command::new(binary());
        command
            .args(args)
            .isolated(self.home.path())
            .current_dir(outside_every_work_root());
        self.output(command, &[])
    }

    /// What `command`, given PATH, the session and `vars`, does.
    fn output(&self, mut command: Command, vars: &[(&str, &str)]) -> Ran {
        let output = command
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("CTXLOOM_SESSION_HARP", self.session)
            .envs(vars.iter().copied())
            .output()
            .expect("taskloom runs");
        Ran::of(output)
    }

    /// The final listing, read by one process alone.
    fn list(&self) -> Vec<Value> {
        let ran = self.run(&["tasks", "list", "--format", "json"]);
        assert!(ran.ok, "list failed: {}", ran.stderr);
        ran.json().as_array().expect("an array").clone()
    }

    /// Add `text` alone, before any concurrency: the project is minted, its
    /// store created and migrated. The task's JSON.
    fn seed(&self, text: &str) -> Value {
        let added = self.run(&["tasks", "add", text, "--format", "json"]);
        assert!(added.ok, "seed: {}", added.stderr);
        added.json()
    }

    /// Permit every tag in the project's store (`tagma.permit:"*:*"`),
    /// minting the project and making its store if need be, so a test of
    /// anything but the permits writes whatever tags it names: every store
    /// is an allowlist from when it is made (F6).
    fn permit_every_tag(&self) {
        let ran = self.run(&[
            "manage",
            "schema",
            "declare",
            "--add",
            r#"tagma.permit:"*:*""#,
            "--yes",
            "--format",
            "json",
        ]);
        assert!(ran.ok, "permit every tag: {}", ran.stderr);
    }

    /// The registry's entries, as `project_id` lines.
    fn registered(&self) -> Vec<String> {
        let index = self.home.path().join(".ctxloom/projects/index.yaml");
        disk::read_to_string(index)
            .expect("the registry")
            .lines()
            .filter(|l| {
                l.trim_start()
                    .trim_start_matches("- ")
                    .starts_with("project_id:")
            })
            .map(str::to_string)
            .collect()
    }

    /// The project stores under the home: one, or identities forked.
    fn stores(&self) -> Vec<PathBuf> {
        let projects = self.home.path().join(".ctxloom/projects");
        let mut stores: Vec<PathBuf> = disk::read_dir(projects)
            .expect("the projects directory")
            .into_iter()
            .map(|dir| dir.join("state.db"))
            .filter(|p| disk::exists(p))
            .collect();
        stores.sort();
        stores
    }
}

/// Run `n` processes at once, the `i`th with `args(i)`.
fn concurrently(env: &Env, n: usize, args: impl Fn(usize) -> Vec<String> + Sync) -> Vec<Ran> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..n)
            .map(|i| {
                let args = &args;
                scope.spawn(move || {
                    let args = args(i);
                    let args: Vec<&str> = args.iter().map(String::as_str).collect();
                    env.run(&args)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

fn texts(tasks: &[Value], field: &str) -> Vec<String> {
    tasks
        .iter()
        .map(|t| t[field].as_str().expect(field).to_string())
        .collect()
}

/// A first-sight race: the project is unseeded, so the 40 adds race to
/// mint and bind its id (under the registry's lock) and to create and
/// migrate its database (under SQLite's write lock), as well as to add.
#[test]
fn test_concurrent_adds_at_first_sight_bind_one_project_and_lose_no_task() {
    let env = Env::new("cross-proc-harp");

    let n = 40;
    let ran = concurrently(&env, n, |i| {
        vec![
            "tasks".to_string(),
            "add".to_string(),
            format!("concurrent-task-{i:03}"),
        ]
    });
    for (i, r) in ran.iter().enumerate() {
        assert!(r.ok, "process {i} failed: {}", r.stderr);
    }

    let tasks = env.list();
    assert_eq!(tasks.len(), n, "every add landed");
    let mut harps = texts(&tasks, "harp_id");
    harps.sort();
    harps.dedup();
    assert_eq!(harps.len(), n, "no harp id twice");
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for text in texts(&tasks, "text") {
        *seen.entry(text).or_default() += 1;
    }
    for i in 0..n {
        let text = format!("concurrent-task-{i:03}");
        assert_eq!(seen.get(&text), Some(&1), "{text} exactly once");
    }
    assert!(
        tasks
            .iter()
            .all(|t| t["origin_session"] == "cross-proc-harp"),
        "each stamped with its session"
    );
    assert_eq!(env.stores().len(), 1, "one project, one store");
    assert_eq!(
        env.registered().len(),
        1,
        "one id bound: {:?}",
        env.registered()
    );
}

#[test]
fn test_readers_during_writes_see_whole_listings() {
    let env = Env::new("reader-writer-harp");
    env.seed("rw-seed");

    let (writers, readers) = (20, 10);
    let ran = concurrently(&env, writers + readers, |i| {
        if i < writers {
            vec![
                "tasks".to_string(),
                "add".to_string(),
                format!("rw-task-{i:03}"),
            ]
        } else {
            vec![
                "tasks".to_string(),
                "list".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ]
        }
    });
    let last: BTreeMap<String, Value> = env
        .list()
        .into_iter()
        .map(|t| (t["harp_id"].as_str().expect("harp_id").to_string(), t))
        .collect();
    assert_eq!(last.len(), writers + 1, "every add, and the seed");
    for (i, r) in ran.iter().enumerate() {
        assert!(r.ok, "process {i} failed: {}", r.stderr);
        if i < writers {
            continue;
        }
        assert!(
            !r.stderr.contains("no project here"),
            "reader {i} found the seeded project: {}",
            r.stderr
        );
        let rows = r.json();
        let rows = rows.as_array().expect("an array");
        assert!(!rows.is_empty(), "reader {i} saw at least the seed");
        for row in rows {
            let harp = row["harp_id"].as_str().expect("harp_id");
            assert_eq!(
                Some(row),
                last.get(harp),
                "reader {i}: a task of the final listing, every field whole"
            );
        }
    }
    assert_eq!(env.stores().len(), 1);
}

/// A first-sight race for readers: listings run while the first adds
/// mint the project and create its database. A reader may find no project
/// yet, or a database not yet migrated or in WAL, and must still answer.
#[test]
fn test_readers_at_first_sight_answer_while_the_store_is_made() {
    let env = Env::new("first-sight-reader-harp");
    let before = env.run(&["tasks", "list"]);
    assert!(
        before.ok && before.stderr.contains("no project here"),
        "{}",
        before.stderr
    );
    let (writers, readers) = (20, 10);
    let ran = concurrently(&env, writers + readers, |i| {
        if i < writers {
            vec![
                "tasks".to_string(),
                "add".to_string(),
                format!("fs-task-{i:03}"),
            ]
        } else {
            vec![
                "tasks".to_string(),
                "list".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ]
        }
    });
    for (i, r) in ran.iter().enumerate() {
        assert!(r.ok, "process {i} failed: {}", r.stderr);
        if i >= writers {
            assert!(r.json().is_array(), "reader {i}: {}", r.stdout);
        }
    }
    assert_eq!(env.list().len(), writers);
    assert_eq!(env.registered().len(), 1);
}

#[test]
fn test_concurrent_tags_of_one_task_lose_no_tag() {
    let env = Env::new("cross-proc-tag-harp");
    let harp = env.seed("tag-me")["harp_id"]
        .as_str()
        .expect("a harp id")
        .to_string();
    env.permit_every_tag();

    let n = 40;
    let ran = concurrently(&env, n, |i| {
        vec![
            "tasks".to_string(),
            "tag".to_string(),
            "add".to_string(),
            harp.clone(),
            format!("tag-{i:03}"),
        ]
    });
    for (i, r) in ran.iter().enumerate() {
        assert!(r.ok, "tag process {i} failed: {}", r.stderr);
    }

    let tasks = env.list();
    assert_eq!(tasks.len(), 1, "the one seeded task");
    let tags: Vec<String> = tasks[0]["tags"]
        .as_array()
        .expect("tags")
        .iter()
        .map(|t| t.as_str().unwrap().to_string())
        .collect();
    let expected: Vec<String> = (0..n).map(|i| format!("tag-{i:03}")).collect();
    assert_eq!(tags, expected, "every tag, sorted");
}

#[test]
fn test_a_task_added_is_listed_in_text_with_its_project() {
    let env = Env::new("smoke-harp");
    let added = env.run(&[
        "tasks",
        "add",
        "Ship",
        "it",
        "--tag",
        "queue:release",
        "--format",
        "text",
    ]);
    assert!(added.ok, "{}", added.stderr);
    let fields: Vec<&str> = added.stdout.trim_end().split('\t').collect();
    assert_eq!(fields[1..], ["To Do", "Ship it"]);
    assert!(
        added.stderr.starts_with("taskloom: project "),
        "{}",
        added.stderr
    );
    let listed = env.run(&["tasks", "list", "--format", "text"]);
    assert!(listed.stdout.starts_with("Project: "), "{}", listed.stdout);
    assert!(
        listed.stdout.contains("Ship it  [queue:release]"),
        "{}",
        listed.stdout
    );
    let json = env.list();
    let created = json[0]["created_at"].as_str().expect("created_at");
    assert!(
        created.starts_with("20"),
        "recorded at the clock's time: {created}"
    );
    let marker = disk::read_to_string(env.project.path().join(".project-id")).unwrap();
    assert_eq!(
        marker.trim().split('-').count(),
        3,
        "a three-word id: {marker}"
    );
}

// "" is never "not given": a value given empty is refused, in its
// envelope, and nothing is written or pruned for it; one left out is the
// default. An empty CTXLOOM_SESSION_HARP or CTXLOOM_PROJECT_ID is unset.
#[test]
fn test_a_value_given_empty_is_refused_never_read_as_left_out() {
    let env = Env::new("empty-harp");
    let seeded = env.seed("Shipped");
    let harp = seeded["harp_id"].as_str().unwrap().to_string();
    let done = env.run(&[
        "tasks", "set", &harp, "--status", "Done", "--format", "json",
    ]);
    assert!(done.ok, "{}", done.stderr);
    for (args, code) in [
        (vec!["tasks", "add", "x", "--status", ""], codes::blank()),
        (vec!["tasks", "add", "x", "--trigger", " "], codes::blank()),
        (vec!["tasks", "add", "x", "--tag", ""], codes::tag_blank()),
        (
            vec!["--project", "", "tasks", "add", "x"],
            codes::invalid_id(),
        ),
        (
            vec![
                "tasks",
                "set",
                &harp,
                "--status",
                "Deferred",
                "--trigger",
                "",
            ],
            codes::blank(),
        ),
        (
            vec!["tasks", "tag", "remove", &harp, ""],
            codes::tag_blank(),
        ),
        (vec!["tasks", "list", "--term", ""], codes::blank()),
        (vec!["tasks", "list", "--tag-query", ""], codes::blank()),
        (vec!["tasks", "list", "--sort", ""], codes::blank()),
        (
            vec!["manage", "schema", "tags", "list", "--tag-query", ""],
            codes::blank(),
        ),
        (vec!["--project", "", "tasks", "list"], codes::invalid_id()),
        (
            vec![
                "tasks",
                "prune",
                "--older-than",
                "2999-01-01",
                "--tag-query",
                "",
                "--yes",
            ],
            codes::blank(),
        ),
    ] {
        let mut args = args.clone();
        args.extend(["--format", "json"]);
        let ran = env.run(&args);
        assert!(!ran.ok, "{args:?}: {}", ran.stdout);
        assert_eq!(ran.stdout, "", "{args:?}");
        let envelope: Value = serde_json::from_str(&ran.stderr).unwrap();
        assert_eq!(envelope["code"], code, "{args:?}: {envelope}");
    }
    let listed = env
        .run(&["tasks", "list", "--all", "--format", "json"])
        .json();
    assert_eq!(
        listed.as_array().unwrap().len(),
        1,
        "nothing added or pruned"
    );
    assert_eq!(listed[0]["status"], "Done", "nothing moved: {listed}");
    assert_eq!(listed[0]["tags"], serde_json::json!([]), "nothing tagged");

    let unset = [("CTXLOOM_SESSION_HARP", ""), ("CTXLOOM_PROJECT_ID", "")];
    let added = env.run_with(&["tasks", "add", "x", "--format", "json"], &unset);
    assert!(added.ok, "{}", added.stderr);
    assert_eq!(added.json()["origin_session"], "", "{}", added.stdout);
    assert!(
        added.stderr.contains(&*seeded_project(&env)),
        "{}",
        added.stderr
    );
}

/// The project id the work root is bound to.
fn seeded_project(env: &Env) -> String {
    disk::read_to_string(env.project.path().join(".project-id"))
        .unwrap()
        .trim()
        .to_string()
}

#[test]
fn test_a_list_of_a_project_never_written_creates_nothing() {
    let env = Env::new("tpyo-harp");
    let listed = env.run(&["tasks", "list", "--project", "tpyo", "--format", "json"]);
    assert!(listed.ok, "{}", listed.stderr);
    assert_eq!(listed.json(), serde_json::json!([]));
    assert!(
        !disk::exists(env.home.path().join(".ctxloom")),
        "no registry, no project directory, no database"
    );
    assert!(!disk::exists(env.project.path().join(".project-id")));
}

#[test]
fn test_a_tag_where_there_is_no_project_fails_and_binds_none() {
    let env = Env::new("no-project-harp");
    let tagged = env.run(&["tasks", "tag", "add", "swift-amber", "x"]);
    assert!(!tagged.ok);
    assert!(
        tagged.stderr.contains("no project here yet"),
        "{}",
        tagged.stderr
    );
    assert!(!disk::exists(env.home.path().join(".ctxloom")));
    assert!(!disk::exists(env.project.path().join(".project-id")));
}

/// Run `taskloom args` with HOME `home`, in a working directory that is
/// gone: the shell enters a directory and removes it, so the binary starts
/// where nothing is.
/// (A unix shell's: the callers run on unix only.)
fn run_where_the_working_directory_is_gone(env: &Env, home: &Path, args: &[&str]) -> Ran {
    let gone = env.project.path().join("gone");
    disk::create_dir_all(&gone).unwrap();
    let output = Command::new("sh")
        .arg("-c")
        .arg(r#"cd "$1" && rmdir "$1" && shift && exec "$@""#)
        .arg("sh")
        .arg(&gone)
        .arg(binary())
        .args(args)
        .isolated(home)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()
        .expect("sh runs");
    Ran::of(output)
}

// A working directory that is gone fails only the commands that need it,
// with the catalogued error: `version` and `loadout` run, `list` is
// refused.
#[test]
fn test_a_deleted_working_directory_fails_only_the_commands_that_need_it() {
    if os::host().flavor() != os::Flavor::Unix {
        return;
    }
    let env = Env::new("gone-cwd-harp");
    let run = |args: &[&str]| run_where_the_working_directory_is_gone(&env, env.home.path(), args);
    let version = run(&["manage", "version"]);
    assert!(version.ok, "{}", version.stderr);
    let loadout = run(&["loadout", "--format", "yaml"]);
    assert!(loadout.ok, "{}", loadout.stderr);
    let list = run(&["tasks", "list"]);
    assert!(!list.ok, "{}", list.stdout);
    assert!(
        list.stderr.contains("the working directory cannot be read"),
        "{}",
        list.stderr
    );
}

// With the working directory gone, a relative path is resolved against
// nothing, never against `/`: a relative HOME's store is refused for the
// working directory, not made (or refused) at `/<home>`.
#[test]
fn test_a_relative_path_is_never_resolved_against_the_top_when_the_cwd_is_gone() {
    if os::host().flavor() != os::Flavor::Unix {
        return;
    }
    let env = Env::new("gone-cwd-harp");
    let home = Path::new("taskloom-relative-home");
    let added = run_where_the_working_directory_is_gone(
        &env,
        home,
        &["tasks", "add", "Ship it", "--project", "jolly-mute-magma"],
    );
    assert!(!added.ok, "{}", added.stdout);
    assert!(
        added
            .stderr
            .contains("taskloom-relative-home/.ctxloom/projects/jolly-mute-magma")
            && added.stderr.contains(angzarr_diagnostic::Diagnostic::code(
                &file_system::CwdError {
                    reason: String::new()
                }
            )),
        "{}",
        added.stderr
    );
    assert!(
        !disk::exists(Path::new("/").join(home)),
        "nothing made at the top"
    );
}

#[test]
fn test_a_read_spans_every_project_outside_any_work_root_and_one_by_its_id() {
    let env = Env::new("global-harp");
    env.seed("in the first");
    let (repo, empty_repo) = (Temp::new(), Temp::new());
    for dir in [&repo, &empty_repo] {
        disk::create_dir_all(dir.path().join(".git")).unwrap();
    }
    let added = env.run_in(
        repo.path(),
        &["tasks", "add", "in the repo", "--format", "json"],
        &[],
    );
    assert!(added.ok, "{}", added.stderr);

    let here = env.run(&["tasks", "list", "--format", "json"]);
    assert_eq!(
        texts(&here.json().as_array().unwrap().clone(), "text"),
        ["in the first"]
    );
    // Another project is read by its id, one project at a time.
    let there = env.run_in(
        repo.path(),
        &["manage", "project", "show", "--format", "json"],
        &[],
    );
    let id = there.json()["project_id"].as_str().unwrap().to_string();
    let other = env.run(&["tasks", "list", "--project", &id, "--format", "json"]);
    assert!(other.ok, "{}", other.stderr);
    let rows = other.json().as_array().unwrap().clone();
    assert_eq!(texts(&rows, "text"), ["in the repo"]);

    let fallback = env.run_outside(&["tasks", "list", "--format", "json"]);
    assert!(fallback.ok, "{}", fallback.stderr);
    assert_eq!(fallback.json().as_array().unwrap().len(), 2);
    assert!(
        fallback.stderr.contains("so this reads every project"),
        "{}",
        fallback.stderr
    );
    let text = env.run_outside(&["tasks", "list", "--format", "text"]);
    assert!(
        text.stdout.starts_with("Projects: 2 (every project)\n"),
        "{}",
        text.stdout
    );

    let in_repo = env.run_in(
        empty_repo.path(),
        &["tasks", "list", "--format", "json"],
        &[],
    );
    assert_eq!(in_repo.json(), serde_json::json!([]));
    assert!(
        in_repo.stderr.contains("no project here yet"),
        "{}",
        in_repo.stderr
    );
    assert!(!disk::exists(outside_every_work_root().join(".project-id")));
    assert!(!disk::exists(empty_repo.path().join(".project-id")));
}

// Where the temporary directory is decides nothing. With TMPDIR inside a
// worktree's target/, the worktree (and its committed marker) was every
// test's project, and a watch that should have found none watched it
// forever. A directory a test runs the binary in is its own work root,
// whatever checkout is above it.
#[test]
fn test_a_checkout_above_a_tests_directory_is_not_its_project() {
    let env = Env::new("enclosed-harp");
    let checkout = Temp::new();
    disk::create_dir_all(checkout.path().join(".git")).unwrap();
    disk::write(checkout.path().join(".project-id"), "jolly-mute-magma\n").unwrap();
    let dir = checkout.path().join("target").join("tmp");
    disk::create_dir_all(&dir).unwrap();

    let listed = env.run_in(&dir, &["tasks", "list", "--format", "json"], &[]);
    assert!(
        listed.stderr.contains("no project here yet"),
        "{}",
        listed.stderr
    );
    let added = env.run_in(&dir, &["tasks", "add", "enclosed", "--format", "json"], &[]);
    assert!(added.ok, "{}", added.stderr);
    let shown = env.run_in(
        &dir,
        &["manage", "project", "show", "--format", "json"],
        &[],
    );
    assert!(shown.ok, "{}", shown.stderr);
    let shown = shown.json();
    assert_ne!(shown["project_id"], "jolly-mute-magma", "{shown}");
    assert_eq!(shown["work_root"], dir.display().to_string(), "{shown}");
    assert_eq!(
        disk::read_to_string(checkout.path().join(".project-id")).unwrap(),
        "jolly-mute-magma\n",
        "the checkout's marker is untouched"
    );
}

#[test]
fn test_every_diagnostic_of_every_crate_renders_from_the_binarys_catalog() {
    let catalog = super::catalog().expect("every catalog loads, no message twice");
    let samples: Vec<Box<dyn angzarr_diagnostic::Diagnostic>> = [
        angzarr_embedded::diagnostic_samples(),
        tasks::diagnostic_samples(),
        tasks_application::diagnostic_samples(),
        task_priority::diagnostic_samples(),
        tag_rules::diagnostic_samples(),
        file_system::diagnostic_samples(),
        project_registry::diagnostic_samples(),
        task_store_sqlite::diagnostic_samples(),
        tasks_cli::diagnostic_samples(),
        project_application::diagnostic_samples(),
        project_cli::diagnostic_samples(),
        jsonl_task_log::diagnostic_samples(),
        tasks_config::diagnostic_samples(),
        angzarr_presentation::diagnostic_samples(),
        file_lock::diagnostic_samples(),
        tasks_watch::diagnostic_samples(),
        plans_application::diagnostic_samples(),
        plan_files::diagnostic_samples(),
        plans_cli::diagnostic_samples(),
        store_cli::diagnostic_samples(),
        angzarr_store_transfer::diagnostic_samples(),
    ]
    .into_iter()
    .flatten()
    .collect();
    for sample in &samples {
        assert!(
            catalog.render(&**sample).is_ok(),
            "{}: {:?}",
            sample.code(),
            catalog.render(&**sample)
        );
    }
}

// The diagnostics are angzarr-diagnostic's, and annals has left the
// workspace (step 4 of taskloom-on-angzarr-embedded.plan.md).

/// Every crate of this workspace's diagnostic samples (not the runtime's,
/// angzarr-embedded's, which are its own).
fn ours() -> Vec<Box<dyn angzarr_diagnostic::Diagnostic>> {
    [
        tasks::diagnostic_samples(),
        tasks_application::diagnostic_samples(),
        task_priority::diagnostic_samples(),
        tag_rules::diagnostic_samples(),
        file_system::diagnostic_samples(),
        project_registry::diagnostic_samples(),
        task_store_sqlite::diagnostic_samples(),
        tasks_cli::diagnostic_samples(),
        project_application::diagnostic_samples(),
        project_cli::diagnostic_samples(),
        jsonl_task_log::diagnostic_samples(),
        tasks_config::diagnostic_samples(),
        angzarr_presentation::diagnostic_samples(),
        file_lock::diagnostic_samples(),
        tasks_watch::diagnostic_samples(),
        plans_application::diagnostic_samples(),
        plan_files::diagnostic_samples(),
        plans_cli::diagnostic_samples(),
        store_cli::diagnostic_samples(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The names of `d`'s args, and of every nested cause's, as `code.name`.
fn arg_names(d: &dyn angzarr_diagnostic::Diagnostic) -> Vec<String> {
    d.args()
        .into_iter()
        .flat_map(|(name, arg)| {
            let mut names = vec![format!("{}.{name}", d.code())];
            if let angzarr_diagnostic::Arg::Nested(cause) = arg {
                names.extend(arg_names(&*cause));
            }
            names
        })
        .collect()
}

#[test]
fn test_a_third_partys_text_is_the_arg_ext_err_txt_in_every_crate() {
    // A third party's text (the OS's through std's io::Error, a parser's,
    // a serializer's) travels verbatim as `ext_err_txt` (angzarr decision
    // 14.37), never under a name of its own.
    let detail: Vec<String> = ours()
        .iter()
        .flat_map(|d| arg_names(&**d))
        .filter(|name| name.ends_with(".detail"))
        .collect();
    assert!(detail.is_empty(), "still `detail`: {detail:#?}");
}

#[test]
fn test_every_diagnostic_displays_as_its_key_and_args_never_english() {
    let english: Vec<String> = ours()
        .iter()
        .map(|d| d.to_string())
        .zip(ours().iter().map(|d| d.code()))
        .filter(|(shown, code)| shown != code && !shown.starts_with(&format!("{code} ")))
        .map(|(shown, code)| format!("{code}: {shown}"))
        .collect();
    assert!(english.is_empty(), "English crosses: {english:#?}");
}

#[test]
fn test_annals_has_left_the_workspace() {
    let lock = include_str!("../../../../../Cargo.lock");
    let annals: Vec<&str> = lock
        .lines()
        .filter_map(|line| line.strip_prefix("name = \""))
        .map(|name| name.trim_end_matches('"'))
        .filter(|name| name.starts_with("annals"))
        .collect();
    assert!(annals.is_empty(), "still in the graph: {annals:?}");
    let manifest = include_str!("../../../../../Cargo.toml");
    assert!(
        !manifest.contains("benjaminabbitt/annals"),
        "the workspace still pins annals"
    );
}

/// stdout as exactly one JSON document, as jq reads it.
fn one_document(text: &str) -> Value {
    let mut documents = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    let first = documents
        .next()
        .unwrap_or_else(|| panic!("no JSON: {text:?}"))
        .unwrap_or_else(|e| panic!("not JSON ({e}): {text:?}"));
    assert!(
        documents.next().is_none(),
        "more than one document: {text:?}"
    );
    first
}

#[test]
fn test_in_json_each_command_writes_one_document_and_errors_an_envelope() {
    let env = Env::new("jq-harp");
    let added = env.run(&["tasks", "add", "Ship", "it", "--format", "json"]);
    assert!(added.ok, "{}", added.stderr);
    let task = one_document(&added.stdout);
    let harp = task["harp_id"].as_str().expect("harp_id").to_string();
    assert!(
        added.stderr.starts_with("taskloom: project "),
        "notices on stderr: {}",
        added.stderr
    );
    env.permit_every_tag();
    let tagged = env.run(&["tasks", "tag", "add", &harp, "x", "--format", "json"]);
    assert!(tagged.ok, "{}", tagged.stderr);
    assert_eq!(
        one_document(&tagged.stdout)["tags"],
        serde_json::json!(["x"])
    );
    let listed = env.run(&["tasks", "list", "--format", "json"]);
    assert!(listed.ok, "{}", listed.stderr);
    assert_eq!(one_document(&listed.stdout)[0]["harp_id"], harp.as_str());
    let queried = env.run(&["tasks", "list", "--tag-query", "x", "--format", "json"]);
    assert!(queried.ok, "{}", queried.stderr);
    assert_eq!(one_document(&queried.stdout)[0]["harp_id"], harp.as_str());
    let unread = env.run(&["tasks", "list", "--tag-query", "x/and", "--format", "json"]);
    assert!(!unread.ok);
    assert_eq!(unread.stdout, "", "an error leaves stdout empty");
    assert_eq!(one_document(&unread.stderr)["code"], codes::tag_query());

    let failed = env.run(&[
        "tasks",
        "tag",
        "add",
        "no-such-task",
        "x",
        "--format",
        "json",
    ]);
    assert!(!failed.ok);
    assert_eq!(failed.stdout, "", "an error leaves stdout empty");
    let envelope = one_document(&failed.stderr);
    assert!(envelope["code"].is_string(), "{envelope}");
    assert!(envelope["args"].is_object(), "{envelope}");
    assert!(envelope["message"].is_string(), "{envelope}");

    let usage = env.run(&["frobnicate", "--format", "json"]);
    assert!(!usage.ok);
    assert_eq!(one_document(&usage.stderr)["code"], codes::usage());

    let logs = env.home.path().join(".ctxloom/tasks");
    disk::create_dir_all(&logs).unwrap();
    disk::write(
        logs.join("jolly-mute-magma.jsonl"),
        "{\"op\":\"add\",\"task\":\"brisk-heron\",\"text\":\"Go's\",\"ts\":\"2026-03-01T10:01:00Z\"}\n",
    )
    .unwrap();
    let imported = env.run(&["manage", "migrate", "tasks", "--format", "json"]);
    assert!(imported.ok, "{}", imported.stderr);
    assert_eq!(one_document(&imported.stdout)["complete"], true);

    let incomplete = env.run(&[
        "manage",
        "migrate",
        "tasks",
        "--project",
        "no-such-project",
        "--format",
        "json",
    ]);
    assert!(!incomplete.ok);
    let report = one_document(&incomplete.stdout);
    assert_eq!(
        report["projects"][0]["failed"]["code"],
        codes::no_jsonl_log()
    );
    assert_eq!(
        one_document(&incomplete.stderr)["code"],
        codes::import_incomplete()
    );
}

/// A project with one task added, for a command's JSON to be checked.
struct Jq {
    env: Env,
    harp: String,
}

impl Jq {
    fn new(session: &'static str) -> Jq {
        let env = Env::new(session);
        let harp = env.seed("Ship it")["harp_id"]
            .as_str()
            .expect("harp_id")
            .to_string();
        env.permit_every_tag();
        Jq { env, harp }
    }

    /// `args`' answer: it succeeded, and wrote one JSON document.
    fn ok(&self, args: &[&str]) -> Value {
        let ran = self.env.run(args);
        assert!(ran.ok, "{args:?}: {}", ran.stderr);
        one_document(&ran.stdout)
    }

    /// `args` failed with `code`: an envelope on stderr, stdout empty.
    fn refused(&self, args: &[&str], code: &str) {
        let ran = self.env.run(args);
        assert!(!ran.ok, "{args:?}");
        assert_eq!(ran.stdout, "", "{args:?}: an error leaves stdout empty");
        assert_eq!(one_document(&ran.stderr)["code"], code, "{args:?}");
    }
}

#[test]
fn test_in_json_each_change_writes_one_document_and_errors_an_envelope() {
    let jq = Jq::new("jq-change-harp");
    let harp = jq.harp.as_str();
    let task = jq.ok(&[
        "tasks",
        "set",
        harp,
        "--status",
        "Deferred",
        "--trigger",
        "later",
        "--format",
        "json",
    ]);
    assert_eq!(
        (&task["status"], &task["trigger"]),
        (&"Deferred".into(), &"later".into())
    );
    // Reverses the reviewed "kept" (the human's ruling on L, 2026-10-27):
    // leaving Deferred by a status clears the trigger.
    let back = jq.ok(&[
        "tasks", "set", harp, "--status", "To Do", "--format", "json",
    ]);
    assert_eq!(back["trigger"], "", "cleared on leaving Deferred");
    jq.ok(&[
        "tasks",
        "set",
        harp,
        "--status",
        "In Progress",
        "--format",
        "json",
    ]);
    let deferred = jq.ok(&[
        "tasks",
        "set",
        harp,
        "--trigger",
        "later",
        "--format",
        "json",
    ]);
    assert_eq!(
        (&deferred["status"], &deferred["trigger"]),
        (&"Deferred".into(), &"later".into()),
        "a trigger alone defers"
    );
    let revived = jq.ok(&["tasks", "set", harp, "--clear-trigger", "--format", "json"]);
    assert_eq!(
        (&revived["status"], &revived["trigger"]),
        (&"In Progress".into(), &"".into()),
        "back to the status it was deferred from"
    );
    jq.refused(
        &[
            "tasks",
            "set",
            harp,
            "--status",
            "Done",
            "--trigger",
            "later",
            "--format",
            "json",
        ],
        &codes::trigger_with_status(),
    );
    jq.refused(
        &[
            "tasks",
            "set",
            harp,
            "--trigger",
            "x",
            "--clear-trigger",
            "--format",
            "json",
        ],
        &codes::trigger_and_clear(),
    );
    jq.ok(&[
        "tasks", "set", harp, "--status", "To Do", "--format", "json",
    ]);
    jq.refused(
        &[
            "tasks", "set", harp, "--status", "@open", "--format", "json",
        ],
        &codes::status_reserved(),
    );
    jq.refused(
        &[
            "tasks",
            "set",
            "no-such-task",
            "--status",
            "Done",
            "--format",
            "json",
        ],
        &codes::no_task(),
    );
}

#[test]
fn test_in_json_each_text_and_tag_change_writes_one_document_and_errors_an_envelope() {
    let jq = Jq::new("jq-text-harp");
    let harp = jq.harp.as_str();
    let task = jq.ok(&[
        "tasks",
        "set",
        harp,
        "--text",
        "Ship it today",
        "--format",
        "json",
    ]);
    assert_eq!(
        (&task["text"], &task["status"]),
        (&"Ship it today".into(), &"To Do".into())
    );
    jq.refused(
        &["tasks", "set", harp, "--text", " ", "--format", "json"],
        &codes::text_required(),
    );
    jq.ok(&[
        "tasks",
        "tag",
        "add",
        harp,
        "triage:level=3",
        "--format",
        "json",
    ]);
    let tagged = jq.ok(&["tasks", "tag", "add", harp, "x", "--format", "json"]);
    assert_eq!(tagged["tags"], serde_json::json!(["triage:level=3", "x"]));
    jq.ok(&[
        "tasks",
        "tag",
        "add",
        harp,
        "triage:level=1",
        "--format",
        "json",
    ]);
    let collapsed = jq.ok(&["tasks", "tag", "remove", harp, "x", "--format", "json"]);
    assert_eq!(
        collapsed["tags"],
        serde_json::json!(["triage:level=1"]),
        "the scalar collapsed, the add then the remove"
    );
}

// A write retried under its request id applies once: the retry answers
// with the task as it stands, a later change by another request kept. An
// add retried under its request id answers with the task it added, and
// adds no other.
#[test]
fn test_a_write_retried_under_its_request_id_applies_once() {
    let jq = Jq::new("jq-request-harp");
    let harp = jq.harp.as_str();
    let moving = [
        "tasks",
        "set",
        harp,
        "--status",
        "In Progress",
        "--request-id",
        "r-1",
        "--format",
        "json",
    ];
    let first = jq.ok(&moving);
    assert_eq!(jq.ok(&moving), first, "its time of change included");
    jq.ok(&["tasks", "set", harp, "--status", "Done", "--format", "json"]);
    assert_eq!(jq.ok(&moving)["status"], "Done", "not applied again");
    let tagging = [
        "tasks",
        "tag",
        "add",
        harp,
        "x",
        "--request-id",
        "r-1",
        "--format",
        "json",
    ];
    jq.ok(&tagging);
    jq.ok(&["tasks", "tag", "remove", harp, "x", "--format", "json"]);
    assert_eq!(jq.ok(&tagging)["tags"], serde_json::json!([]));
    let editing = [
        "tasks",
        "set",
        harp,
        "--text",
        "First",
        "--request-id",
        "r-1",
        "--format",
        "json",
    ];
    jq.ok(&editing);
    jq.ok(&["tasks", "set", harp, "--text", "Second", "--format", "json"]);
    assert_eq!(jq.ok(&editing)["text"], "Second");
    jq.refused(
        &[
            "tasks",
            "set",
            harp,
            "--status",
            "Done",
            "--request-id",
            " ",
            "--format",
            "json",
        ],
        &codes::blank(),
    );
    let adding = [
        "tasks",
        "add",
        "Again",
        "--request-id",
        "r-2",
        "--format",
        "json",
    ];
    let added = jq.ok(&adding);
    assert_eq!(jq.ok(&adding)["harp_id"], added["harp_id"]);
    let listed = jq.ok(&["tasks", "list", "--all", "--format", "json"]);
    assert_eq!(listed.as_array().map(Vec::len), Some(2), "{listed}");
}

#[test]
fn test_in_json_each_read_writes_one_document_and_errors_an_envelope() {
    let jq = Jq::new("jq-read-harp");
    let harp = jq.harp.as_str();
    jq.ok(&["tasks", "tag", "add", harp, "release", "--format", "json"]);
    let narrowed = jq.ok(&[
        "tasks",
        "list",
        "--status",
        "@open",
        "--term",
        "SHIP",
        "--compact",
        "--limit",
        "1",
        "--format",
        "json",
    ]);
    assert_eq!(narrowed[0]["headline"], "Ship it");
    jq.refused(
        &["tasks", "list", "--status", "@done", "--format", "json"],
        &codes::unknown_status_class(),
    );

    assert_eq!(
        jq.ok(&["tasks", "show", harp, "--format", "json"])["text"],
        "Ship it"
    );
    let twice = jq.ok(&["tasks", "show", harp, harp, "--format", "json"]);
    assert_eq!(twice.as_array().map(Vec::len), Some(2));
    jq.refused(
        &["tasks", "show", harp, "no-such-task", "--format", "json"],
        &codes::not_found(),
    );

    for args in [
        &["tasks", "summary", "--format", "json"][..],
        &["tasks", "summary", "--format", "json"],
    ] {
        let summary = jq.ok(args);
        assert_eq!(summary["counts"]["To Do"], 1, "{args:?}");
        assert_eq!(summary["in_progress"], serde_json::json!([]), "{args:?}");
    }

    let statuses = jq.ok(&["manage", "schema", "statuses", "list", "--format", "json"]);
    let names: Vec<&Value> = statuses
        .as_array()
        .expect("an array")
        .iter()
        .map(|s| &s["name"])
        .collect();
    assert_eq!(
        names,
        ["In Progress", "To Do", "Deferred", "Done", "Archived"]
    );

    for args in [
        &["manage", "schema", "tags", "list", "--format", "json"][..],
        &[
            "manage", "schema", "tags", "list", "--status", "@open", "--format", "json",
        ],
    ] {
        assert_eq!(
            jq.ok(args),
            serde_json::json!([{"tag": "release", "active": 1, "total": 1}]),
            "{args:?}"
        );
    }
    jq.refused(
        &[
            "manage",
            "schema",
            "tags",
            "list",
            "--tag-query",
            "x/and",
            "--format",
            "json",
        ],
        &codes::tag_query(),
    );
}

#[test]
fn test_in_json_a_check_writes_one_document_and_a_finding_an_envelope() {
    let jq = Jq::new("jq-check-harp");
    let harp = jq.harp.as_str();
    let linted = jq.ok(&["tasks", "check", "--format", "json"]);
    assert_eq!(linted["violations"], serde_json::json!([]));
    assert!(linted["checked_targets"].as_u64() > Some(0), "{linted}");
    let tagged = jq.ok(&["tasks", "tag", "add", harp, "area:x=c", "--format", "json"]);
    assert_eq!(tagged["tags"].as_array().map(Vec::len), Some(1));
    let declared = jq.ok(&[
        "manage",
        "schema",
        "declare",
        "--yes",
        "--add",
        r#"tagma.enum:"area:x"=a"#,
        "--add",
        r#"tagma.enum:"area:x"=b"#,
        "--format",
        "json",
    ]);
    assert_eq!(declared["changed"], true);
    let ran = jq.env.run(&["tasks", "check", "--format", "json"]);
    assert!(!ran.ok, "a violation exits 1");
    let report = one_document(&ran.stdout);
    assert_eq!(report["violations"][0]["harp_id"], harp);
    assert_eq!(one_document(&ran.stderr)["code"], codes::lint_violations());
}

#[test]
fn test_a_list_sorted_by_priority_ranks_the_projects_tasks_through_its_store() {
    let jq = Jq::new("jq-priority-harp");
    let critical = jq.ok(&[
        "tasks",
        "add",
        "Fix it",
        "--tag",
        "triage:level=1",
        "--format",
        "json",
    ]);
    let ranked = jq.ok(&["tasks", "list", "--sort", "priority", "--format", "json"]);
    let order: Vec<(&Value, &Value)> = ranked
        .as_array()
        .expect("an array")
        .iter()
        .map(|t| (&t["harp_id"], &t["derived_priority"]))
        .collect();
    assert_eq!(
        order,
        [
            (&critical["harp_id"], &serde_json::json!(5)),
            (&serde_json::json!(jq.harp), &serde_json::json!(2.5)),
        ],
        "level 1 first; the untriaged task floors below it"
    );
    assert_eq!(
        jq.ok(&["tasks", "list", "--format", "json"])[0]["derived_priority"],
        Value::Null
    );
}

#[test]
fn test_in_json_a_stream_writes_one_document_a_line() {
    let env = Env::new("jq-watch-harp");
    let added = env.run(&["tasks", "add", "Ship", "it", "--format", "json"]);
    assert!(added.ok, "{}", added.stderr);
    env.permit_every_tag();
    let watching = watch::Watching::start(&env, &["--format", "json"]);
    // Priming line, then a change (owner-authorised with slice D approval
    // 2026-11-03: the first was `changed` and the second named `kind`).
    let primed = one_document(&watching.line());
    assert_eq!(primed["event"], "primed");
    assert_eq!(primed["kind"], "tasks");
    let harp = one_document(&added.stdout)["harp_id"].to_string();
    let tagged = env.run(&["tasks", "tag", "add", harp.trim_matches('"'), "x"]);
    assert!(tagged.ok, "{}", tagged.stderr);
    assert_eq!(one_document(&watching.line())["event"], "changed");
    if let Some(status) = watching.stop("INT") {
        assert!(status.success(), "SIGINT is a clean end: {status}");
    }
}

#[test]
fn test_in_json_each_project_command_writes_one_document_and_errors_an_envelope() {
    let env = Env::new("jq-project-harp");
    let added = env.run(&["tasks", "add", "Ship", "it", "--format", "json"]);
    assert!(added.ok, "{}", added.stderr);
    let shown = env.run(&["manage", "project", "show", "--format", "json"]);
    assert!(shown.ok, "{}", shown.stderr);
    let project = one_document(&shown.stdout)["project_id"]
        .as_str()
        .expect("the project added to")
        .to_string();
    for args in [
        &["manage", "doctor", "heal", "--format", "json"][..],
        &["manage", "doctor", "orphans", "--format", "json"],
        &["manage", "doctor", "repoint", &project, "--format", "json"],
    ] {
        let ran = env.run(args);
        assert!(ran.ok, "{args:?}: {}", ran.stderr);
        one_document(&ran.stdout);
    }
    let refused = env.run(&[
        "manage",
        "doctor",
        "repoint",
        "no-such-project",
        "--format",
        "json",
    ]);
    assert!(!refused.ok);
    assert_eq!(refused.stdout, "", "an error leaves stdout empty");
    assert_eq!(
        one_document(&refused.stderr)["code"],
        codes::unknown_project()
    );
}

/// A store as the e996ea4 build wrote one, migration 0005 pending (see
/// task-store-sqlite's tests): two tasks, in the angzarr wire before the
/// event page's `payload` became `body`. This build does not read it.
const OLD_GENERATION: &[u8] =
    include_bytes!("../../../../adapters/driven/task-store-sqlite/testdata/before-0005.db");

/// The same store with its event pages in the wire this build reads:
/// migration 0005 pending, two tasks.
const THIS_GENERATION_BEFORE_0005: &[u8] =
    include_bytes!("../../../../adapters/driven/task-store-sqlite/testdata/current-before-0005.db");

/// A project whose store file holds `bytes`, nothing beside it: the project
/// is minted by a seeding `add`, then its store replaced.
fn project_with_store(env: &Env, bytes: &[u8]) -> PathBuf {
    env.seed("replaced");
    let stores = env.stores();
    let [store] = stores.as_slice() else {
        panic!("one store: {stores:?}");
    };
    for sibling in ["state.db-wal", "state.db-shm"] {
        let sibling = store.with_file_name(sibling);
        if disk::exists(&sibling) {
            disk::remove_file(&sibling).unwrap();
        }
    }
    disk::write(store, bytes).unwrap();
    store.clone()
}

/// What the directory at `dir` holds: each entry's name, bytes and
/// modification time.
fn tree(dir: &Path) -> Vec<(std::ffi::OsString, Vec<u8>, std::time::SystemTime)> {
    disk::read_dir(dir)
        .unwrap()
        .into_iter()
        .map(|path| {
            let modified = os().metadata(&path).unwrap().modified;
            (
                path.file_name().unwrap().to_owned(),
                disk::read(&path).unwrap(),
                modified,
            )
        })
        .collect()
}

/// The owner ruled: a store an older build wrote stays a read-only archive,
/// read by the build that wrote it; this build starts new stores. Every
/// command that opens the store refuses it, exit 1 as a store that cannot
/// be used is, before anything is written: not migrated, not backed up, no
/// file made beside it, not one byte or mtime changed.
#[test]
fn test_a_store_an_older_generation_wrote_is_refused_by_every_command_and_left_untouched() {
    for beside in [&[][..], &["state.db-wal", "state.db-shm"][..]] {
        let env = Env::new("old-generation-harp");
        let store = project_with_store(&env, OLD_GENERATION);
        for name in beside {
            disk::write(store.with_file_name(name), b"").unwrap();
        }
        let dir = store.parent().unwrap();
        let before = tree(dir);
        for args in [
            &["tasks", "list", "--format", "json"][..],
            &["tasks", "add", "one more", "--format", "json"],
            &["tasks", "show", "no-such-harp", "--format", "json"],
            &["manage", "schema", "show", "--format", "json"],
        ] {
            let ran = env.run(args);
            assert!(!ran.ok, "{args:?} refused");
            assert_eq!(ran.code, 1, "{args:?}: {}", ran.stderr);
            assert_eq!(ran.stdout, "", "{args:?}: an error leaves stdout empty");
            let envelope = one_document(&ran.stderr);
            // As any store the build cannot use: the store is unavailable,
            // and the cause is the crate's own refusal, with its args.
            assert_eq!(
                envelope["code"], "angzarr-ports-store-unavailable",
                "{args:?}: {envelope}"
            );
            let cause = &envelope["args"]["cause"];
            assert_eq!(cause["code"], "task-store-sqlite-older-generation");
            assert_eq!(cause["args"]["path"], store.display().to_string());
            assert_eq!(
                cause["args"]["archive"],
                format!("{}.archive-<date>", store.display())
            );
            let message = envelope["message"].as_str().unwrap();
            assert!(
                message.contains(&store.display().to_string())
                    && message.contains("state.db-wal")
                    && message.contains("state.db.archive-<date>")
                    && message.contains("pre-port"),
                "the message says how to archive it: {message}"
            );
            assert_eq!(tree(dir), before, "{args:?}: untouched, not even an mtime");
        }
        let created: Vec<_> = tree(dir)
            .into_iter()
            .map(|(name, ..)| name.to_string_lossy().into_owned())
            .filter(|n| n.starts_with("state.db.pre-"))
            .collect();
        assert_eq!(created, Vec::<String>::new(), "no backup");
    }
}

/// Archived by hand as the message says, the project starts a store of its
/// own.
#[test]
fn test_with_the_old_store_moved_aside_the_project_starts_a_new_one() {
    let env = Env::new("archived-harp");
    let store = project_with_store(&env, OLD_GENERATION);
    disk::rename(&store, store.with_file_name("state.db.archive-2026-11-07")).unwrap();
    let added = env.run(&["tasks", "add", "afresh", "--format", "json"]);
    assert!(added.ok, "{}", added.stderr);
    assert_eq!(env.list().len(), 1, "only the new store's task");
}

/// The owner ruled 2026-10-31: both read and write migrate on startup. A
/// listing of a project whose store an older build of this generation wrote
/// migrates it, backed up beside it first, and lists its tasks.
#[test]
fn test_a_read_of_a_store_this_generation_wrote_migrates_and_backs_it_up_and_lists_its_tasks() {
    let env = Env::new("before-0005-harp");
    let store = project_with_store(&env, THIS_GENERATION_BEFORE_0005);
    let listed = env.run(&["tasks", "list", "--format", "json"]);
    assert!(listed.ok, "{}", listed.stderr);
    assert_eq!(listed.json().as_array().unwrap().len(), 2, "both tasks");
    let backups: Vec<PathBuf> = disk::read_dir(store.parent().unwrap())
        .unwrap()
        .into_iter()
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("state.db.pre-4-"))
        })
        .collect();
    let [backup] = backups.as_slice() else {
        panic!("one backup, at the store's level before 0005: {backups:?}");
    };
    let ledger = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(angzarr_embedded::angzarr_store_sqlite::SqliteStore::ledger_at(backup))
        .unwrap()
        .expect("a store there");
    let versions: Vec<u64> = ledger.iter().map(|r| r.version).collect();
    assert_eq!(
        versions,
        [1, 2, 3, 4],
        "the store as it was before migrating"
    );
}

/// Ruling 131: every command closes its store before the process exits, so
/// the moment it has exited, with no wait, the store is whole on its own:
/// its log holds nothing. A write's close is the last read-write one, which
/// checkpoints the log and removes it and its index; a read's connection is
/// read-only, which SQLite never lets remove them, so an empty log may stay.
/// Dropped, the store's close was left to the driver's thread, which the
/// exit did not always let run: the log stayed beside the file, holding the
/// command's commits.
#[test]
fn test_every_command_closes_its_store_before_it_exits() {
    let env = Env::new("closed-harp");
    let harp = env.seed("closed")["harp_id"].as_str().unwrap().to_string();
    let stores = env.stores();
    let [store] = stores.as_slice() else {
        panic!("one store: {stores:?}");
    };
    let beside = || {
        ["state.db-wal", "state.db-shm"]
            .into_iter()
            .filter(|name| disk::exists(store.with_file_name(name)))
            .collect::<Vec<_>>()
    };
    let logged = || {
        disk::exists(store.with_file_name("state.db-wal"))
            && !disk::read(store.with_file_name("state.db-wal"))
                .unwrap()
                .is_empty()
    };
    assert_eq!(beside(), Vec::<&str>::new(), "tasks add: nothing beside it");
    for (args, writes) in [
        (&["tasks", "list", "--format", "json"][..], false),
        (
            &[
                "tasks", "set", &harp, "--status", "Done", "--format", "json",
            ],
            true,
        ),
        (&["manage", "schema", "show", "--format", "json"], false),
        (&["plans", "list", "--format", "json"], false),
    ] {
        let ran = env.run(args);
        assert!(ran.ok, "{args:?}: {}", ran.stderr);
        assert!(!logged(), "{args:?}: the log holds nothing");
        if writes {
            assert_eq!(beside(), Vec::<&str>::new(), "{args:?}: nothing beside it");
        }
    }
}

/// A store this build made opens as it always did: no backup, no archive
/// refusal.
#[test]
fn test_a_store_this_build_made_opens_as_before() {
    let env = Env::new("current-harp");
    env.seed("kept");
    let listed = env.list();
    assert_eq!(listed.len(), 1);
    let stores = env.stores();
    let names: Vec<String> = tree(stores[0].parent().unwrap())
        .into_iter()
        .map(|(name, ..)| name.to_string_lossy().into_owned())
        .filter(|n| n.starts_with("state.db.pre-"))
        .collect();
    assert_eq!(names, Vec::<String>::new(), "nothing to back up");
}

#[path = "codes.test.rs"]
mod codes;

#[path = "companion.test.rs"]
mod companion;

#[path = "grammar.test.rs"]
mod grammar;

#[path = "import.test.rs"]
mod import;

#[path = "plan.test.rs"]
mod plan;

#[path = "plans_files.test.rs"]
mod plans_files;

#[path = "project.test.rs"]
mod project;

#[path = "tags.test.rs"]
mod tags;

#[path = "schema.test.rs"]
mod schema;

#[path = "prune.test.rs"]
mod prune;

#[path = "refusals.test.rs"]
mod refusals;

#[path = "snapshot.test.rs"]
mod snapshot;

#[path = "store.test.rs"]
mod store;

#[path = "watch.test.rs"]
mod watch;

#[path = "watch_cursor.test.rs"]
mod watch_cursor;
