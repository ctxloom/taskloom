//! Acceptance: the command grammar of the CLI consolidation (slice A),
//! through the real binary in a temporary home. Red-first: these pin the
//! rulings of `cli-consolidation.plan.md` (noun-verb groups `tasks`,
//! `plans`, `manage` and `loadout`; the old verbs gone, each saying what
//! replaced it; a bare group printing help; no `help` subcommand; `--format`
//! the only machine-output flag; `tasks set`; exit codes 0/1/2; preview by
//! default with `--yes` to apply; help text rules). Each test names one
//! ruling. They spell only the new grammar, and set up through it.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{binary, disk, one_document, Env, Isolated, Ran, Temp};

const SESSION: &str = "grammar-harp";

// ------------------------------------------------------------ helpers

/// `args` run in `env`, which must succeed: its one JSON document.
fn ok(env: &Env, args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.extend(["--format", "json"]);
    let ran = env.run(&args);
    assert!(ran.ok, "{args:?}: {}", ran.stderr);
    one_document(&ran.stdout)
}

/// `args` run in `env`, which must exit `code` with nothing on stdout.
fn refused(env: &Env, args: &[&str], code: i32) -> Ran {
    let ran = env.run(args);
    assert_eq!(ran.code, code, "{args:?}: {}{}", ran.stdout, ran.stderr);
    ran
}

/// Every tag permitted in the project's store, minting the project (every
/// store is an allowlist from when it is made).
fn permit_every_tag(env: &Env) {
    ok(
        env,
        &[
            "manage",
            "schema",
            "declare",
            "--add",
            r#"tagma.permit:"*:*""#,
            "--yes",
        ],
    );
}

/// A task added through `tasks add`: its harp id.
fn add_task(env: &Env, text: &str) -> String {
    ok(env, &["tasks", "add", text])["harp_id"]
        .as_str()
        .expect("a harp id")
        .to_string()
}

fn shown(env: &Env, harp: &str) -> Value {
    ok(env, &["tasks", "show", harp])
}

/// The keys the permits list gives `namespace`, or none if it lists none.
fn permitted_keys(env: &Env, namespace: &str) -> Vec<String> {
    let list = ok(env, &["manage", "schema", "permits", "list"]);
    list["namespaces"]
        .as_array()
        .expect("namespaces")
        .iter()
        .filter(|n| n["namespace"] == namespace)
        .flat_map(|n| n["keys"].as_array().expect("keys").clone())
        .map(|k| k.as_str().expect("a key").to_string())
        .collect()
}

/// `args` as a process that is killed if it has not ended in `patience`,
/// with stdin from /dev/null (a `watch` that ran would never end). A
/// process that had to be killed has code -1.
fn run_bounded(env: &Env, args: &[&str], patience: Duration) -> Ran {
    let mut child = Command::new(binary())
        .args(args)
        .isolated(env.home.path())
        .working_in(env.project.path())
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("taskloom runs");
    let until = Instant::now() + patience;
    while child.try_wait().expect("its status").is_none() {
        if Instant::now() > until {
            child.kill().expect("killed");
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ran::of(child.wait_with_output().expect("its output"))
}

/// The names (and descriptions) under a help text's `Commands:`.
fn commands(help: &str) -> Vec<(String, String)> {
    help.lines()
        .skip_while(|l| *l != "Commands:")
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .map(|l| {
            let l = l.trim_start();
            let (name, about) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
            (name.to_string(), about.trim().to_string())
        })
        .collect()
}

fn names(help: &str) -> BTreeSet<String> {
    commands(help).into_iter().map(|(name, _)| name).collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

/// The lines under a help text's `Examples:`, trimmed.
fn examples(help: &str) -> Vec<String> {
    help.lines()
        .skip_while(|l| l.trim_end() != "Examples:")
        .skip(1)
        .take_while(|l| l.starts_with(' ') && !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect()
}

/// The help text ahead of `Usage:`: the command's one line, if it keeps to
/// one.
fn preamble(help: &str) -> Vec<&str> {
    help.lines()
        .take_while(|l| !l.starts_with("Usage:"))
        .filter(|l| !l.trim().is_empty())
        .collect()
}

/// A command and the help it answers to `--help` and `-h`.
struct Page {
    path: Vec<String>,
    long: Ran,
    short: Ran,
}

impl Page {
    fn children(&self) -> Vec<(String, String)> {
        commands(&self.long.stdout)
    }

    fn is_leaf(&self) -> bool {
        self.children().is_empty()
    }

    fn name(&self) -> String {
        self.path.join(" ")
    }
}

/// Every command reachable from the root by its `Commands:` lists (but not
/// a `help` entry, which another test refuses), each asked for its help.
fn pages() -> &'static [Page] {
    static PAGES: OnceLock<Vec<Page>> = OnceLock::new();
    PAGES.get_or_init(|| {
        let env = Env::new(SESSION);
        let mut pages = Vec::new();
        let mut queue = vec![Vec::<String>::new()];
        while let Some(path) = queue.pop() {
            assert!(path.len() < 6, "a command tree this deep: {path:?}");
            let words: Vec<&str> = path.iter().map(String::as_str).collect();
            let with = |flag: &'static str| [words.clone(), vec![flag]].concat();
            let page = Page {
                long: env.run(&with("--help")),
                short: env.run(&with("-h")),
                path: path.clone(),
            };
            for (name, _) in page.children() {
                if name != "help" {
                    queue.push([path.clone(), vec![name]].concat());
                }
            }
            pages.push(page);
        }
        pages.sort_by(|a, b| a.path.cmp(&b.path));
        pages
    })
}

fn page(path: &[&str]) -> &'static Page {
    pages()
        .iter()
        .find(|p| p.path == path)
        .unwrap_or_else(|| panic!("no command {path:?} in the help"))
}

// ------------------------------------------------------------ 1. groups

/// Every command the rulings name, by path.
const RULED_COMMANDS: &[&[&str]] = &[
    &["tasks", "add"],
    &["tasks", "list"],
    &["tasks", "show"],
    &["tasks", "set"],
    &["tasks", "tag", "add"],
    &["tasks", "tag", "remove"],
    &["tasks", "summary"],
    &["tasks", "prune"],
    &["tasks", "check"],
    &["tasks", "watch"],
    &["plans", "add"],
    &["plans", "list"],
    &["plans", "show"],
    &["plans", "status"],
    &["plans", "link"],
    &["plans", "unlink"],
    &["plans", "check"],
    &["loadout"],
    &["manage", "hook", "stamp-plan"],
    &["manage", "schema", "show"],
    &["manage", "schema", "export"],
    &["manage", "schema", "declare"],
    &["manage", "schema", "check"],
    &["manage", "schema", "permits", "list"],
    &["manage", "schema", "permits", "add"],
    &["manage", "schema", "permits", "remove"],
    &["manage", "schema", "tags", "list"],
    &["manage", "schema", "statuses", "list"],
    &["manage", "migrate"],
    // owner-accepted 2026-11-03: the doctor spelling.
    &["manage", "doctor", "heal"],
    &["manage", "doctor", "repoint"],
    &["manage", "doctor", "orphans"],
    &["manage", "project", "show"],
    &["manage", "version"],
];

#[test]
fn test_the_top_level_groups_are_exactly_tasks_plans_manage_loadout() {
    let env = Env::new(SESSION);
    let help = refused(&env, &["--help"], 0);
    assert_eq!(
        names(&help.stdout),
        set(&["tasks", "plans", "manage", "loadout"]),
        "{}",
        help.stdout
    );
}

#[test]
fn test_each_group_has_exactly_its_ruled_verbs() {
    let children = |path: &[&str]| names(&page(path).long.stdout);
    assert_eq!(
        children(&["tasks"]),
        set(&["add", "list", "show", "set", "tag", "summary", "prune", "check", "watch"])
    );
    assert_eq!(children(&["tasks", "tag"]), set(&["add", "remove"]));
    assert_eq!(
        children(&["manage"]),
        set(&["hook", "schema", "migrate", "doctor", "project", "version"])
    );
    assert_eq!(
        children(&["manage", "schema"]),
        set(&["show", "export", "declare", "check", "permits", "tags", "statuses"])
    );
    assert_eq!(
        children(&["manage", "schema", "permits"]),
        set(&["list", "add", "remove"])
    );
    assert_eq!(children(&["manage", "schema", "tags"]), set(&["list"]));
    assert_eq!(children(&["manage", "schema", "statuses"]), set(&["list"]));
    assert_eq!(children(&["manage", "project"]), set(&["show"]));
    assert_eq!(children(&["manage", "hook"]), set(&["stamp-plan"]));
    assert_eq!(
        children(&["manage", "doctor"]),
        set(&["heal", "repoint", "orphans"])
    );
}

#[test]
fn test_there_is_no_manage_check() {
    // The shell composes the checks (`&&`, a just recipe): owner's ruling.
    let env = Env::new(SESSION);
    refused(&env, &["manage", "check"], 2);
    assert!(!names(&page(&["manage"]).long.stdout).contains("check"));
}

// ------------------------------------------------------------ 1b. removed names

/// One removed top-level verb: run with arguments of the old spelling, it
/// ends with exit 2 and `removed: use <new command>` on stderr, prints
/// nothing on stdout, and runs nothing: no project is minted, no store
/// made. A stub, not an alias.
fn removed(args: &[&str], instead: &str) {
    let env = Env::new(SESSION);
    let ran = run_bounded(&env, args, Duration::from_secs(30));
    assert_eq!(ran.code, 2, "{args:?}: {}{}", ran.stdout, ran.stderr);
    let message = format!("removed: use {instead}");
    assert!(
        ran.stderr.contains(&message),
        "{args:?} should say {message:?}: {}",
        ran.stderr
    );
    assert_eq!(ran.stdout, "", "{args:?}: a stub prints no answer");
    assert!(
        !disk::exists(env.home.path().join(".ctxloom")),
        "{args:?}: nothing ran, so no project or store was made"
    );
}

macro_rules! removed_verbs {
    ($($name:ident: [$($arg:expr),+] => $instead:expr;)+) => {$(
        #[test]
        fn $name() {
            removed(&[$($arg),+], $instead);
        }
    )+};
}

removed_verbs! {
    test_removed_add_says_use_tasks_add: ["add", "buy milk", "--tag", "area:cli"] => "tasks add";
    test_removed_list_says_use_tasks_list: ["list", "--all"] => "tasks list";
    test_removed_show_says_use_tasks_show: ["show", "swift-amber"] => "tasks show";
    test_removed_status_says_use_tasks_set: ["status", "swift-amber", "Done"] => "tasks set";
    test_removed_edit_says_use_tasks_set: ["edit", "swift-amber", "new text"] => "tasks set";
    test_removed_tag_says_use_tasks_tag: ["tag", "swift-amber", "--add", "a:b"] => "tasks tag";
    test_removed_tags_says_use_manage_schema_tags_list: ["tags"] => "manage schema tags list";
    test_removed_summary_says_use_tasks_summary: ["summary"] => "tasks summary";
    test_removed_statuses_says_use_manage_schema_statuses_list: ["statuses"] => "manage schema statuses list";
    test_removed_lint_says_use_tasks_check: ["lint"] => "tasks check";
    test_removed_prune_says_use_tasks_prune_and_prunes_nothing: ["prune", "--yes"] => "tasks prune";
    test_removed_permit_says_use_manage_schema_permits_add: ["permit", "area:cli"] => "manage schema permits add";
    test_removed_revoke_says_use_manage_schema_permits_remove: ["revoke", "area:cli"] => "manage schema permits remove";
    test_removed_permits_says_use_manage_schema_permits_list: ["permits"] => "manage schema permits list";
    test_removed_schema_says_use_manage_schema: ["schema"] => "manage schema";
    test_removed_schema_check_says_use_manage_schema_check: ["schema", "check"] => "manage schema check";
    test_removed_schema_declare_says_use_manage_schema_declare_and_declares_nothing: ["schema", "declare", "--add", "tagma.permit:\"a:b\"", "--yes"] => "manage schema declare";
    test_removed_schema_import_config_says_use_manage_migrate: ["schema", "import-config"] => "manage migrate schema";
    test_removed_import_says_use_manage_migrate: ["import"] => "manage migrate tasks";
    test_removed_project_show_says_use_manage_project_show: ["project", "show"] => "manage project show";
    test_removed_project_heal_says_use_manage_doctor: ["project", "heal", "--yes"] => "manage doctor heal";
    test_removed_project_repoint_says_use_manage_doctor_repoint: ["project", "repoint", "some-id", "--yes"] => "manage doctor repoint";
    test_removed_project_orphans_says_use_manage_doctor_orphans: ["project", "orphans"] => "manage doctor orphans";
    test_removed_plan_says_use_plans: ["plan"] => "plans";
    test_removed_plan_list_says_use_plans_list: ["plan", "list"] => "plans list";
    test_removed_plan_check_says_use_plans_check: ["plan", "check"] => "plans check";
    test_removed_watch_says_use_tasks_watch_and_does_not_watch: ["watch"] => "tasks watch";
    test_removed_version_says_use_manage_version: ["version"] => "manage version";
}

#[test]
fn test_help_is_no_command_at_the_top_or_below() {
    // Not a removed-name stub: there never is a `help` command now, so it
    // is an unknown one, a usage error.
    let env = Env::new(SESSION);
    for args in [
        &["help"][..],
        &["tasks", "help"],
        &["manage", "schema", "help"],
    ] {
        let ran = refused(&env, args, 2);
        assert_eq!(ran.stdout, "", "{args:?}");
    }
}

// ------------------------------------------------------------ 2. help

/// A bare group prints its help on stdout (decision: stdout, so `| less`
/// and `| grep` work, and a usage error keeps stderr) and exits 0, and
/// reads nothing: no project is made.
fn bare(path: &[&str]) {
    let env = Env::new(SESSION);
    let ran = refused(&env, path, 0);
    let usage = format!("Usage: taskloom {}", path.join(" "));
    let usage = usage.replace("taskloom  ", "taskloom ");
    assert!(
        ran.stdout.contains(usage.trim_end()) && ran.stdout.contains("Commands:"),
        "{path:?} prints its help: {}",
        ran.stdout
    );
    assert_eq!(ran.stderr, "", "{path:?}");
    assert!(
        !disk::exists(env.home.path().join(".ctxloom")),
        "{path:?}: a bare group reads nothing"
    );
}

macro_rules! bare_groups {
    ($($name:ident: [$($word:expr),*];)+) => {$(
        #[test]
        fn $name() {
            bare(&[$($word),*]);
        }
    )+};
}

bare_groups! {
    test_bare_taskloom_prints_help: [];
    test_bare_tasks_prints_help_and_does_not_list: ["tasks"];
    test_bare_plans_prints_help_and_does_not_list: ["plans"];
    test_bare_manage_prints_help: ["manage"];
    test_bare_manage_schema_prints_help_and_does_not_show: ["manage", "schema"];
    test_bare_manage_schema_permits_prints_help: ["manage", "schema", "permits"];
    test_bare_manage_schema_tags_prints_help: ["manage", "schema", "tags"];
    test_bare_manage_schema_statuses_prints_help: ["manage", "schema", "statuses"];
    test_bare_tasks_tag_prints_help: ["tasks", "tag"];
    test_bare_manage_project_prints_help_and_does_not_show: ["manage", "project"];
    test_bare_manage_hook_prints_help: ["manage", "hook"];
    test_bare_manage_doctor_prints_help_and_does_not_heal: ["manage", "doctor"];
    test_bare_manage_migrate_prints_help_and_does_not_migrate: ["manage", "migrate"];
}

#[test]
fn test_no_command_lists_a_help_subcommand() {
    for page in pages() {
        assert!(
            !names(&page.long.stdout).contains("help"),
            "{}: {}",
            page.name(),
            page.long.stdout
        );
    }
}

#[test]
fn test_every_command_answers_dash_h_and_dash_dash_help_with_its_usage() {
    // The ruled tree is all there to be asked: each path is a command.
    for path in RULED_COMMANDS {
        page(path);
    }
    for page in pages() {
        for (flag, ran) in [("--help", &page.long), ("-h", &page.short)] {
            assert_eq!(ran.code, 0, "{} {flag}: {}", page.name(), ran.stderr);
            assert!(
                ran.stdout.contains("Usage: taskloom"),
                "{} {flag}: {}",
                page.name(),
                ran.stdout
            );
            assert_eq!(ran.stderr, "", "{} {flag}", page.name());
        }
    }
}

// ------------------------------------------------------------ 3. --format

#[test]
fn test_format_json_and_text_are_accepted_and_json_flag_is_not_listed() {
    let env = Env::new(SESSION);
    let listed = ok(&env, &["tasks", "list"]);
    assert_eq!(listed, json!([]));
    let text = refused(&env, &["tasks", "list", "--format", "text"], 0);
    assert!(
        serde_json::from_str::<Value>(&text.stdout).is_err(),
        "text is not JSON: {}",
        text.stdout
    );
    refused(&env, &["tasks", "list", "--format=json"], 0);
    let help = &page(&[]).long.stdout;
    assert!(help.contains("--format <FORMAT>"), "{help}");
    assert!(!help.contains("--json"), "{help}");
}

#[test]
fn test_json_flag_after_the_command_is_refused_with_use_format_json() {
    let env = Env::new(SESSION);
    for args in [
        &["tasks", "list", "--json"][..],
        &["manage", "version", "--json"],
        &["loadout", "--json"],
    ] {
        let ran = refused(&env, args, 2);
        assert!(
            ran.stderr.contains("removed: use --format json"),
            "{args:?}: {}",
            ran.stderr
        );
        assert_eq!(ran.stdout, "", "{args:?}: nothing ran");
    }
    assert!(!disk::exists(env.home.path().join(".ctxloom")));
}

#[test]
fn test_json_flag_before_the_command_is_refused_with_use_format_json() {
    let env = Env::new(SESSION);
    let ran = refused(&env, &["--json", "tasks", "list"], 2);
    assert!(
        ran.stderr.contains("removed: use --format json"),
        "{}",
        ran.stderr
    );
}

#[test]
fn test_every_command_help_lists_format_and_none_lists_json() {
    for page in pages().iter().filter(|p| p.is_leaf()) {
        assert!(
            page.long.stdout.contains("--format <FORMAT>"),
            "{}: {}",
            page.name(),
            page.long.stdout
        );
        assert!(!page.long.stdout.contains("--json"), "{}", page.name());
    }
}

#[test]
fn test_the_loadout_no_longer_tells_agents_to_use_json() {
    let env = Env::new(SESSION);
    let ran = refused(&env, &["loadout", "--format", "yaml"], 0);
    assert!(!ran.stdout.contains("--json"), "{}", ran.stdout);
    assert!(ran.stdout.contains("taskloom tasks "), "{}", ran.stdout);
}

// ------------------------------------------------------------ 4. tasks set, tasks tag

#[test]
fn test_tasks_set_changes_text_alone_status_alone_and_both() {
    let env = Env::new(SESSION);
    let harp = add_task(&env, "First");

    let text = ok(&env, &["tasks", "set", &harp, "--text", "Second"]);
    assert_eq!(
        (text["text"].as_str(), text["status"].as_str()),
        (Some("Second"), Some("To Do"))
    );

    let status = ok(&env, &["tasks", "set", &harp, "--status", "In Progress"]);
    assert_eq!(
        (status["text"].as_str(), status["status"].as_str()),
        (Some("Second"), Some("In Progress"))
    );

    let both = ok(
        &env,
        &["tasks", "set", &harp, "--text", "Third", "--status", "Done"],
    );
    assert_eq!(
        (both["text"].as_str(), both["status"].as_str()),
        (Some("Third"), Some("Done"))
    );
    assert_eq!(shown(&env, &harp)["status"], "Done", "kept");
}

#[test]
fn test_tasks_set_trigger_defers_with_status_deferred() {
    let env = Env::new(SESSION);
    let harp = add_task(&env, "Waits");
    let deferred = ok(
        &env,
        &[
            "tasks",
            "set",
            &harp,
            "--status",
            "Deferred",
            "--trigger",
            "when it ships",
        ],
    );
    assert_eq!(deferred["status"], "Deferred");
    assert_eq!(deferred["trigger"], "when it ships");
}

#[test]
fn test_tasks_set_with_nothing_to_set_is_a_usage_error() {
    let env = Env::new(SESSION);
    let harp = add_task(&env, "Idle");
    let ran = refused(&env, &["tasks", "set", &harp], 2);
    assert_eq!(ran.stdout, "");
}

#[test]
fn test_tasks_set_applies_text_and_status_under_distinct_request_ids() {
    // One --request-id, two commands (EditText, SetStatus): were they to
    // share the key, the second would be answered as the first's duplicate
    // and its change dropped.
    let env = Env::new(SESSION);
    let harp = add_task(&env, "First");
    let set = ok(
        &env,
        &[
            "tasks",
            "set",
            &harp,
            "--text",
            "Edited",
            "--status",
            "In Progress",
            "--request-id",
            "r-1",
        ],
    );
    assert_eq!(set["text"], "Edited");
    assert_eq!(set["status"], "In Progress");
}

#[test]
fn test_tasks_set_replayed_under_its_request_id_applies_once() {
    let env = Env::new(SESSION);
    let harp = add_task(&env, "First");
    let first = [
        "tasks",
        "set",
        &harp,
        "--text",
        "Edited",
        "--status",
        "In Progress",
        "--request-id",
        "r-1",
    ];
    let applied = ok(&env, &first);
    assert_eq!(
        ok(&env, &first),
        applied,
        "the replay answers as the first did"
    );

    // Another request changes both; replaying the first no longer undoes it.
    ok(
        &env,
        &["tasks", "set", &harp, "--text", "Later", "--status", "Done"],
    );
    let replayed = ok(&env, &first);
    assert_eq!(
        replayed["text"], "Later",
        "the text edit is not applied again"
    );
    assert_eq!(replayed["status"], "Done", "nor the status change");
}

#[test]
fn test_tasks_set_with_a_blank_request_id_is_refused_before_it_runs() {
    let env = Env::new(SESSION);
    let harp = add_task(&env, "First");
    refused(
        &env,
        &["tasks", "set", &harp, "--text", "X", "--request-id", " "],
        2,
    );
    assert_eq!(shown(&env, &harp)["text"], "First");
}

#[test]
fn test_tasks_tag_add_and_remove_take_the_harp_and_one_tag() {
    let env = Env::new(SESSION);
    permit_every_tag(&env);
    let harp = add_task(&env, "Tagged");
    let added = ok(&env, &["tasks", "tag", "add", &harp, "area:cli"]);
    assert_eq!(added["tags"], json!(["area:cli"]));
    let removed = ok(&env, &["tasks", "tag", "remove", &harp, "area:cli"]);
    assert_eq!(removed["tags"], json!([]));
}

#[test]
fn test_tasks_tag_replayed_under_its_request_id_applies_once() {
    let env = Env::new(SESSION);
    permit_every_tag(&env);
    let harp = add_task(&env, "Tagged");
    let adding = ["tasks", "tag", "add", &harp, "x", "--request-id", "r-1"];
    ok(&env, &adding);
    ok(&env, &["tasks", "tag", "remove", &harp, "x"]);
    assert_eq!(ok(&env, &adding)["tags"], json!([]), "not applied again");
}

// ------------------------------------------------------------ 5. exit codes

#[test]
fn test_a_usage_error_exits_2_on_stderr_and_prints_no_answer() {
    let env = Env::new(SESSION);
    for args in [
        &["tasks", "add"][..],                   // a missing argument
        &["tasks", "list", "--no-such-flag"],    // an unknown flag
        &["tasks", "no-such-verb"],              // an unknown verb
        &["no-such-group"],                      // an unknown group
        &["tasks", "list", "--format", "bogus"], // a bad value
        &["tasks", "show"],                      // a missing harp id
        &["manage", "doctor", "repoint"],        // a missing project id
    ] {
        let ran = refused(&env, args, 2);
        assert_eq!(ran.stdout, "", "{args:?}");
        assert!(!ran.stderr.is_empty(), "{args:?} says why");
    }
}

#[test]
fn test_a_tag_no_permit_admits_is_refused_with_2_and_adds_nothing() {
    let env = Env::new(SESSION);
    add_task(&env, "Seed");
    let ran = refused(&env, &["tasks", "add", "Tagged", "--tag", "nope:thing"], 2);
    assert!(ran.stderr.contains("not-permitted"), "{}", ran.stderr);
    assert_eq!(
        ok(&env, &["tasks", "list"]).as_array().map(Vec::len),
        Some(1)
    );
}

#[test]
fn test_tagging_with_a_tag_no_permit_admits_exits_2_and_changes_nothing() {
    let env = Env::new(SESSION);
    let harp = add_task(&env, "Seed");
    refused(&env, &["tasks", "tag", "add", &harp, "nope:thing"], 2);
    assert_eq!(shown(&env, &harp)["tags"], json!([]));
}

#[test]
fn test_arguments_refused_before_anything_ran_exit_2() {
    // Each is wrong on its face, whatever the store holds.
    let env = Env::new(SESSION);
    add_task(&env, "Seed");
    for args in [
        &["tasks", "list", "--limit", "0"][..],
        &["tasks", "prune", "--older-than", "zzz"],
        &["tasks", "list", "--tag-query", "x/and"],
        &["tasks", "list", "--status", "@nonsense"],
        &["manage", "schema", "permits", "add", "*bad"],
        &["manage", "schema", "declare", "--add", "bad", "--yes"],
        &["tasks", "list", "--format", "yaml"],
        &["loadout", "--format", "json"],
    ] {
        refused(&env, args, 2);
    }
}

#[test]
fn test_a_check_that_finds_a_violation_exits_1_naming_it() {
    let env = Env::new(SESSION);
    permit_every_tag(&env);
    let clean = ok(&env, &["tasks", "check"]);
    assert_eq!(clean["violations"], json!([]));
    assert!(clean["checked_targets"].as_u64() > Some(0), "{clean}");

    let harp = add_task(&env, "Off the enum");
    ok(&env, &["tasks", "tag", "add", &harp, "area:x=c"]);
    ok(
        &env,
        &[
            "manage",
            "schema",
            "declare",
            "--yes",
            "--add",
            r#"tagma.enum:"area:x"=a"#,
            "--add",
            r#"tagma.enum:"area:x"=b"#,
        ],
    );
    let ran = refused(&env, &["tasks", "check", "--format", "json"], 1);
    assert_eq!(
        one_document(&ran.stdout)["violations"][0]["harp_id"],
        harp.as_str()
    );
}

#[test]
fn test_schema_check_exits_0_when_clean_and_1_on_a_finding() {
    let env = Env::new(SESSION);
    add_task(&env, "Seed");
    for permit in ["area:cli", "repo:tagma"] {
        ok(&env, &["manage", "schema", "permits", "add", permit]);
    }
    assert_eq!(ok(&env, &["manage", "schema", "check"]), json!([]));
    ok(
        &env,
        &[
            "manage",
            "schema",
            "declare",
            "--yes",
            "--add",
            r#"tagma.permit:"*:secret"=false"#,
        ],
    );
    let ran = refused(&env, &["manage", "schema", "check", "--format", "json"], 1);
    assert_eq!(one_document(&ran.stdout)[0]["code"], "malformed-value");
}

#[test]
fn test_plans_check_exits_0_when_clean_and_1_on_a_finding() {
    let env = Env::new(SESSION);
    let clean = ok(&env, &["plans", "check"]);
    assert_eq!(clean, json!({"ok": true, "findings": []}));
    let added = ok(&env, &["plans", "add", "--title", "Plans"]);
    let rel = added["rel_path"].as_str().expect("a path").to_string();
    disk::remove_file(env.project.path().join(&rel)).unwrap();
    let ran = refused(&env, &["plans", "check"], 1);
    assert_eq!(one_document(&ran.stdout)["ok"], false);
}

#[test]
fn test_the_store_refusing_a_write_exits_1() {
    // Revoking a permit a task's tag still needs: the arguments are fine,
    // the store refuses.
    let env = Env::new(SESSION);
    ok(&env, &["manage", "schema", "permits", "add", "area:cli"]);
    let harp = add_task(&env, "Holds the tag");
    ok(&env, &["tasks", "tag", "add", &harp, "area:cli"]);
    refused(
        &env,
        &["manage", "schema", "permits", "remove", "area:cli", "--yes"],
        1,
    );
    assert_eq!(permitted_keys(&env, "area"), ["cli"], "still permitted");
}

#[test]
fn test_help_and_success_exit_0() {
    let env = Env::new(SESSION);
    refused(&env, &["tasks", "--help"], 0);
    refused(&env, &["manage", "version"], 0);
    refused(&env, &["tasks", "list"], 0);
}

// ------------------------------------------------------------ 6/7. the verbs

#[test]
fn test_permits_add_list_remove_under_manage_schema() {
    let env = Env::new(SESSION);
    let added = ok(&env, &["manage", "schema", "permits", "add", "area:cli"]);
    assert_eq!(added, json!({"permit": "area:cli", "changed": true}));
    let again = ok(&env, &["manage", "schema", "permits", "add", "area:cli"]);
    assert_eq!(again["changed"], false);
    assert_eq!(permitted_keys(&env, "area"), ["cli"]);
    let removed = ok(
        &env,
        &["manage", "schema", "permits", "remove", "area:cli", "--yes"],
    );
    assert_eq!(removed["changed"], true);
    assert!(permitted_keys(&env, "area").is_empty());
}

#[test]
fn test_schema_tags_list_keeps_status_term_tag_query_and_global() {
    let env = Env::new(SESSION);
    permit_every_tag(&env);
    let harp = add_task(&env, "Releasing");
    ok(&env, &["tasks", "tag", "add", &harp, "release"]);
    let list = |extra: &[&str]| {
        let args = [&["manage", "schema", "tags", "list"][..], extra].concat();
        ok(&env, &args)
    };
    let one = json!([{"tag": "release", "active": 1, "total": 1}]);
    assert_eq!(list(&[]), one);
    assert_eq!(list(&["--status", "@open"]), one);
    assert_eq!(list(&["--term", "releas"]), one);
    assert_eq!(list(&["--tag-query", "release"]), one);
    assert_eq!(list(&["--global"]), one);
    assert_eq!(
        list(&["--status", "Done"]),
        json!([]),
        "narrows the tasks counted"
    );
    assert_eq!(list(&["--term", "nothing like it"]), json!([]));
}

#[test]
fn test_schema_statuses_list_names_the_defaults_in_display_order() {
    let env = Env::new(SESSION);
    add_task(&env, "Seed");
    let statuses = ok(&env, &["manage", "schema", "statuses", "list"]);
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
    ok(&env, &["manage", "schema", "statuses", "list", "--global"]);
}

#[test]
fn test_schema_show_and_export_are_verbs_not_the_bare_default() {
    let env = Env::new(SESSION);
    add_task(&env, "Seed");
    assert!(ok(&env, &["manage", "schema", "show"])["targets"].is_array());
    let exported = refused(&env, &["manage", "schema", "export"], 0);
    assert!(exported.stdout.contains("tagma."), "{}", exported.stdout);
}

#[test]
fn test_manage_version_reports_the_name_and_version() {
    let env = Env::new(SESSION);
    let version = ok(&env, &["manage", "version"]);
    assert_eq!(version["name"], "taskloom");
    assert!(version["version"].is_string(), "{version}");
}

#[test]
fn test_manage_project_show_names_the_project() {
    let env = Env::new(SESSION);
    add_task(&env, "Seed");
    let project = ok(&env, &["manage", "project", "show"]);
    assert!(project["project_id"].is_string(), "{project}");
    assert_eq!(project["problems"], json!([]));
}

#[test]
// owner-accepted 2026-11-03: the spelling `manage doctor heal|repoint|orphans [remove]`.
fn test_manage_doctor_heal_reports_nothing_to_change_in_a_sound_project() {
    let env = Env::new(SESSION);
    add_task(&env, "Seed");
    let healed = ok(&env, &["manage", "doctor", "heal"]);
    assert_eq!(healed["applied"], false);
    assert_eq!(healed["changes"], json!([]));
}

#[test]
fn test_manage_hook_stamp_plan_is_a_command_of_manage_hook() {
    // Only its place: what it does is slice C's.
    let help = page(&["manage", "hook", "stamp-plan"]);
    assert_eq!(help.long.code, 0);
    assert!(help.is_leaf());
}

#[test]
fn test_manage_migrate_has_two_children_tasks_and_schema() {
    // owner-accepted 2026-11-03: `plan import` is gone.
    assert_eq!(
        names(&page(&["manage", "migrate"]).long.stdout),
        set(&["tasks", "schema"])
    );
}

// ------------------------------------------------------------ 8. confirmation

#[test]
fn test_prune_previews_by_default_dry_run_applies_nothing_and_yes_applies() {
    let env = Env::new(SESSION);
    let done = ok(&env, &["tasks", "add", "Finished", "--status", "Done"])["harp_id"]
        .as_str()
        .unwrap()
        .to_string();
    let listed = |env: &Env| -> Vec<String> {
        ok(env, &["tasks", "list", "--all"])
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["harp_id"].as_str().unwrap().to_string())
            .collect()
    };
    for flags in [&[][..], &["--dry-run"]] {
        let args = [&["tasks", "prune", "--older-than", "1ns"][..], flags].concat();
        let report = ok(&env, &args);
        assert_eq!(report["applied"], false, "{flags:?}");
        assert_eq!(
            report["pruned"][0]["harp_id"],
            done.as_str(),
            "{flags:?} says what it would take"
        );
        assert_eq!(
            listed(&env),
            std::slice::from_ref(&done),
            "{flags:?} changed nothing"
        );
    }
    let applied = ok(&env, &["tasks", "prune", "--older-than", "1ns", "--yes"]);
    assert_eq!(applied["applied"], true);
    assert!(listed(&env).is_empty());
}

#[test]
fn test_schema_declare_previews_by_default_dry_run_applies_nothing_and_yes_applies() {
    let env = Env::new(SESSION);
    add_task(&env, "Seed");
    let rule = r#"tagma.permit:"zzspace:key""#;
    for flags in [&[][..], &["--dry-run"]] {
        let args = [&["manage", "schema", "declare", "--add", rule][..], flags].concat();
        refused(&env, &args, 0);
        assert!(
            permitted_keys(&env, "zzspace").is_empty(),
            "{flags:?} declared nothing"
        );
    }
    let declared = ok(
        &env,
        &["manage", "schema", "declare", "--add", rule, "--yes"],
    );
    assert_eq!(declared["changed"], true);
    assert_eq!(permitted_keys(&env, "zzspace"), ["key"]);
}

#[test]
fn test_permits_remove_previews_by_default_dry_run_removes_nothing_and_yes_removes() {
    let env = Env::new(SESSION);
    ok(&env, &["manage", "schema", "permits", "add", "area:cli"]);
    for flags in [&[][..], &["--dry-run"]] {
        let args = [
            &["manage", "schema", "permits", "remove", "area:cli"][..],
            flags,
        ]
        .concat();
        refused(&env, &args, 0);
        assert_eq!(
            permitted_keys(&env, "area"),
            ["cli"],
            "{flags:?} removed nothing"
        );
    }
    ok(
        &env,
        &["manage", "schema", "permits", "remove", "area:cli", "--yes"],
    );
    assert!(permitted_keys(&env, "area").is_empty());
}

#[test]
// owner-accepted 2026-11-03: the spelling `manage doctor heal|repoint|orphans [remove]`.
fn test_doctor_heal_previews_by_default_dry_run_writes_nothing_and_yes_heals() {
    let env = Env::new(SESSION);
    add_task(&env, "Seed");
    let marker = env.project.path().join(".project-id");
    disk::remove_file(&marker).unwrap();
    for flags in [&[][..], &["--dry-run"]] {
        let args = [&["manage", "doctor", "heal"][..], flags].concat();
        let would = ok(&env, &args);
        assert_eq!(would["applied"], false, "{flags:?}");
        assert_eq!(would["changes"][0]["kind"], "write-marker", "{flags:?}");
        assert!(!disk::exists(&marker), "{flags:?} wrote nothing");
    }
    let healed = ok(&env, &["manage", "doctor", "heal", "--yes"]);
    assert_eq!(healed["applied"], true);
    assert!(disk::exists(&marker));
}

/// Two trees whose projects forked: the copy's project, then repointed at
/// the original's, leaves its own database an orphan.
struct Forked {
    env: Env,
    copy: Temp,
    kept: String,
    forked: String,
}

impl Forked {
    fn new() -> Forked {
        let env = Env::new(SESSION);
        let copy = Temp::new();
        let at = |env: &Env, dir: &Path, text: &str| -> String {
            let args = ["tasks", "add", text, "--format", "json"];
            assert!(env.run_in(dir, &args, &[]).ok);
            let shown = env.run_in(dir, &["manage", "project", "show", "--format", "json"], &[]);
            one_document(&shown.stdout)["project_id"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let kept = at(&env, env.project.path(), "the original's task");
        let forked = at(&env, copy.path(), "the copy's task");
        assert_ne!(kept, forked);
        Forked {
            env,
            copy,
            kept,
            forked,
        }
    }

    fn run(&self, args: &[&str]) -> Ran {
        let mut args = args.to_vec();
        args.extend(["--format", "json"]);
        self.env.run_in(self.copy.path(), &args, &[])
    }

    fn marker(&self) -> String {
        disk::read_to_string(self.copy.path().join(".project-id")).unwrap()
    }

    fn database(&self, id: &str) -> std::path::PathBuf {
        self.env
            .home
            .path()
            .join(".ctxloom")
            .join("projects")
            .join(id)
    }
}

#[test]
// owner-accepted 2026-11-03: the spelling `manage doctor heal|repoint|orphans [remove]`.
fn test_doctor_repoint_previews_by_default_dry_run_binds_nothing_and_yes_binds() {
    let world = Forked::new();
    let before = world.marker();
    for flags in [&[][..], &["--dry-run"]] {
        let args = [
            &["manage", "doctor", "repoint", &world.kept, "--share"][..],
            flags,
        ]
        .concat();
        let ran = world.run(&args);
        assert!(ran.ok, "{flags:?}: {}", ran.stderr);
        assert_eq!(ran.json()["applied"], false, "{flags:?}");
        assert_eq!(world.marker(), before, "{flags:?} bound nothing");
    }
    let made = world.run(&[
        "manage",
        "doctor",
        "repoint",
        &world.kept,
        "--share",
        "--yes",
    ]);
    assert!(made.ok, "{}", made.stderr);
    assert_eq!(made.json()["applied"], true);
    assert_eq!(world.marker(), format!("{}\n", world.kept));
}

#[test]
// owner-accepted 2026-11-03: the spelling `manage doctor heal|repoint|orphans [remove]`.
fn test_doctor_orphans_remove_previews_by_default_dry_run_keeps_it_and_yes_removes() {
    let world = Forked::new();
    let made = world.run(&[
        "manage",
        "doctor",
        "repoint",
        &world.kept,
        "--share",
        "--yes",
    ]);
    assert!(made.ok, "{}", made.stderr);
    for flags in [&[][..], &["--dry-run"]] {
        let args = [
            &["manage", "doctor", "orphans", "remove", &world.forked][..],
            flags,
        ]
        .concat();
        let ran = world.run(&args);
        assert!(ran.ok, "{flags:?}: {}", ran.stderr);
        assert_eq!(ran.json()["applied"], false, "{flags:?}");
        assert!(
            disk::exists(world.database(&world.forked)),
            "{flags:?} removed nothing"
        );
    }
    let removed = world.run(&[
        "manage",
        "doctor",
        "orphans",
        "remove",
        &world.forked,
        "--yes",
    ]);
    assert!(removed.ok, "{}", removed.stderr);
    assert_eq!(removed.json()["applied"], true);
    assert!(!disk::exists(world.database(&world.forked)));
    assert!(disk::exists(world.database(&world.kept).join("state.db")));
}

// ------------------------------------------------------------ 9. help text

#[test]
fn test_each_command_is_described_in_one_line_in_its_parents_list() {
    // owner-accepted 2026-11-03.
    const LONGEST: usize = 80;
    for page in pages() {
        for (name, about) in page.children() {
            assert!(
                !about.is_empty(),
                "{} {name} has no description",
                page.name()
            );
            assert!(
                about.chars().count() <= LONGEST,
                "{} {name}: {} chars: {about}",
                page.name(),
                about.chars().count()
            );
        }
    }
}

#[test]
fn test_each_commands_help_opens_with_exactly_one_short_line() {
    // owner-accepted 2026-11-03.
    const LONGEST: usize = 80;
    for page in pages().iter().filter(|p| !p.path.is_empty()) {
        let lines = preamble(&page.long.stdout);
        assert_eq!(lines.len(), 1, "{}: {lines:?}", page.name());
        assert!(
            lines[0].chars().count() <= LONGEST,
            "{}: {}",
            page.name(),
            lines[0]
        );
    }
}

#[test]
fn test_each_leaf_command_help_has_one_or_two_examples_of_itself() {
    for page in pages().iter().filter(|p| p.is_leaf() && !p.path.is_empty()) {
        let found = examples(&page.long.stdout);
        let own = format!("taskloom {}", page.name());
        assert!(
            (1..=2).contains(&found.len()),
            "{}: {found:?} in {}",
            page.name(),
            page.long.stdout
        );
        assert!(
            found.iter().all(|e| e.starts_with(&own)),
            "{}: {found:?}",
            page.name()
        );
    }
}

#[test]
fn test_command_help_does_not_explain_ctxloom_root_or_the_work_tree() {
    // `manage project` is the one place the resolution is explained.
    for page in pages() {
        if page
            .path
            .starts_with(&["manage".to_string(), "project".to_string()])
        {
            continue;
        }
        for help in [&page.long.stdout, &page.short.stdout] {
            let lower = help.to_lowercase();
            assert!(!lower.contains("ctxloom_root"), "{}: {help}", page.name());
            assert!(!lower.contains("work tree"), "{}: {help}", page.name());
            assert!(!lower.contains("worktree"), "{}: {help}", page.name());
        }
    }
}

#[test]
fn test_only_loadout_mentions_yaml_as_a_format() {
    for page in pages().iter().filter(|p| p.path != ["loadout"]) {
        for line in page.long.stdout.lines().filter(|l| l.contains("--format")) {
            assert!(
                !line.to_lowercase().contains("yaml"),
                "{}: {line}",
                page.name()
            );
        }
    }
}

#[test]
fn test_top_level_help_is_under_two_kilobytes() {
    // owner-accepted 2026-11-03; today's
    // is about 5 KB.
    const BOUND: usize = 2048;
    let help = &page(&[]).long.stdout;
    assert!(help.len() < BOUND, "{} bytes: {help}", help.len());
}

// ------------------------------------------------------------ 10. plans

#[test]
fn test_plans_keeps_what_plan_did_add_list_show_status_link_check() {
    let env = Env::new(SESSION);
    let task = add_task(&env, "Move plans into taskloom");
    let added = ok(
        &env,
        &["plans", "add", "--title", "Plans", "--governs", &task],
    );
    let id = added["id"].as_str().unwrap().to_string();
    let rel = format!(".taskloom/plans/{id}.plan.md");
    assert_eq!(added["rel_path"], rel.as_str());
    assert_eq!(added["status"], "draft");
    assert_eq!(added["governs"], json!([task]));
    let file = env.project.path().join(&rel);
    let written = disk::read_to_string(&file).expect("the plan's file");
    assert!(written.contains("title: Plans"), "{written}");

    // The VS Code extension's fields.
    let listed = ok(&env, &["plans", "list"]);
    assert_eq!(listed[0]["name"], id.as_str());
    assert_eq!(listed[0]["title"], "Plans");
    assert_eq!(listed[0]["session"], SESSION);
    assert_eq!(listed[0]["sessions"], json!([SESSION]));
    assert_eq!(listed[0]["path"], file.to_string_lossy().as_ref());

    let raw = refused(&env, &["plans", "show", &id, "--format", "text"], 0);
    assert_eq!(raw.stdout, written);
    assert_eq!(
        ok(&env, &["plans", "status", &id, "active"])["status"],
        "active"
    );
    assert_eq!(
        ok(&env, &["plans", "list", "--governs", &task])[0]["id"],
        id.as_str()
    );
}

#[test]
fn test_plans_exit_codes_stay_1_for_a_missing_plan_and_2_for_a_status_none() {
    let env = Env::new(SESSION);
    refused(&env, &["plans", "show", "no-such-plan"], 1);
    let ran = refused(
        &env,
        &["plans", "add", "--title", "T", "--status", "nope"],
        2,
    );
    assert!(ran.stderr.contains("nope"), "{}", ran.stderr);
}

// ------------------------------------------------------------ 5b. exit codes, the owner's rule
// 2: invalid on its own content, refused before state is read. 1: it ran
// against state and the answer was negative.

#[test]
fn test_an_out_of_range_tag_value_is_refused_with_2_and_adds_nothing() {
    let env = Env::new(SESSION);
    let harp = add_task(&env, "Seed");
    // The seeded rules: triage:level is 1-5.
    let ran = refused(
        &env,
        &["tasks", "add", "Tagged", "--tag", "triage:level=7"],
        2,
    );
    assert!(ran.stderr.contains("out-of-range"), "{}", ran.stderr);
    refused(&env, &["tasks", "tag", "add", &harp, "triage:level=7"], 2);
    assert_eq!(shown(&env, &harp)["tags"], json!([]));
    assert_eq!(
        ok(&env, &["tasks", "list"]).as_array().map(Vec::len),
        Some(1)
    );
}

#[test]
fn test_a_value_outside_a_declared_enum_is_refused_with_2() {
    let env = Env::new(SESSION);
    permit_every_tag(&env);
    ok(
        &env,
        &[
            "manage",
            "schema",
            "declare",
            "--yes",
            "--add",
            r#"tagma.enum:"area:x"=a"#,
            "--add",
            r#"tagma.enum:"area:x"=b"#,
        ],
    );
    let harp = add_task(&env, "Seed");
    let ran = refused(&env, &["tasks", "tag", "add", &harp, "area:x=c"], 2);
    assert!(ran.stderr.contains("not-in-enum"), "{}", ran.stderr);
    assert_eq!(shown(&env, &harp)["tags"], json!([]));
    ok(&env, &["tasks", "tag", "add", &harp, "area:x=b"]);
}

#[test]
fn test_adding_a_deferred_task_with_no_trigger_is_refused_with_2() {
    let env = Env::new(SESSION);
    add_task(&env, "Seed");
    let ran = refused(&env, &["tasks", "add", "Waits", "--status", "Deferred"], 2);
    assert_eq!(ran.stdout, "");
    assert_eq!(
        ok(&env, &["tasks", "list", "--all"])
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn test_a_task_no_store_holds_is_answered_with_1_on_show_and_on_set() {
    let env = Env::new(SESSION);
    add_task(&env, "Seed");
    let shown = refused(&env, &["tasks", "show", "no-such-task"], 1);
    assert_eq!(shown.stdout, "");
    refused(&env, &["tasks", "set", "no-such-task", "--text", "X"], 1);
    refused(
        &env,
        &["tasks", "set", "no-such-task", "--status", "Done"],
        1,
    );
}

// ------------------------------------------------------------ owner-accepted 2026-11-03

#[test]
fn test_removed_plan_import_says_plans_are_files_and_imports_nothing() {
    // owner-accepted 2026-11-03.
    let env = Env::new(SESSION);
    let ran = refused(&env, &["plan", "import"], 2);
    assert!(
        ran.stderr
            .contains("removed: plans are files; there is nothing to import"),
        "{}",
        ran.stderr
    );
    assert_eq!(ran.stdout, "");
    assert!(!disk::exists(env.home.path().join(".ctxloom")));
}

/// `args` with `--dry-run --yes`: exit 0, and the output says in text and
/// in JSON that `--yes` was given and ignored, and what it would do without
/// `--dry-run` (`needle` names one such operation).
fn says_yes_was_ignored(env: &Env, args: &[&str], needle: &str) -> Value {
    let json = ok(env, args);
    assert_eq!(json["yes_ignored_by_dry_run"], true, "{args:?}: {json}");
    let would: Vec<&str> = json["would_apply"]
        .as_array()
        .unwrap_or_else(|| panic!("{args:?}: a would_apply list: {json}"))
        .iter()
        .map(|w| w.as_str().unwrap())
        .collect();
    assert!(
        would.iter().any(|w| w.contains(needle)),
        "{args:?}: would_apply names {needle}: {would:?}"
    );
    let text = refused(env, &[args, &["--format", "text"]].concat(), 0).stdout;
    assert!(
        text.contains("--yes"),
        "{args:?}: says --yes was given: {text}"
    );
    assert!(text.contains("--dry-run"), "{args:?}: {text}");
    assert!(
        text.contains(needle),
        "{args:?}: names the operation: {text}"
    );
    json
}

#[test]
fn test_dry_run_together_with_yes_is_absolute_applies_nothing_and_says_so() {
    // owner ruling 2026-11-03: --dry-run wins over --yes; it previews, exit
    // 0, and says that --yes was given and what it would otherwise do.
    let env = Env::new(SESSION);
    let done = ok(&env, &["tasks", "add", "Finished", "--status", "Done"])["harp_id"]
        .as_str()
        .unwrap()
        .to_string();
    ok(&env, &["manage", "schema", "permits", "add", "area:cli"]);
    let prune = says_yes_was_ignored(
        &env,
        &[
            "tasks",
            "prune",
            "--older-than",
            "1ns",
            "--dry-run",
            "--yes",
        ],
        &done,
    );
    assert_eq!(prune["applied"], false);
    assert_eq!(prune["pruned"][0]["harp_id"], done.as_str());
    says_yes_was_ignored(
        &env,
        &[
            "manage",
            "schema",
            "declare",
            "--add",
            r#"tagma.permit:"zz:y""#,
            "--dry-run",
            "--yes",
        ],
        "zz:y",
    );
    says_yes_was_ignored(
        &env,
        &[
            "manage",
            "schema",
            "permits",
            "remove",
            "area:cli",
            "--dry-run",
            "--yes",
        ],
        "area:cli",
    );
    let marker = env.project.path().join(".project-id");
    disk::remove_file(&marker).unwrap();
    let heal = says_yes_was_ignored(
        &env,
        &["manage", "doctor", "heal", "--dry-run", "--yes"],
        ".project-id",
    );
    assert_eq!(heal["applied"], false);
    assert!(!disk::exists(&marker), "nothing was written");
    assert_eq!(permitted_keys(&env, "area"), ["cli"], "nothing was applied");
    assert!(
        permitted_keys(&env, "zz").is_empty(),
        "nothing was declared"
    );
    assert_eq!(
        ok(&env, &["tasks", "list", "--all"])
            .as_array()
            .map(Vec::len),
        Some(1),
        "nothing was pruned"
    );
}

#[test]
fn test_tasks_set_clear_trigger_returns_a_deferred_task_to_the_status_it_held() {
    // owner-accepted 2026-11-03: --clear-trigger survives.
    let env = Env::new(SESSION);
    let harp = add_task(&env, "Waits");
    ok(
        &env,
        &[
            "tasks",
            "set",
            &harp,
            "--status",
            "Deferred",
            "--trigger",
            "when it ships",
        ],
    );
    let cleared = ok(&env, &["tasks", "set", &harp, "--clear-trigger"]);
    assert_eq!(cleared["status"], "To Do");
    assert!(
        cleared["trigger"].is_null() || cleared["trigger"] == "",
        "{cleared}"
    );
}

#[test]
fn test_tasks_set_clear_trigger_with_trigger_is_a_usage_error() {
    // owner-accepted 2026-11-03.
    let env = Env::new(SESSION);
    let harp = add_task(&env, "Idle");
    let ran = refused(
        &env,
        &["tasks", "set", &harp, "--clear-trigger", "--trigger", "x"],
        2,
    );
    assert_eq!(ran.stdout, "");
}
