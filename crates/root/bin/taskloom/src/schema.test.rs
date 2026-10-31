//! RED (red/self-hosted-schema, for the owner's review): the store's own
//! tag rules through the real binary (docs/plans/self-hosted-schema.md).
//! The rules are config tags on the store's schema stream; no file layer
//! (home, work root, TASKLOOM_CONFIG_TAG_SCHEMA, --config-set) is read for
//! them, and the old keys are refused, naming `taskloom schema
//! import-config`, which moves them into the store once.

use std::path::Path;

use serde_json::{json, Value};

use super::{disk, Env};

fn configure(dir: &Path, yaml: &str) {
    disk::create_dir_all(dir.join(".taskloom")).unwrap();
    disk::write(dir.join(".taskloom/config.yaml"), yaml).unwrap();
}

/// The `code` of the JSON error envelope on stderr.
fn code(stderr: &str) -> String {
    let envelope: Value = serde_json::from_str(stderr)
        .unwrap_or_else(|e| panic!("not an envelope ({e}): {stderr:?}"));
    envelope["code"].as_str().unwrap().to_string()
}

fn declare(env: &Env, add: &[&str]) {
    let mut args = vec!["schema", "declare", "--json"];
    for tag in add {
        args.extend(["--add", tag]);
    }
    let ran = env.run(&args);
    assert!(ran.ok, "declare: {}", ran.stderr);
}

fn exported(env: &Env) -> Vec<String> {
    let ran = env.run(&["schema", "export"]);
    assert!(ran.ok, "export: {}", ran.stderr);
    ran.stdout.lines().map(str::to_string).collect()
}

// ---------------------------------------------------------- the CLI

#[test]
fn test_schema_export_prints_the_stores_rules_in_tagma_syntax_sorted() {
    let env = Env::new("calm-otter-fox");
    env.seed("Ship it");
    declare(&env, &["tagma.keys-per-item:lang=1"]);
    let lines = exported(&env);
    let mut sorted = lines.clone();
    sorted.sort();
    assert_eq!(lines, sorted);
    for expected in [
        "tagma.keys-per-item:lang=1",
        "tagma.keys-per-item:area=1",
        r#"tagma.enum:"triage:kind"=defect"#,
        r#"tagma.range:"triage:level"=1..5"#,
    ] {
        assert!(
            lines.iter().any(|l| l == expected),
            "{expected} in {lines:?}"
        );
    }
    assert!(lines.iter().any(|l| l.starts_with("taskloom.priority_fn:")));
    assert!(!lines.iter().any(|l| l.starts_with("tagma.priority_fn:")));
}

// `schema` alone is `schema show`, and keeps the shape it had: the store
// is where it reads from now.
#[test]
fn test_schema_show_keeps_its_json_shape_reading_the_store() {
    let env = Env::new("calm-otter-fox");
    env.seed("Ship it");
    declare(
        &env,
        &[r#"tagma.enum:"area:x"=a"#, r#"tagma.enum:"area:x"=b"#],
    );
    let shown = env.run(&["schema", "show", "--json"]);
    assert!(shown.ok, "{}", shown.stderr);
    let doc = shown.json();
    assert_eq!(env.run(&["schema", "--json"]).json(), doc, "schema is show");
    let targets = doc["targets"].as_array().unwrap();
    let find = |name: &str| {
        targets
            .iter()
            .find(|t| t["target"] == name)
            .unwrap_or_else(|| panic!("{name} not in {doc}"))
    };
    assert_eq!(find("area:x")["enum"], json!(["a", "b"]));
    assert_eq!(find("triage:level")["scalar"], true);
    assert_eq!(find("triage:level")["range"], json!({"min": 1, "max": 5}));
    assert_eq!(find("triage:blocks-release")["type"], "semver");
}

#[test]
fn test_schema_check_reports_tagmas_findings_and_fails_on_any() {
    let env = Env::new("calm-otter-fox");
    env.seed("Ship it");
    let clean = env.run(&["schema", "check", "--json"]);
    assert!(clean.ok, "{}", clean.stderr);
    assert_eq!(clean.json(), json!([]));
    declare(&env, &[r#"tagma.permit:"*:secret"=false"#]);
    let ran = env.run(&["schema", "check", "--json"]);
    assert!(!ran.ok, "a finding fails check, as lint");
    assert_eq!(
        ran.json(),
        json!([{
            "code": "inert-false",
            "tag": r#"tagma.permit:"*:secret"=false"#,
            "args": [["facet", "permit"]],
            "origin": "existing",
        }])
    );
}

#[test]
fn test_schema_declare_answers_the_tags_after_and_whether_they_changed() {
    let env = Env::new("calm-otter-fox");
    env.seed("Ship it");
    let args = [
        "schema",
        "declare",
        "--add",
        "tagma.keys-per-item:lang=1",
        "--remove",
        "tagma.keys-per-item:repo=1",
        "--json",
    ];
    let first = env.run(&args);
    assert!(first.ok, "{}", first.stderr);
    assert_eq!(first.json()["changed"], true);
    let tags = first.json()["tags"].clone();
    assert!(tags
        .as_array()
        .unwrap()
        .contains(&json!("tagma.keys-per-item:lang=1")));
    assert!(!tags
        .as_array()
        .unwrap()
        .contains(&json!("tagma.keys-per-item:repo=1")));
    assert_eq!(env.run(&args).json()["changed"], false);
    let ran = env.run(&["schema", "declare", "--add", "area:cli", "--json"]);
    assert_eq!(code(&ran.stderr), "tasks-schema-not-config");
}

// ------------------------------------------------- the write path

#[test]
fn test_a_write_is_refused_on_an_introduced_finding_and_warned_on_an_existing() {
    let env = Env::new("calm-otter-fox");
    let ran = env.run(&[
        "add",
        "Ship it",
        "--json",
        "--tag",
        "area:cli",
        "--tag",
        "area:core",
    ]);
    assert!(!ran.ok);
    assert_eq!(code(&ran.stderr), "tasks-application-tag-rules");
    assert!(ran.stderr.contains("too-many-keys"), "{}", ran.stderr);

    let ran = env.run(&[
        "add", "Ship it", "--json", "--tag", "lang:en", "--tag", "lang:fr",
    ]);
    assert!(ran.ok, "{}", ran.stderr);
    let harp = ran.json()["harp_id"].as_str().unwrap().to_string();
    declare(&env, &["tagma.keys-per-item:lang=1"]);
    let ran = env.run(&["tag", &harp, "--add", "x", "--json"]);
    assert!(ran.ok, "an existing finding never refuses: {}", ran.stderr);
    let warnings = ran.json()["warnings"].clone();
    assert_eq!(warnings.as_array().map(Vec::len), Some(2), "{warnings}");
    assert_eq!(warnings[0]["code"], "too-many-keys");
    assert_eq!(warnings[0]["origin"], "existing");
    let ran = env.run(&["tag", &harp, "--add", "y"]);
    assert!(ran.ok);
    assert!(
        ran.stderr.contains("too-many-keys"),
        "warned on stderr: {:?}",
        ran.stderr
    );
}

#[test]
fn test_a_task_may_carry_no_taskloom_namespace_tag() {
    let env = Env::new("calm-otter-fox");
    let ran = env.run(&["add", "Ship it", "--json", "--tag", "taskloom:x"]);
    assert!(!ran.ok);
    assert_eq!(
        code(&ran.stderr),
        "tasks-application-tag-reserved-namespace"
    );
}

// ------------------------------------------ the file layers are gone

#[test]
fn test_no_environment_or_override_affects_tag_rules_and_each_is_refused() {
    let env = Env::new("calm-otter-fox");
    env.seed("Ship it");
    let narrow = [(
        "TASKLOOM_CONFIG_TAG_SCHEMA",
        r#"tagma.range:"triage:level"="1,2""#,
    )];
    let ran = env.run_with(
        &["add", "Ship it", "--json", "--tag", "triage:level=4"],
        &narrow,
    );
    assert_eq!(code(&ran.stderr), "tasks-config-key-moved");
    let ran = env.run(&[
        "add",
        "Ship it",
        "--json",
        "--config-set",
        r#"tag_schema=tagma.range:"triage:level"="1,2""#,
    ]);
    assert_eq!(code(&ran.stderr), "tasks-config-key-moved");
    assert!(
        env.run(&["add", "Ship it", "--json", "--tag", "triage:level=4"])
            .ok
    );
}

#[test]
fn test_a_config_file_setting_tag_schema_or_tag_permits_is_refused_naming_the_import() {
    let env = Env::new("calm-otter-fox");
    env.seed("Ship it");
    for (dir, yaml) in [
        (
            env.project.path(),
            "tag_schema:\n  - 'tagma.enum:\"area:x\"=\"a,b\"'\n",
        ),
        (
            env.project.path(),
            "tag_permits:\n  namespaces:\n    area: open\n",
        ),
        (
            env.home.path(),
            "tag_schema:\n  - 'tagma.enum:\"area:x\"=\"a,b\"'\n",
        ),
    ] {
        configure(dir, yaml);
        let ran = env.run(&["list", "--json"]);
        assert!(!ran.ok, "{yaml}");
        assert_eq!(code(&ran.stderr), "tasks-config-key-moved", "{yaml}");
        assert!(
            ran.stderr.contains("schema import-config"),
            "{}",
            ran.stderr
        );
        disk::remove_file(dir.join(".taskloom/config.yaml")).unwrap();
    }
}

// PROPOSAL (ruling 3 asks which): an explicit, one-shot import moves the
// work root's `tag_permits` (taskloom's own repo has one) and
// `tag_schema` into the store, then takes the keys out of the file, its
// other keys and comments kept.
#[test]
fn test_import_config_moves_the_work_roots_permits_into_the_store_once() {
    let env = Env::new("calm-otter-fox");
    env.seed("Ship it");
    configure(
        env.project.path(),
        "schema_version: 1\n# kept\ntag_permits:\n  namespaces:\n    area: [cli]\n    relates: open\n  tags: [urgent]\n",
    );
    let ran = env.run(&["schema", "import-config", "--json"]);
    assert!(ran.ok, "{}", ran.stderr);
    let lines = exported(&env);
    for expected in [
        r#"tagma.permit:"*:*"=false"#,
        r#"tagma.permit:"area:cli"=true"#,
        r#"tagma.permit:"relates:*"=true"#,
        "tagma.permit:urgent=true",
    ] {
        assert!(
            lines.iter().any(|l| l == expected),
            "{expected} in {lines:?}"
        );
    }
    let file = disk::read_to_string(env.project.path().join(".taskloom/config.yaml")).unwrap();
    assert_eq!(file, "schema_version: 1\n# kept\n");
    assert!(env.run(&["list", "--json"]).ok, "the key is gone");
    let again = env.run(&["schema", "import-config", "--json"]);
    assert!(
        again.ok,
        "nothing left to import is no error: {}",
        again.stderr
    );
    assert_eq!(again.json()["changed"], false);
}

// ------------------------------------------------ permit and revoke

#[test]
fn test_permit_and_revoke_edit_the_stores_schema_and_never_a_file() {
    let env = Env::new("calm-otter-fox");
    env.seed("Ship it");
    let ran = env.run(&["permit", "area:", "--json"]);
    assert!(ran.ok, "{}", ran.stderr);
    assert!(
        disk::read(env.project.path().join(".taskloom/config.yaml")).is_err(),
        "no config file written"
    );
    let lines = exported(&env);
    assert!(lines.iter().any(|l| l == r#"tagma.permit:"*:*"=false"#));
    assert!(lines.iter().any(|l| l == r#"tagma.permit:"area:*"=true"#));
    let ran = env.run(&["add", "Ship it", "--json", "--tag", "lang:en"]);
    assert_eq!(code(&ran.stderr), "tasks-application-tag-rules");
    assert!(ran.stderr.contains("not-permitted"), "{}", ran.stderr);
    let carrying = env.run(&["add", "Ship it", "--json", "--tag", "area:cli"]);
    assert!(carrying.ok, "{}", carrying.stderr);
    let harp = carrying.json()["harp_id"].as_str().unwrap().to_string();
    let ran = env.run(&["revoke", "area:", "--json"]);
    assert_eq!(code(&ran.stderr), "tasks-application-permit-in-use");
    assert!(ran.stderr.contains(&harp), "names the task: {}", ran.stderr);
}
