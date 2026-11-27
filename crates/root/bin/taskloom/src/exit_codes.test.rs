//! Acceptance: the exit codes the binary publishes, end to end through the
//! built binary, by the exit-code convention (rust-support's
//! `docs/exit-codes.md`): every success exits 0, or 16 under
//! `--detailed-exitcode`; a failure (the command tried and something broke)
//! 32; a decline (the request or the state forbids it, a usage error among
//! them) 48; a check that examined the store and found it unworkable 49.

use super::{project_with_store, Env, Jq, Ran, OLD_GENERATION};

const SESSION: &str = "exit-codes-harp";

fn exits(ran: &Ran, code: i32, what: &str) {
    assert_eq!(ran.code, code, "{what}: {}{}", ran.stdout, ran.stderr);
}

#[test]
fn test_a_usage_error_exits_48_not_clap_s_2() {
    let env = Env::new(SESSION);
    for args in [
        &["tasks", "add"][..],
        &["tasks", "no-such-verb"],
        &["--no-such-flag", "tasks", "list"],
        &["tasks", "add", "x", "--format", "json", "--no-such-flag"],
    ] {
        exits(&env.run(args), 48, &format!("{args:?}"));
    }
}

#[test]
fn test_a_removed_spelling_exits_48() {
    let env = Env::new(SESSION);
    exits(&env.run(&["add", "x"]), 48, "a removed verb");
}

#[test]
fn test_a_success_exits_0_without_the_flag_and_16_with_it_wherever_it_is_put() {
    let env = Env::new(SESSION);
    env.seed("first");
    exits(&env.run(&["tasks", "list"]), 0, "plain");
    for args in [
        &["--detailed-exitcode", "tasks", "list"][..],
        &["tasks", "--detailed-exitcode", "list"],
        &["tasks", "list", "--detailed-exitcode"],
    ] {
        exits(&env.run(args), 16, &format!("{args:?}"));
    }
}

#[test]
fn test_help_and_the_version_are_successes_exit_0_and_16_under_the_flag() {
    let env = Env::new(SESSION);
    for args in [&["--help"][..], &["--version"], &["tasks", "add", "--help"]] {
        exits(&env.run(args), 0, &format!("{args:?}"));
        let mut flagged = args.to_vec();
        flagged.push("--detailed-exitcode");
        exits(&env.run(&flagged), 16, &format!("{flagged:?}"));
    }
}

#[test]
fn test_a_group_given_no_command_prints_its_help_and_is_a_success() {
    let env = Env::new(SESSION);
    exits(&env.run(&["tasks"]), 0, "a bare group");
}

#[test]
fn test_a_request_the_state_forbids_exits_48_under_either_flag_setting() {
    let env = Env::new(SESSION);
    env.seed("first");
    for flag in [&[][..], &["--detailed-exitcode"]] {
        let mut args = vec!["tasks", "show", "no-such-harp"];
        args.extend_from_slice(flag);
        exits(&env.run(&args), 48, "a task that is not there");
    }
}

#[test]
fn test_a_store_that_cannot_be_used_is_a_failure_exiting_32() {
    let env = Env::new(SESSION);
    project_with_store(&env, OLD_GENERATION);
    exits(&env.run(&["tasks", "list"]), 32, "an archived store");
    exits(
        &env.run(&["tasks", "list", "--detailed-exitcode"]),
        32,
        "a failure is not collapsed or moved by the flag",
    );
}

#[test]
fn test_a_check_that_found_violations_exits_49_fatal_findings() {
    let jq = Jq::new(SESSION);
    let harp = jq.harp.as_str();
    jq.ok(&["tasks", "tag", "add", harp, "area:x=c", "--format", "json"]);
    jq.ok(&[
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
    exits(&jq.env.run(&["tasks", "check"]), 49, "a violation");
    exits(
        &jq.env.run(&["tasks", "check", "--detailed-exitcode"]),
        49,
        "a violation under the flag",
    );
}

#[test]
fn test_a_check_that_found_nothing_is_a_success() {
    let jq = Jq::new(SESSION);
    exits(&jq.env.run(&["tasks", "check"]), 0, "clean");
    exits(
        &jq.env.run(&["tasks", "check", "--detailed-exitcode"]),
        16,
        "clean",
    );
}

#[test]
fn test_plans_check_with_findings_exits_49_and_clean_exits_0() {
    let env = Env::new(SESSION);
    env.seed("first");
    exits(&env.run(&["plans", "check"]), 0, "no plans, no findings");
    let plans = env.project.path().join("docs/design");
    super::disk::create_dir_all(&plans).unwrap();
    super::disk::write(plans.join("broken.plan.md"), "---\nid: [unclosed\n").unwrap();
    exits(
        &env.run(&["plans", "check"]),
        49,
        "a plan file with bad frontmatter",
    );
    exits(
        &env.run(&["plans", "check", "--detailed-exitcode"]),
        49,
        "findings under the flag",
    );
}

// ------------------------------------------------------------ previews

/// A preview exits 17 under `--detailed-exitcode` when applying it would
/// change something and 16 when it would change nothing; without the flag
/// both exit 0. Applying it is a plain success.
fn previews(env: &Env, args: &[&str], with_work: bool) {
    let (flagged, plain) = if with_work { (17, 0) } else { (16, 0) };
    let mut detailed = args.to_vec();
    detailed.push("--detailed-exitcode");
    exits(&env.run(&detailed), flagged, &format!("{args:?} flagged"));
    exits(&env.run(args), plain, &format!("{args:?} plain"));
}

#[test]
fn test_a_schema_declaration_preview_that_would_change_exits_17_and_one_that_would_not_16() {
    let env = Env::new(SESSION);
    env.permit_every_tag();
    let rule = r#"tagma.enum:"area:x"=a"#;
    previews(&env, &["manage", "schema", "declare", "--add", rule], true);
    previews(
        &env,
        &[
            "manage",
            "schema",
            "declare",
            "--add",
            rule,
            "--yes",
            "--dry-run",
        ],
        true,
    );
    exits(
        &env.run(&["manage", "schema", "declare", "--add", rule, "--yes"]),
        0,
        "applied",
    );
    exits(
        &env.run(&[
            "manage",
            "schema",
            "declare",
            "--add",
            rule,
            "--yes",
            "--detailed-exitcode",
        ]),
        16,
        "applied under the flag",
    );
    previews(&env, &["manage", "schema", "declare", "--add", rule], false);
}

#[test]
fn test_a_permit_removal_preview_exits_17_and_its_application_16() {
    let env = Env::new(SESSION);
    env.permit_every_tag();
    exits(
        &env.run(&["manage", "schema", "permits", "add", "area:cli"]),
        0,
        "permit",
    );
    let remove = ["manage", "schema", "permits", "remove", "area:cli"];
    previews(&env, &remove, true);
    let mut applied = remove.to_vec();
    applied.extend(["--yes", "--detailed-exitcode"]);
    exits(&env.run(&applied), 16, "applied");
}

#[cfg(feature = "purge")]
#[test]
fn test_a_prune_preview_exits_17_when_it_would_take_a_task_and_16_when_it_would_take_none() {
    let env = Env::new(SESSION);
    env.seed("open work");
    let prune = ["tasks", "prune", "--older-than", "1ns"];
    previews(&env, &prune, false);
    exits(
        &env.run(&["tasks", "add", "Finished", "--status", "Done"]),
        0,
        "a Done task",
    );
    previews(&env, &prune, true);
    let mut applied = prune.to_vec();
    applied.extend(["--yes", "--detailed-exitcode"]);
    exits(&env.run(&applied), 16, "applied");
    previews(&env, &prune, false);
}
