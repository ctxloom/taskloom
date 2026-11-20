//! Acceptance: the retired `ctxloom:plan` tag stays refused on a task, in
//! the real binary in a temporary home and work tree. (The plan commands
//! themselves are `plans_files.test.rs`.)

use std::path::PathBuf;
use std::process::Command;

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
    /// root and the session set.
    fn run(&self, args: &[&str]) -> Ran {
        let output = Command::new(binary())
            .args(args)
            .current_dir(self.root())
            .isolated(self.home.path())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("CTXLOOM_ROOT", self.root())
            .env("CTXLOOM_SESSION_HARP", "inept-sixth-stick")
            .output()
            .expect("taskloom runs");
        Ran::of(output)
    }
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
