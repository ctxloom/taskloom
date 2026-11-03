//! Acceptance: `taskloom plans`, the real binary in a temporary home and
//! work tree: a plan added, listed with the VS Code extension's fields,
//! shown as JSON and as its markdown, linked to a task, its file edited
//! (the hash tracks it) and removed (check says so), and the retired
//! `ctxloom:plan` tag refused.

use std::path::PathBuf;
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
        args.extend(["--format", "json"]);
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

// `show` names the plans governing each task it shows, read in the
// task's own project's store, across projects too; a task no plan governs
// has no plans line.
#[test]
fn show_names_the_plans_governing_each_task_and_none_for_a_task_ungoverned() {
    let world = World::new();
    let governed = world.json(&["tasks", "add", "Move plans into taskloom"]);
    let governed = text(&governed["harp_id"]).to_string();
    let free = world.json(&["tasks", "add", "Tidy the docs"]);
    let free = text(&free["harp_id"]).to_string();
    let first = world.json(&["plans", "add", "--title", "Plans", "--governs", &governed]);
    let first = text(&first["id"]).to_string();
    let second = world.json(&["plans", "add", "--title", "More", "--governs", &governed]);
    let second = text(&second["id"]).to_string();
    let mut both = [first, second];
    both.sort();
    let line = format!("\n    plans: {}\n", both.join(", "));
    {
        let args = vec![
            "tasks",
            "show",
            governed.as_str(),
            free.as_str(),
            "--format",
            "text",
        ];
        let shown = world.run(&args);
        assert!(shown.ok, "{}", shown.stderr);
        assert_eq!(
            shown.stdout.matches("    plans: ").count(),
            1,
            "only the governed task's: {}",
            shown.stdout
        );
        assert!(shown.stdout.contains(&line), "{}", shown.stdout);
        let (governed_at, free_at) = (
            shown.stdout.find(&governed).unwrap(),
            shown.stdout.find(&free).unwrap(),
        );
        let plans_at = shown.stdout.find("    plans: ").unwrap();
        assert!(
            governed_at < plans_at && plans_at < free_at,
            "the line is the governed task's: {}",
            shown.stdout
        );
    }
}

#[test]
fn a_plan_added_is_written_listed_shown_linked_and_tracked() {
    let world = World::new();
    let task = world.json(&["tasks", "add", "Move plans into taskloom"]);
    let task = text(&task["harp_id"]).to_string();

    let added = world.json(&["plans", "add", "--title", "Plans", "--governs", &task]);
    let id = text(&added["id"]).to_string();
    let rel = format!(".taskloom/plans/{id}.plan.md");
    assert_eq!(added["rel_path"], rel.as_str());
    assert_eq!(added["status"], "draft");
    assert_eq!(added["governs"], serde_json::json!([task]));
    let file = world.path(&rel);
    let written = disk::read_to_string(&file).expect("the plan's file");
    assert!(written.contains("title: Plans"), "{written}");

    // The VS Code extension's fields, off a terminal.
    let listed = world.run(&["plans", "list"]);
    assert!(listed.ok, "{}", listed.stderr);
    let first = &listed.json()[0];
    assert_eq!(first["path"], file.to_string_lossy().as_ref());
    assert_eq!(first["name"], id.as_str());
    assert_eq!(first["title"], "Plans");
    assert_eq!(first["session"], "inept-sixth-stick");
    assert_eq!(first["sessions"], serde_json::json!(["inept-sixth-stick"]));

    // Off a terminal, JSON; its text is the markdown, raw.
    let shown = world.run(&["plans", "show", &file.to_string_lossy()]);
    assert!(shown.ok, "{}", shown.stderr);
    assert_eq!(shown.json()["content"], written.as_str());
    let raw = world.run(&["plans", "show", &id, "--format", "text"]);
    assert_eq!(raw.stdout, written);

    // The hash tracks an edit.
    disk::write(&file, "# Plans, edited\n").unwrap();
    let checked = world.run(&["plans", "check"]);
    assert!(checked.ok, "{}", checked.stderr);
    assert_eq!(
        checked.json(),
        serde_json::json!({"ok": true, "findings": []})
    );
    let after = world.json(&["plans", "show", &id]);
    assert_ne!(after["content_hash"], added["content_hash"]);

    let moved = world.json(&["plans", "status", &id, "active"]);
    assert_eq!(moved["status"], "active");
    let governing = world.json(&["plans", "list", "--governs", &task]);
    assert_eq!(governing[0]["id"], id.as_str());
    let shown = world.run(&["tasks", "show", &task, "--format", "text"]);
    assert!(
        shown.stdout.contains(&format!("\n    plans: {id}\n")),
        "{}",
        shown.stdout
    );

    // A missing file is drift: exit 1, named.
    disk::remove_file(&file).unwrap();
    let missing = world.run(&["plans", "check", "--format", "text"]);
    assert!(!missing.ok);
    assert_eq!(missing.stdout, format!("missing: {id} {rel}\n"));
}

#[test]
fn a_usage_error_the_parser_cannot_see_exits_2() {
    let world = World::new();
    let refused = world.run(&["plans", "add", "--title", "T", "--status", "nope"]);
    assert_eq!(refused_code(&refused), 2);
    assert!(refused.stderr.contains("nope"), "{}", refused.stderr);
    let unknown = world.run(&["plans", "show", "no-such-plan"]);
    assert_eq!(refused_code(&unknown), 1);
}

fn refused_code(ran: &Ran) -> i32 {
    assert!(!ran.ok);
    ran.code
}

/// `plan import` is removed (plans become files, so there is nothing to
/// import); the `ctxloom:plan` tag it retired stays refused on write.
#[test]
fn the_retired_ctxloom_plan_tag_is_refused_on_write() {
    let world = World::new();
    let added = world.run(&["tasks", "add", "Seed", "--format", "json"]);
    assert!(added.ok, "{}", added.stderr);
    let harp = added.json()["harp_id"].as_str().unwrap().to_string();
    let refused = world.run(&["tasks", "tag", "add", &harp, "ctxloom:plan=\"x.plan.md\""]);
    assert!(!refused.ok);
    assert!(
        refused.stderr.contains("ctxloom:plan"),
        "{}",
        refused.stderr
    );
}
