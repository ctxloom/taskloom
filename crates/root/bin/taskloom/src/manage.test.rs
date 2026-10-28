//! Acceptance: `taskloom manage`, the real binary, only ever in a
//! temporary home with an empty PATH: it writes the recorded bytes into
//! `~/.claude.json` and a project's `.mcp.json`, says what the recorded run
//! says, and reads back what it wrote. Nothing outside the temporary home
//! and project directory is read or written.

use std::path::Path;

use super::{disk, one_document, Env, Isolated, Ran, Temp};

/// The recorded case `name`'s file `file`.
fn go(name: &str, file: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../adapters/driven/agent-configs/fixtures/go-manage")
        .join(name)
        .join(file);
    disk::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `taskloom manage <args>` in `env`, nothing on its PATH.
fn manage(env: &Env, path: &Temp, args: &[&str]) -> Ran {
    let mut all = vec!["manage"];
    all.extend_from_slice(args);
    env.run_with(&all, &[("PATH", path.path().to_str().unwrap())])
}

/// The recorded stderr for the case, its exit code dropped, `$HOME` and
/// `$DIR` put back, each with the separator after it the host's (the
/// recording is unix's: `$HOME/.claude.json`).
fn go_said(name: &str, file: &str, env: &Env) -> String {
    let text = String::from_utf8(go(name, file)).unwrap();
    let (_, said) = text.split_once('\n').unwrap();
    let sep = std::path::MAIN_SEPARATOR;
    said.replace("$HOME/", &format!("{}{sep}", env.home.path().display()))
        .replace("$DIR/", &format!("{}{sep}", env.project.path().display()))
        .replace("$HOME", &env.home.path().display().to_string())
        .replace("$DIR", &env.project.path().display().to_string())
}

#[test]
fn test_install_and_uninstall_write_gos_bytes_and_say_gos_words() {
    let env = Env::new("manage-harp");
    let empty = Temp::new();
    let config = env.home.path().join(".claude.json");
    disk::write(&config, go("indented-other-server", "before.json")).unwrap();

    let ran = manage(&env, &empty, &["install", "--format", "text"]);
    assert!(ran.ok, "{}", ran.stderr);
    assert_eq!(ran.stdout, "");
    assert_eq!(
        ran.stderr,
        go_said("indented-other-server", "install.out", &env)
    );
    assert_eq!(
        disk::read(&config).unwrap(),
        go("indented-other-server", "install.json")
    );

    let ran = manage(&env, &empty, &["check", "--format", "text"]);
    assert_eq!(
        ran.stdout,
        format!(
            "binary: taskloom NOT on PATH\nclaude-code  user     registered ({})\n",
            config.display()
        )
    );

    let ran = manage(&env, &empty, &["uninstall", "--format", "text"]);
    assert!(ran.ok, "{}", ran.stderr);
    assert_eq!(
        ran.stderr,
        go_said("indented-other-server", "uninstall.out", &env)
    );
    assert_eq!(
        disk::read(&config).unwrap(),
        go("indented-other-server", "before.json")
    );
}

#[test]
fn test_an_engine_named_makes_its_file_and_project_is_a_switch() {
    let env = Env::new("manage-project-harp");
    let empty = Temp::new();
    let ran = manage(&env, &empty, &["install", "--format", "text"]);
    assert!(!ran.ok);
    assert_eq!(
        ran.stderr,
        "taskloom: no agent backends detected; name one with --engine (claude-code)\n"
    );
    let ran = manage(
        &env,
        &empty,
        &["install", "--engine", "claude-code", "--json"],
    );
    assert!(ran.ok, "{}", ran.stderr);
    assert_eq!(one_document(&ran.stdout)["configs"][0]["written"], true);
    assert_eq!(
        disk::read(env.home.path().join(".claude.json")).unwrap(),
        go("absent", "install.json")
    );

    disk::write(
        env.project.path().join(".mcp.json"),
        go("project-scope", "before.json"),
    )
    .unwrap();
    let ran = manage(&env, &empty, &["install", "--project", "--format", "text"]);
    assert!(ran.ok, "{}", ran.stderr);
    assert!(
        ran.stderr.ends_with("  config: .mcp.json\n"),
        "{}",
        ran.stderr
    );
    assert_eq!(
        disk::read(env.project.path().join(".mcp.json")).unwrap(),
        go("project-scope", "install.json")
    );
    let ran = manage(&env, &empty, &["check", "--json"]);
    let doc = one_document(&ran.stdout);
    assert_eq!(doc["binary_on_path"], false);
    assert_eq!(doc["configs"].as_array().unwrap().len(), 2);

    // Every other command's --project still names a project.
    let listed = env.run(&["list", "--project", "jolly-mute-magma", "--json"]);
    assert!(listed.ok, "{}", listed.stderr);
}

#[test]
fn test_a_corrupt_file_is_copied_aside_and_left_alone() {
    let env = Env::new("manage-corrupt-harp");
    let empty = Temp::new();
    let config = env.home.path().join(".claude.json");
    let broken = go("corrupt", "before.json");
    disk::write(&config, &broken).unwrap();
    let ran = manage(&env, &empty, &["install", "--json"]);
    assert!(!ran.ok);
    assert_eq!(ran.stdout, "");
    let envelope = one_document(&ran.stderr);
    assert_eq!(envelope["code"], "registration-application-engine-failed");
    assert_eq!(envelope["args"]["engine"], "claude-code");
    assert_eq!(envelope["args"]["error"]["code"], "agent-configs-corrupt");
    assert_eq!(disk::read(&config).unwrap(), broken);
    let copies: Vec<_> = disk::read_dir(env.home.path())
        .unwrap()
        .into_iter()
        .filter_map(|path| {
            path.file_name()?
                .to_string_lossy()
                .starts_with(".claude.json.corrupt-")
                .then(|| disk::read(&path).unwrap())
        })
        .collect();
    assert_eq!(copies, [broken]);
}

/// Whether some holder keeps `path` from being locked exclusively now.
fn held(path: &Path) -> bool {
    match disk::open(path) {
        Ok(file) => fs4::FileExt::try_lock(&file).is_err(),
        Err(_) => false,
    }
}

#[test]
fn test_manage_takes_gos_lock_on_the_file_and_waits_its_turn() {
    let env = Env::new("manage-lock-harp");
    let empty = Temp::new();
    let config = env.home.path().join(".claude.json");
    // The sidecar lock taken for the file:
    // ~/.ctxloom/locks/<FlatName(path)>.lock.
    let locks = env.home.path().join(".ctxloom/locks");
    let sidecar = locks.join(format!(
        "{}.lock",
        file_lock::path::flat_name(os::host().flavor(), &config.display().to_string())
    ));
    let interlock = locks.join("reap.interlock");
    // Claude Code is in use here.
    disk::create_dir_all(env.home.path().join(".claude")).unwrap();

    // Uncontended, it makes the sidecar, recording the file it guards.
    let ran = manage(&env, &empty, &["install", "--json"]);
    assert!(ran.ok, "{}", ran.stderr);
    assert_eq!(
        disk::read_to_string(&sidecar).unwrap(),
        format!("{}\n", config.display())
    );
    disk::remove_file(&config).unwrap();

    // Held here, as another ctxloom-family binary mid-write holds it.
    let holder = disk::open(&sidecar).unwrap();
    fs4::FileExt::lock(&holder).unwrap();
    let mut child = std::process::Command::new(super::binary())
        .args(["manage", "install", "--json"])
        .current_dir(env.project.path())
        .isolated(env.home.path())
        .env("PATH", empty.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // It has begun its lock once it holds the reap interlock (shared); it
    // can write nothing until the sidecar is let go.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !held(&interlock) {
        assert!(std::time::Instant::now() < deadline, "never began its lock");
        assert!(
            child.try_wait().unwrap().is_none(),
            "ended without the lock"
        );
        std::thread::yield_now();
    }
    assert!(
        !disk::exists(&config),
        "nothing written while Go's lock is held"
    );
    assert!(child.try_wait().unwrap().is_none(), "still waiting");
    drop(holder);
    assert!(child.wait().unwrap().success());
    assert!(disk::read_to_string(&config)
        .unwrap()
        .contains("\"taskloom\""));
    assert!(!held(&sidecar), "let go once written");
}

/// `manage` is not a task command: it runs where the task store and the
/// project registry cannot be reached (`~/.ctxloom` a file, so nothing
/// under it opens) and in a directory no project is bound to, because
/// nothing of the tasks' wiring is built for it.
#[test]
fn test_manage_builds_nothing_of_the_tasks() {
    let env = Env::new("manage-no-tasks-harp");
    let empty = Temp::new();
    disk::write(env.home.path().join(".ctxloom"), "not a directory").unwrap();
    let ran = manage(&env, &empty, &["check", "--format", "text"]);
    assert!(ran.ok, "{}", ran.stderr);
    assert_eq!(ran.stdout, "binary: taskloom NOT on PATH\n");
    assert!(!disk::exists(env.project.path().join(".project-id")));
}
