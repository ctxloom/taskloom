//! The adapter against recorded runs: every case in fixtures/go-manage is
//! what taskloom's `manage` did to that file (scripts/go-manage-fixtures.sh
//! recorded it), and the adapter must leave the same bytes, refuse where
//! the recorded run refused, and copy a file aside where it did. Every
//! case runs on a filesystem of its own (`Rooted::temp`); nothing outside
//! it is written, and only the recorded cases are read from the crate.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use annals_diagnostic::Diagnostic;
use file_system::{Fs, Open, Os, Perms, Rooted};
use os::Mode;
use ports::{AgentConfigs, McpServer, Scope};

use super::{go_join, records, AgentConfigFiles, CLAUDE_CODE};

/// This crate's directory, where the recorded cases are.
fn fixtures() -> Os {
    Os::new(os::host(), Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// A filesystem of the test's own.
fn rooted() -> Arc<dyn Fs> {
    Arc::new(Rooted::temp().unwrap())
}

/// Write `bytes` at `path`, making its directory.
fn write(fs: &dyn Fs, path: &Path, bytes: impl AsRef<[u8]>) {
    fs.create_dir_all(path.parent().unwrap(), Mode::DIR)
        .unwrap();
    fs.write_atomic(path, bytes.as_ref(), Perms::Exact(Mode::FILE))
        .unwrap();
}

fn text(fs: &dyn Fs, path: &Path) -> String {
    fs.read_to_string(path).unwrap()
}

fn server() -> McpServer {
    McpServer {
        command: "taskloom".to_string(),
        args: vec!["mcp".to_string()],
    }
}

/// One recorded case.
struct Case {
    name: String,
    dir: PathBuf,
}

impl Case {
    fn all() -> Vec<Case> {
        let root = Path::new("fixtures/go-manage");
        let cases: Vec<Case> = fixtures()
            .read_dir(root)
            .expect("the Go cases")
            .into_iter()
            .map(|e| Case {
                name: e.name.to_string_lossy().into_owned(),
                dir: root.join(&e.name),
            })
            .collect();
        assert!(cases.len() > 20, "the cases are there");
        cases
    }

    fn file(&self, name: &str) -> Option<Vec<u8>> {
        fixtures().read(&self.dir.join(name)).ok()
    }

    fn project(&self) -> bool {
        fixtures().exists(&self.dir.join("scope")).unwrap()
    }

    /// An outcome file: its exit code and the rest.
    fn outcome(&self, name: &str) -> (i32, String) {
        let text = String::from_utf8(self.file(name).expect(name)).unwrap();
        let (code, rest) = text.split_once('\n').unwrap_or((&text, ""));
        (code.parse().unwrap(), rest.to_string())
    }
}

/// A fresh home and project directory, the case's file at its place with
/// `content`, and the adapter over them, with nothing on its PATH.
struct Home {
    fs: Arc<dyn Fs>,
    configs: AgentConfigFiles,
    path: PathBuf,
}

impl Home {
    fn new(case: &Case, content: Option<&[u8]>) -> Home {
        let fs = rooted();
        let (home, dir, bin) = (
            &*os::fixture("/home/u"),
            &*os::fixture("/work"),
            &*os::fixture("/bin-empty"),
        );
        for made in [home, dir, bin] {
            fs.create_dir_all(made, Mode::DIR).unwrap();
        }
        let mut configs = AgentConfigFiles {
            fs: Arc::clone(&fs),
            home: Some(home.to_path_buf()),
            path: Some(bin.as_os_str().to_owned()),
            pathext: None,
            waits: file_lock::Waits::DEFAULT,
        };
        let scope = if case.project() {
            Scope::Project(dir.to_path_buf())
        } else {
            Scope::User
        };
        let path = configs.config_path(CLAUDE_CODE, &scope).unwrap();
        if let Some(content) = content {
            write(&*fs, &path, content);
        }
        Home { fs, configs, path }
    }

    fn register(&mut self, server: Option<&McpServer>) -> Result<bool, annals_diagnostic::Report> {
        let path = self.path.clone();
        self.configs
            .register(CLAUDE_CODE, &path, "taskloom", "taskloom", server, false)
            .map(|r| r.changed)
    }

    fn content(&self) -> Option<Vec<u8>> {
        self.fs.read(&self.path).ok()
    }

    fn backups(&self) -> Vec<Vec<u8>> {
        let dir = self.path.parent().unwrap();
        let prefix = format!(
            "{}.corrupt-",
            self.path.file_name().unwrap().to_string_lossy()
        );
        self.fs
            .read_dir(dir)
            .unwrap()
            .into_iter()
            .filter_map(|e| {
                let name = e.name.to_string_lossy().into_owned();
                name.starts_with(&prefix)
                    .then(|| self.fs.read(&dir.join(&e.name)).unwrap())
            })
            .collect()
    }
}

/// `outcome` (the recorded exit code and stderr) held against what the
/// adapter answered and left: the file `want` after a success, the file
/// untouched and copied aside (or not) after a refusal.
fn hold(
    case: &Case,
    step: &str,
    home: &Home,
    answered: &Result<bool, annals_diagnostic::Report>,
    outcome: (i32, String),
    before: Option<&[u8]>,
    want: Option<Vec<u8>>,
) {
    let label = format!("{} {step}", case.name);
    let (code, stderr) = outcome;
    match answered {
        Ok(changed) => {
            assert_eq!(code, 0, "{label}: Go refused ({stderr}), Rust did not");
            assert_eq!(
                home.content()
                    .map(|c| String::from_utf8_lossy(&c).into_owned()),
                want.map(|w| String::from_utf8_lossy(&w).into_owned()),
                "{label}: the bytes"
            );
            let removed = stderr.contains("removed MCP server");
            let nothing = stderr.contains("nothing to remove");
            if removed || nothing {
                assert_eq!(*changed, removed, "{label}: changed");
            }
            assert!(home.backups().is_empty(), "{label}: no copy");
        }
        Err(report) => {
            assert_eq!(code, 1, "{label}: Rust refused ({report}), Go did not");
            assert_eq!(home.content().as_deref(), before, "{label}: left as it was");
            let copied = stderr.contains("backed up to");
            assert_eq!(
                report.code() == "agent-configs-corrupt",
                copied,
                "{label}: {report}"
            );
            let want: Vec<Vec<u8>> = if copied {
                vec![before.unwrap_or_default().to_vec()]
            } else {
                vec![]
            };
            assert_eq!(home.backups(), want, "{label}: the copy");
        }
    }
}

#[test]
fn test_an_install_does_to_each_file_what_gos_did() {
    for case in Case::all() {
        let before = case.file("before.json");
        let mut home = Home::new(&case, before.as_deref());
        let answered = home.register(Some(&server()));
        let want = case.file("install.json");
        hold(
            &case,
            "install",
            &home,
            &answered,
            case.outcome("install.out"),
            before.as_deref(),
            want,
        );
    }
}

#[test]
fn test_an_uninstall_with_no_record_does_what_gos_did_with_none() {
    for case in Case::all() {
        let Some(installed) = case
            .file("install.json")
            .or_else(|| case.file("before.json"))
        else {
            continue;
        };
        let mut home = Home::new(&case, Some(&installed));
        let answered = home.register(None);
        hold(
            &case,
            "uninstall",
            &home,
            &answered,
            case.outcome("uninstall.out"),
            Some(&installed),
            case.file("uninstall.json"),
        );
    }
}

#[test]
fn test_an_uninstall_after_gos_install_does_what_gos_uninstall_did() {
    let mut held = 0;
    for case in Case::all() {
        let (Some(record), Some(installed)) = (case.file("record.yaml"), case.file("install.json"))
        else {
            continue;
        };
        let mut home = Home::new(&case, Some(&installed));
        let target = home.path.display().to_string();
        let records = home
            .configs
            .home
            .clone()
            .unwrap()
            .join(".ctxloom/records/taskloom");
        let record = String::from_utf8(record)
            .unwrap()
            .replace("$TARGET", &target);
        let name = format!(
            "{}__20261021T000000.000000000Z.hew-record.yaml",
            records::flat_name(os::host().flavor(), &target)
        );
        write(&*home.fs, &records.join(name), record);
        let answered = home.register(None);
        assert!(answered.is_ok(), "{}: {answered:?}", case.name);
        assert_eq!(
            home.content()
                .map(|c| String::from_utf8_lossy(&c).into_owned()),
            case.file("recorded.json")
                .map(|c| String::from_utf8_lossy(&c).into_owned()),
            "{}: Go's own uninstall",
            case.name
        );
        held += 1;
    }
    assert!(held > 10, "{held} cases with Go's record");
}

#[test]
fn test_a_check_reads_each_file_as_gos_did() {
    for case in Case::all() {
        let before = case.file("before.json");
        let mut home = Home::new(&case, before.as_deref());
        let path = home.path.clone();
        let found = home.configs.installed(CLAUDE_CODE, &path, "taskloom");
        let (code, stdout) = case.outcome("check.out");
        assert_eq!(code, 0);
        let row = stdout.lines().nth(1).unwrap_or_default();
        let label = &case.name;
        match found {
            Ok(None) => assert_eq!(row, "", "{label}"),
            Ok(Some(true)) => assert!(row.contains(" registered ("), "{label}: {row}"),
            Ok(Some(false)) => assert!(row.contains(" not registered ("), "{label}: {row}"),
            Err(report) => {
                assert!(row.contains(" unreadable: "), "{label}: {row}");
                assert_eq!(report.code(), "agent-configs-parse", "{label}");
            }
        }
    }
}

// ------------------------------------------------------------ the rest

/// A filesystem of the test's own, a home on it, and the adapter there.
fn files() -> (Arc<dyn Fs>, PathBuf, AgentConfigFiles) {
    let fs = rooted();
    let home = os::fixture("/home/u");
    fs.create_dir_all(&home, Mode::DIR).unwrap();
    let configs = AgentConfigFiles {
        fs: Arc::clone(&fs),
        home: Some(home.clone()),
        path: None,
        pathext: None,
        waits: file_lock::Waits::DEFAULT,
    };
    (fs, home, configs)
}

/// The adapter over `fs` with no home, PATH `path`.
fn homeless(fs: &Arc<dyn Fs>, path: Option<std::ffi::OsString>) -> AgentConfigFiles {
    AgentConfigFiles {
        fs: Arc::clone(fs),
        home: None,
        path,
        pathext: None,
        waits: file_lock::Waits::DEFAULT,
    }
}

/// Register taskloom's server in the file at `path`, as a dry run or not.
fn install(
    configs: &mut AgentConfigFiles,
    path: &Path,
    dry_run: bool,
) -> Result<ports::Rewritten, annals_diagnostic::Report> {
    configs.register(
        CLAUDE_CODE,
        path,
        "taskloom",
        "taskloom",
        Some(&server()),
        dry_run,
    )
}

#[test]
fn test_claude_code_is_the_one_agent_and_its_files_are_gos() {
    let (fs, home, mut configs) = files();
    assert_eq!(configs.engines(), ["claude-code"]);
    assert_eq!(
        configs.config_path(CLAUDE_CODE, &Scope::User).unwrap(),
        home.join(".claude.json")
    );
    assert_eq!(
        configs
            .config_path(CLAUDE_CODE, &Scope::Project(PathBuf::from(".")))
            .unwrap(),
        PathBuf::from(".mcp.json"),
        "Go's filepath.Join"
    );
    let unknown = configs.config_path("codex", &Scope::User).unwrap_err();
    assert_eq!(unknown.code(), "agent-configs-unknown-engine");
    let path = home.join(".claude.json");
    let refused = configs
        .register("codex", &path, "taskloom", "taskloom", None, false)
        .unwrap_err();
    assert_eq!(refused.code(), "agent-configs-unknown-engine");
    let homeless = homeless(&fs, None)
        .config_path(CLAUDE_CODE, &Scope::User)
        .unwrap_err();
    assert_eq!(homeless.code(), "agent-configs-no-home");
    // The variable read on this host: HOME, or USERPROFILE on Windows.
    assert_eq!(named_var(&homeless), os::host().flavor().home_var());
}

/// The variable a no-home error names, as its `var` arg.
fn named_var(error: &dyn Diagnostic) -> String {
    match &error.args()[..] {
        [("var", annals_diagnostic::Arg::Str(var))] => var.clone(),
        other => panic!("no var: {other:?}"),
    }
}

#[test]
fn test_an_agent_is_present_where_its_file_or_its_directory_is() {
    let (fs, home, mut configs) = files();
    let project = &*os::fixture("/work");
    fs.create_dir_all(project, Mode::DIR).unwrap();
    let scope = Scope::Project(project.to_path_buf());
    assert!(!configs.present(CLAUDE_CODE, &Scope::User));
    assert!(!configs.present(CLAUDE_CODE, &scope));
    fs.create_dir_all(&home.join(".claude"), Mode::DIR).unwrap();
    assert!(configs.present(CLAUDE_CODE, &Scope::User));
    write(&*fs, &project.join(".mcp.json"), "{}");
    assert!(configs.present(CLAUDE_CODE, &scope));
    assert!(!configs.present("codex", &Scope::User));
    assert!(!homeless(&fs, None).present(CLAUDE_CODE, &Scope::User));
    let other = &*os::fixture("/other");
    write(&*fs, &other.join(".claude.json"), "{}");
    let mut configs = AgentConfigFiles {
        home: Some(other.to_path_buf()),
        ..homeless(&fs, None)
    };
    assert!(configs.present(CLAUDE_CODE, &Scope::User));
}

#[test]
fn test_a_relative_project_is_looked_for_in_the_filesystems_working_directory() {
    let fs: Arc<dyn Fs> = Arc::new(Rooted::temp_with_cwd(&os::fixture("/work")).unwrap());
    write(&*fs, &os::fixture("/work/.mcp.json"), "{}");
    let mut configs = homeless(&fs, None);
    assert!(configs.present(CLAUDE_CODE, &Scope::Project(PathBuf::from("."))));
}

#[test]
fn test_a_dry_run_answers_the_document_and_writes_nothing() {
    let (fs, home, mut configs) = files();
    let path = home.join(".claude.json");
    write(&*fs, &path, "{}");
    let rewritten = install(&mut configs, &path, true).unwrap();
    assert!(rewritten.changed);
    assert_eq!(
        rewritten.after,
        r#"{ "mcpServers": { "taskloom": { "args": ["mcp"], "command": "taskloom" } } }"#
    );
    assert_eq!(text(&*fs, &path), "{}");
}

#[test]
fn test_a_rewrite_that_changes_nothing_writes_nothing() {
    let (fs, home, mut configs) = files();
    let path = home.join(".claude.json");
    install(&mut configs, &path, false).unwrap();
    let written = fs.metadata(&path).unwrap().modified;
    write(&*fs, &home.join("marker"), "");
    let again = install(&mut configs, &path, false).unwrap();
    assert!(!again.changed);
    assert_eq!(fs.metadata(&path).unwrap().modified, written);
    let entries: Vec<_> = fs
        .read_dir(&home)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .filter(|name| name != ".ctxloom")
        .collect();
    assert_eq!(
        entries.len(),
        2,
        "no file staged and left behind: {entries:?}"
    );
}

#[test]
fn test_a_write_keeps_the_files_permissions_and_a_new_file_is_private() {
    if os::host().flavor() != os::Flavor::Unix {
        return;
    }
    let (fs, home, mut configs) = files();
    let made = home.join("deeper/.mcp.json");
    install(&mut configs, &made, false).unwrap();
    let mode = |p: &Path| fs.metadata(p).unwrap().mode.unwrap().bits();
    assert_eq!(mode(&made), 0o600);
    let kept = home.join(".claude.json");
    write(&*fs, &kept, "{}");
    fs.set_permissions(&kept, Mode::from_bits(0o640)).unwrap();
    install(&mut configs, &kept, false).unwrap();
    assert_eq!(mode(&kept), 0o640);
}

#[test]
fn test_a_corrupt_files_copy_is_private() {
    if os::host().flavor() != os::Flavor::Unix {
        return;
    }
    let (fs, home, mut configs) = files();
    let path = home.join(".claude.json");
    write(&*fs, &path, "not json");
    let refused = install(&mut configs, &path, false).unwrap_err();
    assert_eq!(refused.code(), "agent-configs-corrupt");
    let copy = fs
        .read_dir(&home)
        .unwrap()
        .into_iter()
        .find(|e| {
            e.name
                .to_string_lossy()
                .starts_with(".claude.json.corrupt-")
        })
        .expect("the copy");
    let copy = home.join(copy.name);
    assert_eq!(fs.read(&copy).unwrap(), b"not json");
    assert_eq!(fs.metadata(&copy).unwrap().mode, Some(Mode::PRIVATE_FILE));
}

#[test]
fn test_a_file_that_cannot_be_read_or_written_says_so() {
    if os::host().flavor() != os::Flavor::Unix {
        return;
    }
    let (fs, home, mut configs) = files();
    let dir = home.join("a-directory");
    fs.create_dir_all(&dir, Mode::DIR).unwrap();
    let unreadable = install(&mut configs, &dir, false).unwrap_err();
    assert_eq!(unreadable.code(), "agent-configs-read");
    let found = configs
        .installed(CLAUDE_CODE, &dir, "taskloom")
        .unwrap_err();
    assert_eq!(found.code(), "agent-configs-read");

    let locked = home.join("locked");
    write(&*fs, &locked.join(".mcp.json"), "{}");
    fs.set_permissions(&locked, Mode::from_bits(0o500)).unwrap();
    let unwritable = install(&mut configs, &locked.join(".mcp.json"), false);
    fs.set_permissions(&locked, Mode::PRIVATE_DIR).unwrap();
    assert_eq!(unwritable.unwrap_err().code(), "agent-configs-write");
    assert_eq!(text(&*fs, &locked.join(".mcp.json")), "{}");
}

#[test]
fn test_an_entry_running_another_program_is_refused_by_name() {
    let (fs, home, mut configs) = files();
    let path = home.join(".claude.json");
    let src = r#"{"mcpServers": {"taskloom": {"command": "mywrapper"}}}"#;
    write(&*fs, &path, src);
    let refused = install(&mut configs, &path, false).unwrap_err();
    assert_eq!(refused.code(), "agent-configs-foreign");
    assert!(refused.to_string().contains("named taskloom"), "{refused}");
    assert_eq!(text(&*fs, &path), src);
}

#[test]
fn test_a_command_resolves_where_an_executable_of_its_name_is_on_the_path() {
    let fs = rooted();
    let (bin, other) = (&*os::fixture("/bin"), &*os::fixture("/other"));
    for dir in [bin, other] {
        fs.create_dir_all(dir, Mode::DIR).unwrap();
    }
    let mut configs = homeless(&fs, Some(std::env::join_paths([other, bin]).unwrap()));
    assert!(!configs.resolvable("taskloom"));
    let program = bin.join(os::host().flavor().program_file("taskloom"));
    write(&*fs, &program, "#!/bin/sh\n");
    if os::host().flavor() == os::Flavor::Unix {
        assert!(!configs.resolvable("taskloom"), "not executable");
        fs.set_permissions(&program, Mode::from_bits(0o755))
            .unwrap();
    }
    assert!(configs.resolvable("taskloom"));
    fs.create_dir_all(&other.join("dir"), Mode::DIR).unwrap();
    assert!(!configs.resolvable("dir"), "a directory runs nothing");
    assert!(!homeless(&fs, None).resolvable("taskloom"));
}

#[test]
fn test_an_empty_path_entry_never_means_the_working_directory() {
    // POSIX reads an empty entry as `.`: a program planted in whatever
    // directory taskloom runs in would be found. It is dropped instead.
    let fs: Arc<dyn Fs> = Arc::new(Rooted::temp_with_cwd(&os::fixture("/work")).unwrap());
    let (work, bin) = (&*os::fixture("/work"), &*os::fixture("/bin"));
    fs.create_dir_all(bin, Mode::DIR).unwrap();
    let planted = work.join(os::host().flavor().program_file("taskloom"));
    write(&*fs, &planted, "#!/bin/sh\n");
    fs.set_permissions(&planted, Mode::from_bits(0o755))
        .unwrap();
    let path = std::env::join_paths([Path::new(""), bin]).unwrap();
    assert!(!homeless(&fs, Some(path)).resolvable("taskloom"));
    let named = std::env::join_paths([work, bin]).unwrap();
    assert!(
        homeless(&fs, Some(named)).resolvable("taskloom"),
        "the directory named is searched"
    );
}

#[test]
fn test_a_path_is_joined_and_cleaned_as_gos() {
    for (dir, want) in [
        (".", ".mcp.json"),
        ("./", ".mcp.json"),
        ("/home/me/", "/home/me/.mcp.json"),
        ("/home/me/./work/../x", "/home/me/x/.mcp.json"),
        ("../up", "../up/.mcp.json"),
        ("/..", "/.mcp.json"),
        ("a/../..", "../.mcp.json"),
    ] {
        assert_eq!(
            go_join(Path::new(dir), ".mcp.json"),
            PathBuf::from(want),
            "{dir}"
        );
    }
    assert_eq!(go_join(Path::new(""), ""), PathBuf::from("."));
}

// ------------------------------------------------------------ the lock

/// Whether some holder keeps `path` from being locked exclusively now.
fn held(fs: &dyn Fs, path: &Path) -> bool {
    match fs.open(path, Open::Read) {
        Ok(file) => fs4::FileExt::try_lock(&file).is_err(),
        Err(_) => false,
    }
}

/// A home, a project directory, and the adapter over them waiting `wait`
/// for a held lock; the project's `.mcp.json`, and the sidecar lock taken
/// for it, held here as another process holding it would.
struct Locking {
    fs: Arc<dyn Fs>,
    configs: AgentConfigFiles,
    path: PathBuf,
    sidecar: file_lock::path::Sidecar,
    holder: Option<std::fs::File>,
}

impl Locking {
    fn new(wait: std::time::Duration) -> Locking {
        let fs = rooted();
        let (home, dir) = (&*os::fixture("/home/u"), &*os::fixture("/work"));
        for made in [home, dir] {
            fs.create_dir_all(made, Mode::DIR).unwrap();
        }
        let configs = AgentConfigFiles {
            fs: Arc::clone(&fs),
            home: Some(home.to_path_buf()),
            path: None,
            pathext: None,
            waits: file_lock::Waits {
                lock: wait,
                reap_interlock: wait,
            },
        };
        let path = dir.join(".mcp.json");
        let sidecar = file_lock::path::sidecar_for(&*fs, home, &path);
        assert_eq!(
            sidecar.lock.parent().unwrap(),
            home.join(".ctxloom/locks"),
            "Go's home lock directory"
        );
        fs.create_dir_all(sidecar.lock.parent().unwrap(), Mode::DIR)
            .unwrap();
        let holder = fs.open(&sidecar.lock, Open::Create(Mode::FILE)).unwrap();
        fs4::FileExt::lock(&holder).unwrap();
        Locking {
            fs,
            configs,
            path,
            sidecar,
            holder: Some(holder),
        }
    }
}

#[test]
fn test_a_registration_waits_while_another_process_holds_the_files_lock_then_writes() {
    let mut at = Locking::new(std::time::Duration::from_secs(60));
    let interlock = at.sidecar.interlock.clone().unwrap();
    let (path, mut configs) = (at.path.clone(), at.configs.clone());
    let registering = std::thread::spawn(move || install(&mut configs, &path, false));
    // The registration has begun its lock once it holds the reap interlock
    // (shared); from then until the sidecar is let go it can write nothing.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !held(&*at.fs, &interlock) {
        assert!(std::time::Instant::now() < deadline, "never began its lock");
        assert!(!registering.is_finished(), "{:?}", registering.join());
        std::thread::yield_now();
    }
    assert!(
        !at.fs.exists(&at.path).unwrap(),
        "nothing written while the lock is held"
    );
    at.holder = None;
    let rewritten = registering.join().unwrap().expect("written once let go");
    assert!(rewritten.changed);
    assert!(text(&*at.fs, &at.path).contains("\"taskloom\""));
    assert!(!held(&*at.fs, &at.sidecar.lock), "released after the write");
    assert!(!held(&*at.fs, &interlock));
}

#[test]
fn test_a_registration_whose_file_stays_locked_past_the_wait_is_refused_and_writes_nothing() {
    let mut at = Locking::new(std::time::Duration::from_millis(30));
    write(&*at.fs, &at.path, "{}");
    for server in [Some(server()), None] {
        let path = at.path.clone();
        let refused = at
            .configs
            .register(
                CLAUDE_CODE,
                &path,
                "taskloom",
                "taskloom",
                server.as_ref(),
                false,
            )
            .unwrap_err();
        assert_eq!(refused.code(), "file-lock-timeout");
        assert_eq!(text(&*at.fs, &at.path), "{}");
    }
    drop(at.holder.take());
    let path = at.path.clone();
    install(&mut at.configs, &path, false).expect("written once let go");
    assert!(text(&*at.fs, &at.path).contains("\"taskloom\""));
}

#[test]
fn test_a_registration_with_no_home_has_no_lock_to_take_and_is_refused() {
    let fs = rooted();
    let mut homeless = homeless(&fs, None);
    let path = &*os::fixture("/work/.mcp.json");
    let refused = install(&mut homeless, path, true).unwrap_err();
    assert_eq!(refused.code(), "agent-configs-no-home");
    assert!(!fs.exists(path).unwrap());
}
