//! Acceptance: `taskloom plan`, the real binary in a temporary home and
//! work tree: a plan added, listed with the VS Code extension's fields,
//! shown as JSON and as its markdown, linked to a task, its file edited
//! (the hash tracks it) and removed (check says so), and the `ctxloom:plan`
//! tags an imported Go log carries turned into plans.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use super::{binary, disk, Isolated, Ran, Temp};

/// A temporary home, and a work tree (CTXLOOM_ROOT) beside it.
struct World {
    home: Temp,
    tree: Temp,
}

impl World {
    fn new() -> World {
        World {
            home: Temp::new(),
            tree: Temp::new(),
        }
    }

    fn root(&self) -> PathBuf {
        disk::canonicalize(self.tree.path()).unwrap()
    }

    /// `taskloom args` in the work tree, with only HOME, PATH, the work
    /// root, the session and `vars` set.
    fn run_with(&self, args: &[&str], vars: &[(&str, &str)]) -> Ran {
        let output = Command::new(binary())
            .args(args)
            .current_dir(self.root())
            .isolated(self.home.path())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("CTXLOOM_ROOT", self.root())
            .env("CTXLOOM_SESSION_HARP", "inept-sixth-stick")
            .envs(vars.iter().copied())
            .output()
            .expect("taskloom runs");
        Ran::of(output)
    }

    fn run(&self, args: &[&str]) -> Ran {
        self.run_with(args, &[])
    }

    /// `taskloom args --json`, which must succeed: its document.
    fn json(&self, args: &[&str]) -> Value {
        let mut args = args.to_vec();
        args.push("--json");
        let ran = self.run(&args);
        assert!(ran.ok, "{args:?}: {}", ran.stderr);
        ran.json()
    }

    fn path(&self, rel: &str) -> PathBuf {
        rel.split('/')
            .fold(self.root(), |path, part| path.join(part))
    }
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_else(|| panic!("{value} is text"))
}

#[test]
fn a_plan_added_is_written_listed_shown_linked_and_tracked() {
    let world = World::new();
    let task = world.json(&["add", "Move plans into taskloom"]);
    let task = text(&task["harp_id"]).to_string();

    let added = world.json(&["plan", "add", "--title", "Plans", "--governs", &task]);
    let id = text(&added["id"]).to_string();
    let rel = format!(".taskloom/plans/{id}.plan.md");
    assert_eq!(added["rel_path"], rel.as_str());
    assert_eq!(added["status"], "draft");
    assert_eq!(added["governs"], serde_json::json!([task]));
    let file = world.path(&rel);
    let written = disk::read_to_string(&file).expect("the plan's file");
    assert!(written.contains("title: Plans"), "{written}");

    // The VS Code extension's fields, off a terminal.
    let listed = world.run(&["plan", "list"]);
    assert!(listed.ok, "{}", listed.stderr);
    let first = &listed.json()[0];
    assert_eq!(first["path"], file.to_string_lossy().as_ref());
    assert_eq!(first["name"], id.as_str());
    assert_eq!(first["title"], "Plans");
    assert_eq!(first["session"], "inept-sixth-stick");
    assert_eq!(first["sessions"], serde_json::json!(["inept-sixth-stick"]));

    // Off a terminal, JSON; its text is the markdown, raw.
    let shown = world.run(&["plan", "show", &file.to_string_lossy()]);
    assert!(shown.ok, "{}", shown.stderr);
    assert_eq!(shown.json()["content"], written.as_str());
    let raw = world.run(&["plan", "show", &id, "--format", "text"]);
    assert_eq!(raw.stdout, written);

    // The hash tracks an edit.
    disk::write(&file, "# Plans, edited\n").unwrap();
    let checked = world.run(&["plan", "check"]);
    assert!(checked.ok, "{}", checked.stderr);
    assert_eq!(
        checked.json(),
        serde_json::json!({"ok": true, "findings": []})
    );
    let after = world.json(&["plan", "show", &id]);
    assert_ne!(after["content_hash"], added["content_hash"]);

    let moved = world.json(&["plan", "status", &id, "active"]);
    assert_eq!(moved["status"], "active");
    let governing = world.json(&["plan", "list", "--governs", &task]);
    assert_eq!(governing[0]["id"], id.as_str());
    let shown = world.run(&["show", &task, "--format", "text"]);
    assert!(
        shown.stdout.contains(&format!("\n    plans: {id}\n")),
        "{}",
        shown.stdout
    );

    // A missing file is drift: exit 1, named.
    disk::remove_file(&file).unwrap();
    let missing = world.run(&["plan", "check", "--format", "text"]);
    assert!(!missing.ok);
    assert_eq!(missing.stdout, format!("missing: {id} {rel}\n"));
}

#[test]
fn a_usage_error_the_parser_cannot_see_exits_2() {
    let world = World::new();
    let refused = world.run(&["plan", "add", "--title", "T", "--status", "nope"]);
    assert_eq!(refused_code(&refused), 2);
    assert!(refused.stderr.contains("nope"), "{}", refused.stderr);
    let unknown = world.run(&["plan", "show", "no-such-plan"]);
    assert_eq!(refused_code(&unknown), 1);
}

fn refused_code(ran: &Ran) -> i32 {
    assert!(!ran.ok);
    ran.code
}

/// A Go task log under the home whose tasks carry `ctxloom:plan` tags: one
/// naming a plan in the work tree, one outside it, one a dead path.
fn go_log(home: &Path, project: &str, inside: &Path, outside: &Path) {
    let tag = |path: &Path| {
        serde_json::to_string(&format!("ctxloom:plan=\"{}\"", path.display())).unwrap()
    };
    let lines = [
        format!(
            r#"{{"op":"add","task":"brisk-heron","text":"Inside","tags":[{}],"ts":"2026-03-01T10:00:00Z"}}"#,
            tag(inside)
        ),
        format!(
            r#"{{"op":"add","task":"quiet-maple","text":"Outside","tags":[{}],"ts":"2026-03-01T10:01:00Z"}}"#,
            tag(outside)
        ),
        format!(
            r#"{{"op":"add","task":"tidy-otter","text":"Dead","tags":[{}],"ts":"2026-03-01T10:02:00Z"}}"#,
            tag(&outside.with_file_name("gone.plan.md"))
        ),
    ];
    let tasks = home.join(".ctxloom/tasks");
    disk::create_dir_all(&tasks).unwrap();
    disk::write(
        tasks.join(format!("{project}.jsonl")),
        lines.join("\n") + "\n",
    )
    .unwrap();
}

#[test]
fn import_turns_ctxloom_plan_tags_into_plans_and_the_tag_is_retired_after() {
    let world = World::new();
    let project = "jolly-mute-magma";
    let pinned = [("CTXLOOM_PROJECT_ID", project)];
    let inside = world.path("docs/design.plan.md");
    disk::create_dir_all(inside.parent().unwrap()).unwrap();
    disk::write(&inside, "---\ntitle: Design\n---\n# Design\n").unwrap();
    let elsewhere = Temp::new();
    let outside = disk::canonicalize(elsewhere.path())
        .unwrap()
        .join("v1-removal.plan.md");
    disk::write(&outside, "# V1 removal\n").unwrap();
    go_log(world.home.path(), project, &inside, &outside);
    let imported = world.run_with(&["import", "--json"], &pinned);
    assert!(imported.ok, "{}", imported.stderr);

    let dry = world.run_with(&["plan", "import", "--dry-run", "--json"], &pinned);
    assert_eq!(dry.json()["plans"].as_array().unwrap().len(), 2);
    assert!(world.run_with(&["plan", "list", "--json"], &pinned).json()[0].is_null());

    let ran = world.run_with(&["plan", "import", "--json"], &pinned);
    assert_eq!(ran.code, 1, "a dead path was left: {}", ran.stderr);
    let report = ran.json();
    assert_eq!(report["plans"][0]["path"], "docs/design.plan.md");
    assert_eq!(
        report["plans"][0]["governs"],
        serde_json::json!(["brisk-heron"])
    );
    assert_eq!(report["plans"][1]["external"], true);
    let copy = text(&report["plans"][1]["path"]).to_string();
    assert_eq!(
        disk::read_to_string(world.path(&copy)).unwrap(),
        "# V1 removal\n"
    );
    assert_eq!(disk::read_to_string(&outside).unwrap(), "# V1 removal\n");
    assert_eq!(
        report["dead"][0]["tasks"],
        serde_json::json!(["tidy-otter"])
    );
    assert_eq!(report["retired"].as_array().unwrap().len(), 2);

    let plans = world.run_with(&["plan", "list", "--json"], &pinned).json();
    assert_eq!(plans.as_array().unwrap().len(), 2);
    assert_eq!(plans[0]["status"], "active");

    // The tags migrated are gone; the dead one is kept.
    let shown = world.run_with(&["show", "brisk-heron", "tidy-otter", "--json"], &pinned);
    let shown = shown.json();
    assert!(!shown[0]["tags"].to_string().contains("ctxloom"), "{shown}");
    assert!(shown[1]["tags"].to_string().contains("ctxloom"), "{shown}");

    // Retired: a write adding it is refused.
    let refused = world.run_with(
        &["tag", "brisk-heron", "--add", "ctxloom:plan=\"x.plan.md\""],
        &pinned,
    );
    assert!(!refused.ok);
    assert!(
        refused.stderr.contains("ctxloom:plan"),
        "{}",
        refused.stderr
    );
}
