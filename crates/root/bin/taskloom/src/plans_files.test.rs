//! Acceptance: plans become plain files (consolidation slice C), through
//! the real binary in a temporary home and work tree. Red-first: these pin
//! the rulings of `cli-consolidation.plan.md`, "Plans leave the event
//! system": a plan is any `*.plan.md` under the configured roots; its YAML
//! frontmatter is the one authority for `id`, `title`, `status`, `tags`,
//! `governs`, `supersedes` and `sessions`; taskloom edits the frontmatter
//! and touches no other byte; the store keeps no record of a plan.
//!
//! Exit codes: 2 = invalid on its own content (bad arguments, an unknown
//! status, a tag no permit admits, a blank root); 1 = ran against state
//! and the answer was negative (no such task, a `check` finding, no such
//! plan).
//!
//! Anything this file spells that the plan does not settle is marked
//! PROPOSAL (pending owner review) at the test that spells it.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

use super::{binary, disk, Env, Isolated, Ran, Temp};

const SESSION: &str = "plans-files-harp";

/// How long a watch line may take before the stream is judged dead.
const PATIENCE: Duration = Duration::from_secs(30);

// ------------------------------------------------------------ helpers

/// A project whose store exists and admits every tag.
fn world() -> Env {
    let env = Env::new(SESSION);
    env.permit_every_tag();
    env
}

/// A project whose store exists with only its seeded permits (so
/// `human:decision` is admitted and `area:cli` is not).
fn seeded_world() -> Env {
    let env = Env::new(SESSION);
    env.seed("Seed");
    env
}

fn under(root: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .fold(root.to_path_buf(), |p, part| p.join(part))
}

/// `text` written to `rel` under the work tree; its absolute path.
fn write_plan(env: &Env, rel: &str, text: &str) -> PathBuf {
    let path = under(env.project.path(), rel);
    disk::create_dir_all(path.parent().unwrap()).unwrap();
    disk::write(&path, text).unwrap();
    path
}

fn read(path: &Path) -> String {
    disk::read_to_string(path).unwrap()
}

fn s(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A plan file: frontmatter of `id` and `fields` (each line ending in a
/// newline), then `body`.
fn plan_text(id: &str, fields: &str, body: &str) -> String {
    format!("---\nid: {id}\n{fields}---\n{body}")
}

/// `args` which must succeed: its one JSON document.
fn ok(env: &Env, args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.extend(["--format", "json"]);
    let ran = env.run(&args);
    assert!(ran.ok, "{args:?}: {}{}", ran.stdout, ran.stderr);
    ran.json()
}

/// `args` which must exit `code`.
fn exits(env: &Env, args: &[&str], code: i32) -> Ran {
    let ran = env.run(args);
    assert_eq!(ran.code, code, "{args:?}: {}{}", ran.stdout, ran.stderr);
    ran
}

fn add_task(env: &Env, text: &str) -> String {
    ok(env, &["tasks", "add", text])["harp_id"]
        .as_str()
        .expect("a harp id")
        .to_string()
}

/// The strings of a JSON array.
fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("{value} is an array"))
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("{v} is text"))
                .to_string()
        })
        .collect()
}

/// The `name` of every plan `plans list` gives, sorted.
fn listed_names(env: &Env, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["plans", "list"];
    args.extend(extra);
    let mut names = ok(env, &args)
        .as_array()
        .expect("an array")
        .iter()
        .map(|p| p["name"].as_str().expect("a name").to_string())
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// The lines of `new` that remain once `old`'s lines are matched, in
/// order, against it: what an edit added, if it removed and reordered
/// nothing. Panics if `old` is not a subsequence of `new`.
fn added_lines(old: &str, new: &str) -> Vec<String> {
    let mut old = old.lines().peekable();
    let mut added = Vec::new();
    for line in new.lines() {
        if old.peek() == Some(&line) {
            old.next();
        } else {
            added.push(line.to_string());
        }
    }
    assert!(
        old.peek().is_none(),
        "an edit lost or moved the line {:?}; after the edit:\n{new}",
        old.peek().unwrap()
    );
    added
}

/// `text` without its lines that start with `prefix`.
fn without_lines_starting(text: &str, prefix: &str) -> String {
    text.split_inclusive('\n')
        .filter(|l| !l.starts_with(prefix))
        .collect()
}

/// A harp: lowercase words joined by hyphens.
fn is_harp(text: &str) -> bool {
    let words = text.split('-').collect::<Vec<_>>();
    words.len() >= 2
        && words
            .iter()
            .all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase()))
}

/// `taskloom args` with `stdin` as its standard input and, if given,
/// `session` as CTXLOOM_SESSION_HARP (else the variable is not set).
fn run_with_stdin(env: &Env, args: &[&str], stdin: &str, session: Option<&str>) -> Ran {
    let mut command = Command::new(binary());
    command
        .args(args)
        .isolated(env.home.path())
        .working_in(env.project.path())
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(session) = session {
        command.env("CTXLOOM_SESSION_HARP", session);
    }
    let mut child = command.spawn().expect("taskloom runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .expect("stdin is written");
    Ran::of(child.wait_with_output().expect("its output"))
}

// ------------------------------------------------ 1. roots and discovery

#[test]
fn test_a_plan_is_any_plan_md_under_the_default_roots_the_work_tree_and_taskloom_plans() {
    let env = world();
    write_plan(
        &env,
        "docs/design/in-tree.plan.md",
        &plan_text("quiet-amber-falcon", "", "# A\n"),
    );
    write_plan(
        &env,
        ".taskloom/plans/in-plans.plan.md",
        &plan_text("brisk-cedar-otter", "", "# B\n"),
    );
    write_plan(
        &env,
        "docs/readme.md",
        &plan_text("calm-river-stone", "", "not a plan file\n"),
    );
    write_plan(
        &env,
        "docs/notes.plan.txt",
        &plan_text("deep-ember-lark", "", "not a plan file\n"),
    );
    let elsewhere = Temp::new();
    disk::write(
        elsewhere.path().join("outside.plan.md"),
        plan_text("hazy-pine-wren", "", "# Z\n"),
    )
    .unwrap();

    assert_eq!(listed_names(&env, &[]), ["in-plans", "in-tree"]);
}

#[test]
fn test_plan_roots_is_a_yaml_list_in_the_work_roots_config_file() {
    let env = world();
    let elsewhere = Temp::new();
    disk::write(
        elsewhere.path().join("far.plan.md"),
        plan_text("hazy-pine-wren", "", "# Z\n"),
    )
    .unwrap();
    disk::create_dir_all(env.project.path().join(".taskloom")).unwrap();
    disk::write(
        env.project.path().join(".taskloom/config.yaml"),
        format!("plan_roots:\n  - \"{}\"\n", elsewhere.path().display()),
    )
    .unwrap();

    assert!(listed_names(&env, &[]).contains(&"far".to_string()));
}

#[test]
fn test_plan_roots_is_comma_separated_in_the_environment() {
    let env = world();
    let (one, two) = (Temp::new(), Temp::new());
    disk::write(
        one.path().join("one.plan.md"),
        plan_text("hazy-pine-wren", "", "# 1\n"),
    )
    .unwrap();
    disk::write(
        two.path().join("two.plan.md"),
        plan_text("lucky-dune-ibis", "", "# 2\n"),
    )
    .unwrap();
    let roots = format!("{},{}", one.path().display(), two.path().display());

    let ran = env.run_with(
        &["plans", "list", "--format", "json"],
        &[("TASKLOOM_CONFIG_PLAN_ROOTS", &roots)],
    );
    assert!(ran.ok, "{}", ran.stderr);
    let names = strings(&Value::Array(
        ran.json()
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].clone())
            .collect(),
    ));
    assert!(
        names.contains(&"one".to_string()) && names.contains(&"two".to_string()),
        "{names:?}"
    );
}

#[test]
fn test_plan_roots_is_comma_separated_in_config_set() {
    let env = world();
    let (one, two) = (Temp::new(), Temp::new());
    disk::write(
        one.path().join("one.plan.md"),
        plan_text("hazy-pine-wren", "", "# 1\n"),
    )
    .unwrap();
    disk::write(
        two.path().join("two.plan.md"),
        plan_text("lucky-dune-ibis", "", "# 2\n"),
    )
    .unwrap();
    let set = format!(
        "plan_roots={},{}",
        one.path().display(),
        two.path().display()
    );

    let names = listed_names(&env, &["--config-set", &set]);
    assert!(
        names.contains(&"one".to_string()) && names.contains(&"two".to_string()),
        "{names:?}"
    );
}

#[test]
fn test_a_blank_plan_root_is_refused_with_exit_2_from_every_source() {
    let env = world();
    let dir = Temp::new();
    let good = dir.path().display().to_string();

    // Controls: the same sources with no blank item are accepted, so the
    // refusals below are for the blank item alone.
    let control = format!("plan_roots={good}");
    exits(
        &env,
        &[
            "plans",
            "list",
            "--config-set",
            &control,
            "--format",
            "json",
        ],
        0,
    );
    let ran = env.run_with(
        &["plans", "list", "--format", "json"],
        &[("TASKLOOM_CONFIG_PLAN_ROOTS", &good)],
    );
    assert!(ran.ok, "{}", ran.stderr);

    for blank in [
        format!("{good},,{good}"),
        format!("{good},"),
        format!(",{good}"),
    ] {
        let set = format!("plan_roots={blank}");
        let ran = exits(&env, &["plans", "list", "--config-set", &set], 2);
        assert_eq!(ran.stdout, "", "{blank:?}");
        let ran = env.run_with(
            &["plans", "list"],
            &[("TASKLOOM_CONFIG_PLAN_ROOTS", &blank)],
        );
        assert_eq!(ran.code, 2, "env {blank:?}: {}", ran.stderr);
    }

    disk::create_dir_all(env.project.path().join(".taskloom")).unwrap();
    disk::write(
        env.project.path().join(".taskloom/config.yaml"),
        format!("plan_roots:\n  - \"{good}\"\n  - \"\"\n"),
    )
    .unwrap();
    exits(&env, &["plans", "list"], 2);
}

// ------------------------------------------------------ 2. the frontmatter

#[test]
fn test_the_frontmatter_holds_all_the_metadata_and_show_and_list_read_it_from_the_file() {
    let env = world();
    let task = add_task(&env, "Govern me");
    let file = write_plan(
        &env,
        "docs/full.plan.md",
        &plan_text(
            "quiet-amber-falcon",
            &format!(
                "title: The full plan\nstatus: active\ntags:\n  - area:cli\n  - priority=3\ngoverns:\n  - {task}\nsupersedes: []\nsessions:\n  - older-session-harp\n"
            ),
            "# The full plan\n",
        ),
    );
    for plan in [
        ok(&env, &["plans", "show", "quiet-amber-falcon"]),
        ok(&env, &["plans", "show", &s(&file)]),
        ok(&env, &["plans", "list"])[0].clone(),
    ] {
        assert_eq!(plan["id"], "quiet-amber-falcon", "{plan}");
        assert_eq!(plan["title"], "The full plan");
        assert_eq!(plan["status"], "active");
        assert_eq!(plan["tags"], json!(["area:cli", "priority=3"]));
        assert_eq!(plan["governs"], json!([task]));
        assert_eq!(plan["supersedes"], json!([]));
        assert_eq!(plan["sessions"], json!(["older-session-harp"]));
    }
}

#[test]
fn test_superseded_is_derived_from_another_plans_supersedes_and_never_written() {
    let env = world();
    let old = write_plan(
        &env,
        "docs/old.plan.md",
        &plan_text(
            "quiet-amber-falcon",
            "title: Old\nstatus: active\n",
            "# Old\n",
        ),
    );
    let new = write_plan(
        &env,
        "docs/new.plan.md",
        &plan_text(
            "brisk-cedar-otter",
            "title: New\nstatus: active\nsupersedes:\n  - quiet-amber-falcon\n",
            "# New\n",
        ),
    );
    let (old_before, new_before) = (read(&old), read(&new));

    let shown = ok(&env, &["plans", "show", "quiet-amber-falcon"]);
    assert_eq!(shown["status"], "superseded");
    let listed = ok(&env, &["plans", "list"]);
    let status_of = |name: &str| {
        listed
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("{name} not listed: {listed}"))["status"]
            .clone()
    };
    assert_eq!(status_of("old"), "superseded");
    assert_eq!(status_of("new"), "active");
    assert_eq!(listed_names(&env, &["--status", "superseded"]), ["old"]);
    assert_eq!(listed_names(&env, &["--status", "active"]), ["new"]);
    // Reading wrote nothing, and nothing was written to the superseded
    // plan's file to say so.
    assert_eq!((read(&old), read(&new)), (old_before, new_before));
}

#[test]
fn test_a_plan_cannot_be_set_to_superseded_by_hand() {
    let env = world();
    let file = write_plan(
        &env,
        "docs/p.plan.md",
        &plan_text("quiet-amber-falcon", "title: P\nstatus: draft\n", "# P\n"),
    );
    // Control: a status that may be set is.
    ok(
        &env,
        &["plans", "set", "quiet-amber-falcon", "--status", "done"],
    );
    let before = read(&file);
    assert!(before.contains("status: done"), "{before}");

    exits(
        &env,
        &[
            "plans",
            "set",
            "quiet-amber-falcon",
            "--status",
            "superseded",
        ],
        2,
    );
    assert_eq!(read(&file), before);
}

// ------------------------------------- 3. fences, ids and the body's bytes

#[test]
fn test_a_file_with_no_frontmatter_gets_a_fence_block_on_its_first_write_and_the_body_is_untouched()
{
    let env = world();
    let body = "# A hand-written plan\n\nSome text.\n\n---\nnot a fence, a rule\n";
    let file = write_plan(&env, "docs/bare.plan.md", body);

    // Reading writes nothing.
    ok(&env, &["plans", "list"]);
    assert_eq!(read(&file), body);

    ok(&env, &["plans", "set", &s(&file), "--title", "Bare"]);
    let written = read(&file);
    let inside = written
        .strip_prefix("---\n")
        .unwrap_or_else(|| panic!("no opening fence: {written}"));
    let (frontmatter, rest) = inside
        .split_once("\n---\n")
        .unwrap_or_else(|| panic!("no closing fence: {written}"));
    assert_eq!(
        rest, body,
        "the body, byte for byte, after the closing fence line"
    );
    assert!(
        frontmatter.lines().any(|l| l == "title: Bare"),
        "{frontmatter}"
    );
    let id = frontmatter
        .lines()
        .find_map(|l| l.strip_prefix("id: "))
        .unwrap_or_else(|| panic!("no id minted: {frontmatter}"));
    assert!(is_harp(id), "{id:?} is a harp");
    assert_eq!(ok(&env, &["plans", "show", id])["title"], "Bare");
}

#[test]
fn test_a_hand_written_plan_without_an_id_gets_one_minted_and_keeps_its_body() {
    let env = world();
    let body = "# Plan\n\nBody text, with a trailing blank line.\n\n";
    let original = format!("---\ntitle: Hand written\nstatus: draft\n---\n{body}");
    let file = write_plan(&env, "docs/hand.plan.md", &original);

    ok(&env, &["plans", "set", &s(&file), "--status", "active"]);
    let written = read(&file);
    let minted = added_lines(
        &original.replace("status: draft", "status: active"),
        &written,
    );
    assert_eq!(minted.len(), 1, "only the id is added: {minted:?}");
    let id = minted[0]
        .strip_prefix("id: ")
        .unwrap_or_else(|| panic!("{minted:?}"));
    assert!(is_harp(id), "{id:?} is a harp");
    assert!(written.ends_with(&format!("---\n{body}")), "{written}");

    // A second write keeps the id it minted.
    ok(&env, &["plans", "set", &s(&file), "--status", "done"]);
    assert_eq!(
        read(&file),
        written.replace("status: active", "status: done")
    );
}

// ------------------------------------ 4. an edit changes only its own item

const HAND: &str = "---\n\
# kept: a comment ahead of the id\n\
id: quiet-amber-falcon\n\
title: Original\n\
\n\
status: draft\n\
flavor: {a: 1, b: [x, y]}\n\
tags: [area:cli, priority=3]\n\
# sessions are stamped by the hook\n\
sessions:\n\
\x20 - older-session-harp\n\
---\n\
# Body\n\
\n\
Text, then a rule:\n\
\n\
---\n\
not a fence\n";

#[test]
fn test_set_changes_only_the_edited_scalar_and_keeps_comments_blank_lines_order_and_unknown_keys() {
    let env = world();
    let file = write_plan(&env, "docs/hand.plan.md", HAND);

    ok(
        &env,
        &["plans", "set", "quiet-amber-falcon", "--status", "active"],
    );
    let after = read(&file);
    assert_eq!(after, HAND.replacen("status: draft", "status: active", 1));

    ok(
        &env,
        &["plans", "set", "quiet-amber-falcon", "--title", "Renamed"],
    );
    assert_eq!(
        read(&file),
        after.replacen("title: Original", "title: Renamed", 1)
    );
}

#[test]
fn test_tag_add_and_remove_edit_a_flow_style_list_and_change_no_other_line() {
    let env = world();
    let file = write_plan(&env, "docs/hand.plan.md", HAND);

    ok(
        &env,
        &[
            "plans",
            "tag",
            "add",
            "quiet-amber-falcon",
            "human:decision",
        ],
    );
    let added = read(&file);
    assert_eq!(
        without_lines_starting(&added, "tags:"),
        without_lines_starting(HAND, "tags:"),
        "only the tags line changed"
    );
    assert_eq!(
        ok(&env, &["plans", "show", "quiet-amber-falcon"])["tags"],
        json!(["area:cli", "priority=3", "human:decision"])
    );

    ok(
        &env,
        &["plans", "tag", "remove", "quiet-amber-falcon", "area:cli"],
    );
    let removed = read(&file);
    assert_eq!(
        without_lines_starting(&removed, "tags:"),
        without_lines_starting(HAND, "tags:")
    );
    assert_eq!(
        ok(&env, &["plans", "show", "quiet-amber-falcon"])["tags"],
        json!(["priority=3", "human:decision"])
    );
}

#[test]
fn test_link_and_unlink_add_and_remove_one_list_item_and_change_no_other_line() {
    let env = world();
    let (first, second) = (add_task(&env, "First"), add_task(&env, "Second"));
    let original = HAND.replacen(
        "status: draft\n",
        &format!("status: draft\ngoverns:\n  - {first}\n"),
        1,
    );
    let file = write_plan(&env, "docs/hand.plan.md", &original);

    ok(
        &env,
        &["plans", "link", "quiet-amber-falcon", "--governs", &second],
    );
    let linked = read(&file);
    let added = added_lines(&original, &linked);
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!(added[0].trim(), format!("- {second}"));

    ok(
        &env,
        &[
            "plans",
            "unlink",
            "quiet-amber-falcon",
            "--governs",
            &second,
        ],
    );
    assert_eq!(read(&file), original);
}

// --------------------------------------------- 5. the verbs, set and show

#[test]
fn test_plans_has_exactly_the_verbs_add_list_show_set_tag_link_unlink_check_watch() {
    let env = world();
    let verbs = |args: &[&str]| -> Vec<String> {
        let ran = exits(&env, &[args, &["--help"]].concat(), 0);
        let mut found = ran
            .stdout
            .lines()
            .skip_while(|l| *l != "Commands:")
            .skip(1)
            .take_while(|l| !l.trim().is_empty())
            .map(|l| l.split_whitespace().next().unwrap().to_string())
            .collect::<Vec<_>>();
        found.sort();
        found
    };
    assert_eq!(
        verbs(&["plans"]),
        ["add", "check", "link", "list", "set", "show", "tag", "unlink", "watch"]
    );
    assert_eq!(verbs(&["plans", "tag"]), ["add", "remove"]);
}

#[test]
fn test_plans_add_writes_a_plan_file_under_the_default_plans_root_with_a_minted_id() {
    let env = world();
    let added = ok(&env, &["plans", "add", "--title", "Fresh plan"]);
    let id = added["id"].as_str().expect("an id");
    assert!(is_harp(id), "{id:?}");
    let path = PathBuf::from(added["path"].as_str().expect("a path"));
    assert!(
        path.starts_with(env.project.path().join(".taskloom").join("plans")),
        "{path:?}"
    );
    assert!(path.to_string_lossy().ends_with(".plan.md"), "{path:?}");
    let written = read(&path);
    assert!(
        written.lines().any(|l| l == format!("id: {id}")),
        "{written}"
    );
    assert!(
        written.lines().any(|l| l == "title: Fresh plan"),
        "{written}"
    );
    assert_eq!(added["status"], "draft");
    assert_eq!(ok(&env, &["plans", "show", id])["title"], "Fresh plan");
    assert_eq!(listed_names(&env, &[]).len(), 1);
}

#[test]
fn test_show_takes_an_id_or_a_path_and_a_plan_not_found_is_exit_1() {
    let env = world();
    let file = write_plan(
        &env,
        "docs/shown.plan.md",
        &plan_text(
            "quiet-amber-falcon",
            "title: Shown\nstatus: draft\n",
            "# Shown\n",
        ),
    );
    let by_id = ok(&env, &["plans", "show", "quiet-amber-falcon"]);
    let by_path = ok(&env, &["plans", "show", &s(&file)]);
    assert_eq!(by_id["id"], "quiet-amber-falcon");
    assert_eq!(by_id["path"], s(&file).as_str());
    assert_eq!(by_id["id"], by_path["id"]);
    assert_eq!(by_id["title"], by_path["title"]);

    // Its text is the file, raw.
    let raw = env.run(&["plans", "show", "quiet-amber-falcon", "--format", "text"]);
    assert!(raw.ok, "{}", raw.stderr);
    assert_eq!(raw.stdout, read(&file));

    exits(&env, &["plans", "show", "no-such-plan"], 1);
    exits(
        &env,
        &[
            "plans",
            "show",
            &s(&under(env.project.path(), "docs/missing.plan.md")),
        ],
        1,
    );
}

#[test]
fn test_set_changes_status_and_title_and_an_unknown_status_is_exit_2_leaving_the_file() {
    // PROPOSAL, pending owner review: `plans set <plan> --status S --title T`,
    // flags as `tasks set` has them; <plan> an id or a path as `show` takes.
    let env = world();
    let file = write_plan(
        &env,
        "docs/p.plan.md",
        &plan_text("quiet-amber-falcon", "title: P\nstatus: draft\n", "# P\n"),
    );
    for status in ["active", "done", "abandoned", "draft"] {
        let set = ok(
            &env,
            &["plans", "set", "quiet-amber-falcon", "--status", status],
        );
        assert_eq!(set["status"], status);
        assert!(read(&file)
            .lines()
            .any(|l| l == format!("status: {status}")));
    }
    let set = ok(
        &env,
        &[
            "plans",
            "set",
            &s(&file),
            "--title",
            "Retitled",
            "--status",
            "active",
        ],
    );
    assert_eq!(
        (set["title"].clone(), set["status"].clone()),
        (json!("Retitled"), json!("active"))
    );

    let before = read(&file);
    let refused = exits(
        &env,
        &["plans", "set", "quiet-amber-falcon", "--status", "nonsense"],
        2,
    );
    assert!(refused.stderr.contains("nonsense"), "{}", refused.stderr);
    assert_eq!(read(&file), before);
}

// ----------------------------------------------------------------- 6. tags

#[test]
fn test_a_tag_no_permit_admits_is_exit_2_and_the_file_is_untouched() {
    let env = seeded_world();
    let file = write_plan(
        &env,
        "docs/p.plan.md",
        &plan_text("quiet-amber-falcon", "title: P\nstatus: draft\n", "# P\n"),
    );
    // Control: a seeded permit admits this one.
    ok(
        &env,
        &[
            "plans",
            "tag",
            "add",
            "quiet-amber-falcon",
            "human:decision",
        ],
    );
    let before = read(&file);
    assert!(before.contains("human:decision"), "{before}");

    let refused = exits(
        &env,
        &["plans", "tag", "add", "quiet-amber-falcon", "area:cli"],
        2,
    );
    assert!(refused.stderr.contains("area:cli"), "{}", refused.stderr);
    assert_eq!(read(&file), before);
}

#[test]
fn test_the_retired_ctxloom_plan_tag_is_exit_2_on_a_plan_too() {
    let env = world();
    let file = write_plan(
        &env,
        "docs/p.plan.md",
        &plan_text("quiet-amber-falcon", "title: P\nstatus: draft\n", "# P\n"),
    );
    ok(
        &env,
        &["plans", "tag", "add", "quiet-amber-falcon", "area:cli"],
    );
    let before = read(&file);
    let refused = exits(
        &env,
        &[
            "plans",
            "tag",
            "add",
            "quiet-amber-falcon",
            r#"ctxloom:plan="x""#,
        ],
        2,
    );
    assert!(
        refused.stderr.contains("ctxloom:plan"),
        "{}",
        refused.stderr
    );
    assert_eq!(read(&file), before);
}

#[test]
fn test_tags_round_trip_as_plain_yaml_list_items_and_quoted_where_yaml_needs_it() {
    let env = world();
    let file = write_plan(
        &env,
        "docs/p.plan.md",
        &plan_text("quiet-amber-falcon", "title: P\nstatus: draft\n", "# P\n"),
    );
    // Tagma's own spelling: a component outside its bare alphabet is
    // quoted, so `"a/b"` and `key="a b"` are the plan's `a/b` and `key=a b`.
    let plain = ["area:cli", "priority=3", "human:decision", r#"key="a b""#];
    // These need YAML quoting: one starts with a double quote, one holds `: `.
    let quoted = [r#""a/b""#, r##""#x""##, r#"key="a: b""#];
    let all = plain
        .iter()
        .chain(quoted.iter())
        .copied()
        .collect::<Vec<_>>();
    for tag in &all {
        ok(&env, &["plans", "tag", "add", "quiet-amber-falcon", tag]);
    }
    let shown = ok(&env, &["plans", "show", "quiet-amber-falcon"]);
    assert_eq!(strings(&shown["tags"]), all, "read back identically");

    let written = read(&file);
    let items = written
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- "))
        .collect::<Vec<_>>();
    for tag in plain {
        assert!(items.contains(&tag), "{tag} is a plain item: {written}");
    }
    for tag in quoted {
        assert!(
            !items.contains(&tag),
            "{tag} written bare would load as a different string: {written}"
        );
    }

    // And the other way: filtering by one finds the plan.
    for tag in &all {
        let names = listed_names(&env, &["--tag-query", tag]);
        assert_eq!(names, ["p"], "{tag}");
    }

    for tag in &all {
        ok(&env, &["plans", "tag", "remove", "quiet-amber-falcon", tag]);
    }
    assert_eq!(
        ok(&env, &["plans", "show", "quiet-amber-falcon"])["tags"],
        json!([])
    );
}

// ------------------------------------------------------------- 7. listing

#[test]
fn test_list_objects_carry_the_extensions_fields_and_the_new_metadata() {
    let env = world();
    let task = add_task(&env, "Governed");
    let file = write_plan(
        &env,
        "docs/listed.plan.md",
        &plan_text(
            "quiet-amber-falcon",
            &format!("title: Listed\nstatus: active\ntags: [area:cli]\ngoverns: [{task}]\nsessions: [only-session-harp]\n"),
            "# Listed\n",
        ),
    );
    let listed = ok(&env, &["plans", "list"]);
    let plan = &listed.as_array().unwrap()[0];
    for key in [
        "id",
        "path",
        "name",
        "title",
        "status",
        "tags",
        "governs",
        "supersedes",
        "session",
        "sessions",
    ] {
        assert!(plan.get(key).is_some(), "no {key}: {plan}");
    }
    assert_eq!(plan["path"], s(&file).as_str());
    // PROPOSAL, pending owner review: `name` is the file's name without
    // `.plan.md` (the old id-named files made it the id).
    assert_eq!(plan["name"], "listed");
    assert_eq!(plan["title"], "Listed");
    assert_eq!(plan["session"], "only-session-harp");
    assert_eq!(plan["sessions"], json!(["only-session-harp"]));
}

#[test]
fn test_list_filters_by_governs_tag_query_and_status() {
    let env = world();
    let (one, two) = (add_task(&env, "One"), add_task(&env, "Two"));
    write_plan(
        &env,
        "docs/a.plan.md",
        &plan_text(
            "quiet-amber-falcon",
            &format!("status: active\ntags: [area:cli]\ngoverns: [{one}]\n"),
            "# a\n",
        ),
    );
    write_plan(
        &env,
        "docs/b.plan.md",
        &plan_text(
            "brisk-cedar-otter",
            &format!("status: draft\ntags: [human:decision]\ngoverns: [{one}, {two}]\n"),
            "# b\n",
        ),
    );
    write_plan(
        &env,
        "docs/c.plan.md",
        &plan_text(
            "calm-river-stone",
            "status: done\ntags: [area:cli]\n",
            "# c\n",
        ),
    );
    assert_eq!(listed_names(&env, &["--governs", &one]), ["a", "b"]);
    assert_eq!(listed_names(&env, &["--governs", &two]), ["b"]);
    assert_eq!(listed_names(&env, &["--tag-query", "area:cli"]), ["a", "c"]);
    assert_eq!(
        listed_names(&env, &["--tag-query", "area:cli/human:decision/or"]),
        ["a", "b", "c"]
    );
    assert_eq!(listed_names(&env, &["--status", "active"]), ["a"]);
    assert_eq!(
        listed_names(&env, &["--status", "active,draft"]),
        ["a", "b"]
    );
    assert_eq!(
        listed_names(&env, &["--governs", &one, "--tag-query", "area:cli"]),
        ["a"]
    );
}

#[test]
fn test_tasks_show_no_longer_prints_a_plans_line() {
    let env = world();
    let task = add_task(&env, "Governed task");
    // Made through `plans add --governs`, as a plan governing the task.
    let added = ok(
        &env,
        &["plans", "add", "--title", "Governing", "--governs", &task],
    );
    assert_eq!(
        ok(&env, &["plans", "list", "--governs", &task])[0]["id"],
        added["id"],
        "the plan governs the task"
    );
    let text = env.run(&["tasks", "show", &task, "--format", "text"]);
    assert!(text.ok, "{}", text.stderr);
    assert!(!text.stdout.contains("plans:"), "{}", text.stdout);
    let json = ok(&env, &["tasks", "show", &task]);
    assert!(!json.to_string().contains("plans"), "{json}");
}

// ----------------------------------------------------- 8. link and unlink

#[test]
fn test_link_governs_edits_the_frontmatter_and_a_task_that_does_not_exist_is_exit_1() {
    let env = world();
    let task = add_task(&env, "Real");
    let file = write_plan(
        &env,
        "docs/p.plan.md",
        &plan_text("quiet-amber-falcon", "title: P\nstatus: draft\n", "# P\n"),
    );
    ok(
        &env,
        &["plans", "link", "quiet-amber-falcon", "--governs", &task],
    );
    assert_eq!(
        ok(&env, &["plans", "show", "quiet-amber-falcon"])["governs"],
        json!([task])
    );
    assert!(
        read(&file).contains(&format!("- {task}")),
        "{}",
        read(&file)
    );
    assert_eq!(listed_names(&env, &["--governs", &task]), ["p"]);

    let before = read(&file);
    exits(
        &env,
        &[
            "plans",
            "link",
            "quiet-amber-falcon",
            "--governs",
            "ghost-of-a-task",
        ],
        1,
    );
    assert_eq!(read(&file), before);

    ok(
        &env,
        &["plans", "unlink", "quiet-amber-falcon", "--governs", &task],
    );
    assert_eq!(
        ok(&env, &["plans", "show", "quiet-amber-falcon"])["governs"],
        json!([])
    );
    assert_eq!(
        listed_names(&env, &["--governs", &task]),
        Vec::<String>::new()
    );
}

#[test]
fn test_link_supersedes_edits_the_frontmatter_and_derives_the_superseded_status() {
    let env = world();
    let old = write_plan(
        &env,
        "docs/old.plan.md",
        &plan_text("quiet-amber-falcon", "status: active\n", "# o\n"),
    );
    let new = write_plan(
        &env,
        "docs/new.plan.md",
        &plan_text("brisk-cedar-otter", "status: active\n", "# n\n"),
    );
    let old_before = read(&old);

    ok(
        &env,
        &[
            "plans",
            "link",
            "brisk-cedar-otter",
            "--supersedes",
            "quiet-amber-falcon",
        ],
    );
    assert_eq!(
        ok(&env, &["plans", "show", "brisk-cedar-otter"])["supersedes"],
        json!(["quiet-amber-falcon"])
    );
    assert!(
        read(&new).contains("- quiet-amber-falcon"),
        "{}",
        read(&new)
    );
    assert_eq!(
        read(&old),
        old_before,
        "the superseded plan's file is not written"
    );
    assert_eq!(
        ok(&env, &["plans", "show", "quiet-amber-falcon"])["status"],
        "superseded"
    );

    ok(
        &env,
        &[
            "plans",
            "unlink",
            "brisk-cedar-otter",
            "--supersedes",
            "quiet-amber-falcon",
        ],
    );
    assert_eq!(
        ok(&env, &["plans", "show", "quiet-amber-falcon"])["status"],
        "active"
    );
}

#[test]
fn test_a_self_supersede_or_a_cycle_is_exit_2_and_leaves_the_files() {
    let env = world();
    let a = write_plan(
        &env,
        "docs/a.plan.md",
        &plan_text("quiet-amber-falcon", "status: active\n", "# a\n"),
    );
    let b = write_plan(
        &env,
        "docs/b.plan.md",
        &plan_text("brisk-cedar-otter", "status: active\n", "# b\n"),
    );
    // Control: a plain supersede is accepted.
    ok(
        &env,
        &[
            "plans",
            "link",
            "brisk-cedar-otter",
            "--supersedes",
            "quiet-amber-falcon",
        ],
    );
    let (a_before, b_before) = (read(&a), read(&b));

    exits(
        &env,
        &[
            "plans",
            "link",
            "quiet-amber-falcon",
            "--supersedes",
            "quiet-amber-falcon",
        ],
        2,
    );
    exits(
        &env,
        &[
            "plans",
            "link",
            "quiet-amber-falcon",
            "--supersedes",
            "brisk-cedar-otter",
        ],
        2,
    );
    assert_eq!((read(&a), read(&b)), (a_before, b_before));
}

// -------------------------------------------------------------- 9. check

/// Each of `faulty` named in `plans check`'s findings, in JSON and in
/// text, which exits 1; `clean` named in neither.
fn check_names(env: &Env, faulty: &[&Path], clean: &[&Path]) {
    let as_json = exits(env, &["plans", "check", "--format", "json"], 1);
    let as_text = exits(env, &["plans", "check", "--format", "text"], 1);
    for file in faulty {
        for out in [&as_json.stdout, &as_text.stdout] {
            assert!(out.contains(&s(file)), "{file:?} is not named: {out}");
        }
    }
    for file in clean {
        for out in [&as_json.stdout, &as_text.stdout] {
            assert!(
                !out.contains(&s(file)),
                "{file:?} is clean but named: {out}"
            );
        }
    }
}

#[test]
fn test_check_exits_0_with_no_findings_when_every_plan_is_sound() {
    let env = seeded_world();
    let task = add_task(&env, "Real");
    write_plan(
        &env,
        "docs/a.plan.md",
        &plan_text(
            "quiet-amber-falcon",
            &format!("tags: [human:decision]\ngoverns: [{task}]\n"),
            "# a\n",
        ),
    );
    write_plan(
        &env,
        "docs/b.plan.md",
        &plan_text(
            "brisk-cedar-otter",
            "supersedes: [quiet-amber-falcon]\n",
            "# b\n",
        ),
    );
    let ran = exits(&env, &["plans", "check", "--format", "json"], 0);
    assert_eq!(ran.json()["findings"], json!([]));
}

#[test]
fn test_check_finds_frontmatter_that_does_not_parse() {
    let env = seeded_world();
    let good = write_plan(
        &env,
        "docs/good.plan.md",
        &plan_text("quiet-amber-falcon", "", "# g\n"),
    );
    let bad = write_plan(
        &env,
        "docs/bad.plan.md",
        "---\nid: brisk-cedar-otter\ntitle: [unclosed\n---\n# b\n",
    );
    check_names(&env, &[&bad], &[&good]);
}

#[test]
fn test_check_finds_a_tag_the_stores_rules_do_not_admit() {
    let env = seeded_world();
    let good = write_plan(
        &env,
        "docs/good.plan.md",
        &plan_text("quiet-amber-falcon", "tags: [human:decision]\n", "# g\n"),
    );
    let bad = write_plan(
        &env,
        "docs/bad.plan.md",
        &plan_text("brisk-cedar-otter", "tags: [area:cli]\n", "# b\n"),
    );
    check_names(&env, &[&bad], &[&good]);
}

#[test]
fn test_check_finds_governs_naming_a_task_that_does_not_exist() {
    let env = seeded_world();
    let task = add_task(&env, "Real");
    let good = write_plan(
        &env,
        "docs/good.plan.md",
        &plan_text(
            "quiet-amber-falcon",
            &format!("governs: [{task}]\n"),
            "# g\n",
        ),
    );
    let bad = write_plan(
        &env,
        "docs/bad.plan.md",
        &plan_text("brisk-cedar-otter", "governs: [ghost-of-a-task]\n", "# b\n"),
    );
    check_names(&env, &[&bad], &[&good]);
}

#[test]
fn test_check_finds_supersedes_naming_a_plan_that_does_not_exist() {
    let env = seeded_world();
    let good = write_plan(
        &env,
        "docs/good.plan.md",
        &plan_text("quiet-amber-falcon", "", "# g\n"),
    );
    let bad = write_plan(
        &env,
        "docs/bad.plan.md",
        &plan_text(
            "brisk-cedar-otter",
            "supersedes: [ghost-of-a-plan]\n",
            "# b\n",
        ),
    );
    check_names(&env, &[&bad], &[&good]);
}

#[test]
fn test_check_finds_a_duplicate_id_across_files_naming_both() {
    let env = seeded_world();
    let good = write_plan(
        &env,
        "docs/good.plan.md",
        &plan_text("quiet-amber-falcon", "", "# g\n"),
    );
    let one = write_plan(
        &env,
        "docs/copy-one.plan.md",
        &plan_text("brisk-cedar-otter", "", "# 1\n"),
    );
    let two = write_plan(
        &env,
        "docs/copy-two.plan.md",
        &plan_text("brisk-cedar-otter", "", "# 2\n"),
    );
    check_names(&env, &[&one, &two], &[&good]);
}

#[test]
fn test_check_finds_a_file_with_no_id() {
    let env = seeded_world();
    let good = write_plan(
        &env,
        "docs/good.plan.md",
        &plan_text("quiet-amber-falcon", "", "# g\n"),
    );
    let bad = write_plan(&env, "docs/noid.plan.md", "---\ntitle: No id\n---\n# n\n");
    let bare = write_plan(&env, "docs/bare.plan.md", "# no frontmatter at all\n");
    check_names(&env, &[&bad, &bare], &[&good]);
    // Checking is a read: it minted nothing.
    assert_eq!(read(&bad), "---\ntitle: No id\n---\n# n\n");
}

// -------------------------------------------------------------- 10. watch

/// A running `taskloom plans watch`, killed if a test ends with it still
/// running.
struct Watching {
    child: Option<Child>,
    lines: Receiver<String>,
}

impl Watching {
    fn start(env: &Env) -> Watching {
        let mut child = Command::new(binary())
            .args(["plans", "watch"])
            .isolated(env.home.path())
            .working_in(env.project.path())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("CTXLOOM_SESSION_HARP", SESSION)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("taskloom plans watch starts");
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Watching {
            child: Some(child),
            lines,
        }
    }

    fn line(&self) -> Value {
        let line = self
            .lines
            .recv_timeout(PATIENCE)
            .unwrap_or_else(|e| panic!("no line from plans watch: {e}"));
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line:?}"))
    }

    /// The lines up to and including the first for `path`.
    fn until(&self, path: &Path) -> Vec<Value> {
        let mut seen = Vec::new();
        loop {
            let line = self.line();
            let found = line["path"] == s(path).as_str();
            seen.push(line);
            if found {
                return seen;
            }
            assert!(seen.len() < 50, "no line for {path:?}: {seen:?}");
        }
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn test_watch_primes_then_emits_a_line_for_each_plan_file_created_modified_and_deleted() {
    // PROPOSAL, pending owner review: a line is
    // {"event":"changed","kind":"plans","path":"<absolute path of the plan file>"};
    // the priming line is the same without "path" and is written once the
    // watch is established, so a change after it is never missed. The line
    // does not say which of create, modify or delete it was.
    let env = world();
    let modified = write_plan(
        &env,
        "docs/modify.plan.md",
        &plan_text("quiet-amber-falcon", "", "# m\n"),
    );
    let deleted = write_plan(
        &env,
        "docs/delete.plan.md",
        &plan_text("brisk-cedar-otter", "", "# d\n"),
    );
    let watch = Watching::start(&env);

    let primed = watch.line();
    assert_eq!(primed["event"], "changed", "{primed}");
    assert_eq!(primed["kind"], "plans", "{primed}");

    // Created: and a file that is no plan before it says nothing.
    write_plan(&env, "docs/notes.md", "not a plan\n");
    let created = write_plan(
        &env,
        "docs/create.plan.md",
        &plan_text("calm-river-stone", "", "# c\n"),
    );
    let seen = watch.until(&created);
    assert!(
        seen.iter().all(|l| l["path"] == s(&created).as_str()),
        "a non-plan file made a line: {seen:?}"
    );
    for line in &seen {
        assert_eq!(
            (&line["event"], &line["kind"]),
            (&json!("changed"), &json!("plans"))
        );
    }

    disk::write(
        &modified,
        plan_text("quiet-amber-falcon", "title: edited\n", "# m\n"),
    )
    .unwrap();
    watch.until(&modified);

    disk::remove_file(&deleted).unwrap();
    watch.until(&deleted);
}

#[test]
fn test_watch_ends_with_exit_0_at_sigterm_and_writes_nothing_after() {
    if os::host().flavor() != os::Flavor::Unix {
        return;
    }
    let env = world();
    let mut watch = Watching::start(&env);
    let primed = watch.line();
    assert_eq!(primed["kind"], "plans");

    let mut child = watch.child.take().unwrap();
    let sent = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(sent.success());
    let status = child.wait().expect("watch ends");
    assert_eq!(status.code(), Some(0), "{status:?}");
    assert!(
        matches!(
            watch.lines.recv_timeout(PATIENCE),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ),
        "it wrote more after the priming line"
    );
}

// ------------------------------------------------------ 11. stamp-plan

/// A hook payload naming `file` as the edited file.
///
/// ASSUMPTION, pending owner review: Claude's post-tool-use shape, as the
/// Go hook's codec reads it: `tool_input.file_path` (`notebook_path` for a
/// notebook edit), and no `--engine` flag, since only one engine's shape is
/// spoken.
fn payload(file: &Path) -> String {
    json!({
        "session_id": "backend-session-id",
        "hook_event_name": "PostToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": s(file)},
    })
    .to_string()
}

fn stamp(env: &Env, file: &Path, session: Option<&str>) -> Ran {
    run_with_stdin(
        env,
        &["manage", "hook", "stamp-plan"],
        &payload(file),
        session,
    )
}

#[test]
fn test_stamp_plan_appends_the_session_to_sessions_once_and_is_idempotent() {
    let env = world();
    let file = write_plan(
        &env,
        "docs/p.plan.md",
        &plan_text(
            "quiet-amber-falcon",
            "title: P\nsessions:\n  - older-session-harp\n",
            "# P\n",
        ),
    );
    let ran = stamp(&env, &file, Some("this-session-harp"));
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let once = read(&file);
    assert_eq!(
        ok(&env, &["plans", "show", "quiet-amber-falcon"])["sessions"],
        json!(["older-session-harp", "this-session-harp"])
    );
    let added = added_lines(
        &plan_text(
            "quiet-amber-falcon",
            "title: P\nsessions:\n  - older-session-harp\n",
            "# P\n",
        ),
        &once,
    );
    assert_eq!(added.len(), 1, "only the session is added: {added:?}");

    assert_eq!(stamp(&env, &file, Some("this-session-harp")).code, 0);
    assert_eq!(read(&file), once, "stamping again changes nothing");
}

#[test]
fn test_stamp_plan_through_the_same_editor_mints_an_id_creates_the_key_and_keeps_the_body() {
    let env = world();
    let body = "# Plan\n\nBody.\n";
    let file = write_plan(&env, "docs/p.plan.md", body);
    let ran = stamp(&env, &file, Some("this-session-harp"));
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let written = read(&file);
    let inside = written
        .strip_prefix("---\n")
        .unwrap_or_else(|| panic!("no fence: {written}"));
    let (frontmatter, rest) = inside
        .split_once("\n---\n")
        .unwrap_or_else(|| panic!("no close: {written}"));
    assert_eq!(rest, body);
    let id = frontmatter
        .lines()
        .find_map(|l| l.strip_prefix("id: "))
        .expect("an id");
    assert!(is_harp(id), "{id:?}");
    assert_eq!(
        ok(&env, &["plans", "show", id])["sessions"],
        json!(["this-session-harp"])
    );
}

#[test]
fn test_stamp_plan_is_a_no_op_without_the_session_variable() {
    let env = world();
    let control = write_plan(
        &env,
        "docs/control.plan.md",
        &plan_text("brisk-cedar-otter", "", "# c\n"),
    );
    assert_eq!(stamp(&env, &control, Some("this-session-harp")).code, 0);
    assert!(
        read(&control).contains("this-session-harp"),
        "the control is stamped"
    );

    let file = write_plan(
        &env,
        "docs/p.plan.md",
        &plan_text("quiet-amber-falcon", "title: P\n", "# P\n"),
    );
    let before = read(&file);
    let ran = stamp(&env, &file, None);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(read(&file), before);
}

#[test]
fn test_stamp_plan_is_a_no_op_for_a_file_that_is_not_a_plan() {
    let env = world();
    let control = write_plan(
        &env,
        "docs/control.plan.md",
        &plan_text("brisk-cedar-otter", "", "# c\n"),
    );
    assert_eq!(stamp(&env, &control, Some("this-session-harp")).code, 0);
    assert!(
        read(&control).contains("this-session-harp"),
        "the control is stamped"
    );

    let notes = write_plan(&env, "docs/notes.md", "---\ntitle: notes\n---\n# n\n");
    let before = read(&notes);
    let ran = stamp(&env, &notes, Some("this-session-harp"));
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(read(&notes), before);
}

// ----------------------------------------------------- 12. no store records

/// Whether any file under `dir` holds `needle`.
fn holds(dir: &Path, needle: &[u8]) -> bool {
    disk::read_dir(dir).unwrap().iter().any(|entry| {
        if disk::read_dir(entry).is_ok() {
            holds(entry, needle)
        } else {
            let bytes = disk::read(entry).unwrap_or_default();
            bytes.windows(needle.len()).any(|w| w == needle)
        }
    })
}

#[test]
fn test_plans_commands_create_no_plan_stream_in_the_store() {
    // The store is a SQLite file under the home; a plan aggregate's events
    // carry their type URLs (`ctxloom.plans.v1.PlanAdded`) as bytes in it.
    // Black-box this cannot show that no other plan table exists, only that
    // no plan event is recorded.
    let env = world();
    let task = add_task(&env, "Governed");
    let added = ok(
        &env,
        &["plans", "add", "--title", "Filed", "--governs", &task],
    );
    let id = added["id"].as_str().unwrap().to_string();
    assert!(
        !holds(env.home.path(), b"ctxloom.plans"),
        "`plans add` recorded a plan event in the store"
    );
    ok(&env, &["plans", "set", &id, "--status", "active"]);
    ok(&env, &["plans", "tag", "add", &id, "area:cli"]);
    ok(&env, &["plans", "link", &id, "--governs", &task]);
    ok(&env, &["plans", "unlink", &id, "--governs", &task]);
    ok(&env, &["plans", "list"]);
    ok(&env, &["plans", "show", &id]);
    exits(&env, &["plans", "check"], 0);

    assert!(
        !holds(env.home.path(), b"ctxloom.plans"),
        "a plan event is in the store"
    );
    assert!(
        !holds(env.home.path(), b"PlanAdded"),
        "a plan event is in the store"
    );
}

// ------------------------------------------- rulings of 2026-11-03

// owner ruling 2026-11-03: `plan_roots`, when set, REPLACES the default
// roots; unset means the defaults; a relative root resolves against the
// work tree root.
#[test]
fn test_plan_roots_when_set_replaces_the_defaults_and_unset_keeps_them() {
    let env = world();
    write_plan(
        &env,
        "docs/in-tree.plan.md",
        &plan_text("quiet-amber-falcon", "", "# t\n"),
    );
    write_plan(
        &env,
        ".taskloom/plans/in-plans.plan.md",
        &plan_text("brisk-cedar-otter", "", "# p\n"),
    );
    let x = Temp::new();
    disk::write(
        x.path().join("far.plan.md"),
        plan_text("hazy-pine-wren", "", "# f\n"),
    )
    .unwrap();

    assert_eq!(
        listed_names(&env, &[]),
        ["in-plans", "in-tree"],
        "unset: the defaults"
    );
    let set = format!("plan_roots={}", x.path().display());
    assert_eq!(
        listed_names(&env, &["--config-set", &set]),
        ["far"],
        "set: only X"
    );
}

#[test]
fn test_a_relative_plan_root_resolves_against_the_work_tree_root() {
    let env = world();
    write_plan(
        &env,
        "docs/in-docs.plan.md",
        &plan_text("quiet-amber-falcon", "", "# d\n"),
    );
    write_plan(
        &env,
        "other/in-other.plan.md",
        &plan_text("brisk-cedar-otter", "", "# o\n"),
    );
    assert_eq!(
        listed_names(&env, &["--config-set", "plan_roots=docs"]),
        ["in-docs"]
    );
}

// orchestrator decision, pending owner review: absent status is draft;
// absent title is the first markdown heading, else the file name; `session`
// is the first of `sessions`, empty (null or "") when there are none.
#[test]
fn test_absent_status_title_and_session_take_their_defaults() {
    let env = world();
    write_plan(
        &env,
        "docs/headed.plan.md",
        &plan_text(
            "quiet-amber-falcon",
            "",
            "Intro line\n\n# The Heading\n\ntext\n",
        ),
    );
    write_plan(
        &env,
        "docs/headless.plan.md",
        &plan_text(
            "brisk-cedar-otter",
            "sessions: [first-harp, second-harp]\n",
            "just text\n",
        ),
    );
    let listed = ok(&env, &["plans", "list"]);
    let find = |name: &str| {
        listed
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()
            .clone()
    };
    let headed = find("headed");
    assert_eq!(headed["status"], "draft");
    assert_eq!(headed["title"], "The Heading");
    assert!(
        headed["session"].is_null() || headed["session"] == "",
        "{headed}"
    );
    let headless = find("headless");
    assert_eq!(headless["status"], "draft");
    assert!(
        headless["title"].as_str().unwrap().contains("headless"),
        "{headless}"
    );
    assert_eq!(headless["session"], "first-harp");
}

// orchestrator decision, pending owner review: `superseded` is derived, so
// a hand-written one is a `check` finding.
#[test]
fn test_check_flags_a_hand_written_superseded_status() {
    let env = seeded_world();
    let good = write_plan(
        &env,
        "docs/good.plan.md",
        &plan_text("quiet-amber-falcon", "status: active\n", "# g\n"),
    );
    let bad = write_plan(
        &env,
        "docs/bad.plan.md",
        &plan_text("brisk-cedar-otter", "status: superseded\n", "# b\n"),
    );
    check_names(&env, &[&bad], &[&good]);
}

// orchestrator decision, pending owner review: a hook must not block an
// edit, so a payload it cannot read warns on stderr and exits 0.
#[test]
fn test_stamp_plan_warns_and_exits_0_on_a_malformed_or_empty_payload() {
    let env = world();
    let control = write_plan(
        &env,
        "docs/control.plan.md",
        &plan_text("brisk-cedar-otter", "", "# c\n"),
    );
    assert_eq!(stamp(&env, &control, Some("this-session-harp")).code, 0);
    assert!(
        read(&control).contains("this-session-harp"),
        "the control is stamped"
    );

    let file = write_plan(
        &env,
        "docs/p.plan.md",
        &plan_text("quiet-amber-falcon", "", "# p\n"),
    );
    let before = read(&file);
    for input in ["", "not json", "{}", r#"{"tool_input":{}}"#] {
        let ran = run_with_stdin(
            &env,
            &["manage", "hook", "stamp-plan"],
            input,
            Some("this-session-harp"),
        );
        assert_eq!(ran.code, 0, "{input:?}: {}", ran.stderr);
        assert!(!ran.stderr.trim().is_empty(), "{input:?} warns on stderr");
        assert_eq!(read(&file), before);
    }
}

// orchestrator decision, pending owner review: removing a tag the plan
// does not carry is a no-op, exit 0.
#[test]
fn test_tag_remove_of_a_tag_the_plan_lacks_is_a_no_op_exit_0() {
    let env = world();
    let file = write_plan(
        &env,
        "docs/p.plan.md",
        &plan_text("quiet-amber-falcon", "tags: [area:cli]\n", "# p\n"),
    );
    ok(
        &env,
        &["plans", "tag", "remove", "quiet-amber-falcon", "area:cli"],
    );
    assert_eq!(
        ok(&env, &["plans", "show", "quiet-amber-falcon"])["tags"],
        json!([])
    );
    let before = read(&file);
    ok(
        &env,
        &["plans", "tag", "remove", "quiet-amber-falcon", "area:cli"],
    );
    assert_eq!(read(&file), before);
}
