//! Acceptance: tags through the real binary, its configuration read from
//! a temporary home and project, never a real one.

use std::path::Path;

use super::{disk, Env};

fn configure(dir: &Path, yaml: &str) {
    disk::create_dir_all(dir.join(".taskloom")).unwrap();
    disk::write(dir.join(".taskloom/config.yaml"), yaml).unwrap();
}

/// The `code` of the JSON error envelope on stderr.
fn code(stderr: &str) -> String {
    let envelope: serde_json::Value = serde_json::from_str(stderr)
        .unwrap_or_else(|e| panic!("not an envelope ({e}): {stderr:?}"));
    envelope["code"].as_str().unwrap().to_string()
}

/// `add` declared on the store's tag rules, `remove` taken out.
fn declare(env: &Env, add: &[&str], remove: &[&str]) {
    let mut args = vec!["schema", "declare", "--json"];
    for tag in add {
        args.extend(["--add", tag]);
    }
    for tag in remove {
        args.extend(["--remove", tag]);
    }
    let ran = env.run(&args);
    assert!(ran.ok, "declare: {}", ran.stderr);
}

// Rewritten (red/self-hosted-schema): the rules no longer come from the
// layers, the last winning. They are the store's: the seeded ones until a
// person declares otherwise, and no file, variable or override changes
// them (each is refused, schema.test.rs).
#[test]
fn test_the_rules_are_the_stores_seeded_until_declared_otherwise() {
    let env = Env::new("calm-otter-fox");
    let add = |tag: &str| env.run(&["add", "Ship it", "--json", "--tag", tag]);

    // The seeded rules: triage:level is 1-5.
    let ran = add("triage:level=7");
    assert!(!ran.ok);
    assert_eq!(code(&ran.stderr), "tasks-application-tag-rules");
    assert!(ran.stderr.contains("out-of-range"), "{}", ran.stderr);

    // Widened in the store: the seeded range replaced, an enum declared.
    declare(
        &env,
        &[
            r#"tagma.range:"triage:level"=1..9"#,
            r#"tagma.enum:"area:x"=a"#,
            r#"tagma.enum:"area:x"=b"#,
        ],
        &[r#"tagma.range:"triage:level"=1..5"#],
    );
    assert!(add("triage:level=7").ok, "the store's range");
    let ran = add("area:x=c");
    assert!(ran.stderr.contains("not-in-enum"), "{}", ran.stderr);
    assert!(add("area:x=b").ok);
}

// A malformed config file still refuses the command, naming the file; a
// malformed rule is the store's, recorded and reported by `schema check`.
#[test]
fn test_a_malformed_configuration_refuses_the_command_and_says_where() {
    let env = Env::new("calm-otter-fox");
    env.seed("Ship it");
    declare(&env, &[r#"tagma.arty:"x:y"=1"#], &[]);
    let checked = env.run(&["schema", "check", "--json"]);
    assert!(!checked.ok);
    assert_eq!(checked.json()[0]["code"], "unknown-facet");
    configure(env.project.path(), "homeing: home\n");
    let ran = env.run(&["add", "Ship it"]);
    assert!(!ran.ok);
    // The envelope's message (stderr is JSON, a `\` in it escaped) names
    // the file, joined by the host's separator.
    let envelope: serde_json::Value = serde_json::from_str(&ran.stderr).expect("an envelope");
    let file = Path::new(".taskloom").join("config.yaml");
    assert!(
        envelope["message"]
            .as_str()
            .is_some_and(|message| message.contains(&file.display().to_string())),
        "{}",
        ran.stderr
    );
    configure(env.project.path(), "homing: home\n");
    let ran = env.run(&["add", "Ship it", "--json", "--config-set", "nonsense"]);
    assert_eq!(code(&ran.stderr), "tasks-config-not-an-override");
}

#[test]
fn test_a_scalar_tag_displaces_its_old_value() {
    let env = Env::new("calm-otter-fox");
    let task = env.seed("Ship it");
    let harp = task["harp_id"].as_str().unwrap();
    for level in ["triage:level=3", "triage:level=1"] {
        let ran = env.run(&["tag", harp, "--add", level, "--add", "x", "--json"]);
        assert!(ran.ok, "{}", ran.stderr);
    }
    let listed = env.list();
    assert_eq!(
        listed[0]["tags"],
        serde_json::json!(["triage:level=1", "x"])
    );
}

// Rewritten (red/self-hosted-schema): one write giving a scalar target two
// values keeps the last, as tagma's write rules have it (SPEC §8, the last
// value wins within a batch too), on add and on tag alike.
#[test]
fn test_two_values_for_a_scalar_target_in_one_write_keep_the_last() {
    let env = Env::new("calm-otter-fox");
    let two = ["--tag", "triage:level=3", "--tag", "triage:level=1"];
    let ran = env.run(&[&["add", "Ship it", "--json"][..], &two[..]].concat());
    assert!(ran.ok, "{}", ran.stderr);
    assert_eq!(ran.json()["tags"], serde_json::json!(["triage:level=1"]));
    let harp = ran.json()["harp_id"].as_str().unwrap().to_string();
    let both = ["--add", "triage:level=2", "--add", "triage:level=4"];
    let ran = env.run(&[&["tag", &harp, "--json"][..], &both[..]].concat());
    assert!(ran.ok, "{}", ran.stderr);
    let listed = env.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["tags"], serde_json::json!(["triage:level=4"]));
}

#[test]
fn test_list_selects_by_a_postfix_tag_query_and_refuses_infix() {
    let env = Env::new("calm-otter-fox");
    for (text, tag) in [
        ("one", "urgent"),
        ("two", "release"),
        ("three", "area:cli=x"),
    ] {
        let ran = env.run(&["add", text, "--tag", tag, "--json"]);
        assert!(ran.ok, "{}", ran.stderr);
    }
    let texts = |query: &str| -> Vec<String> {
        let ran = env.run(&["list", "--json", "--tag-query", query]);
        assert!(ran.ok, "{query}: {}", ran.stderr);
        ran.json()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["text"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(texts("urgent/release/or"), ["one", "two"]);
    assert_eq!(texts("urgent/not"), ["two", "three"]);
    assert_eq!(texts("*:cli=+"), ["three"]);
    for refused in ["urgent/and", "urgent or release"] {
        let ran = env.run(&["list", "--json", "--tag-query", refused]);
        assert!(!ran.ok, "{refused}");
        assert_eq!(code(&ran.stderr), "tasks-application-tag-query");
    }
}

// A hidden tag is left out of text and kept in JSON, through the binary.
#[test]
fn test_a_tag_the_schema_hides_is_left_out_of_text_and_kept_in_json() {
    let env = Env::new("calm-otter-fox");
    env.seed("First");
    declare(&env, &[r#"tagma.hide:"triage:cwe"=true"#], &[]);
    let ran = env.run(&[
        "add",
        "Ship it",
        "--tag",
        "triage:cwe=79",
        "--tag",
        "x",
        "--json",
    ]);
    assert!(ran.ok, "{}", ran.stderr);
    let harp = ran.json()["harp_id"].as_str().unwrap().to_string();
    let text = env.run(&["list", "--format", "text"]);
    assert!(text.stdout.contains("Ship it  [x]"), "{}", text.stdout);
    let tagged = env.run(&["tag", &harp, "--add", "y", "--format", "text"]);
    assert_eq!(tagged.stdout, format!("{harp}\tTo Do\tx,y\n"));
    assert_eq!(
        env.list()[1]["tags"],
        serde_json::json!(["triage:cwe=79", "x", "y"])
    );
}

// `taskloom schema` reads the store's rules the writes are held to: the
// seeded targets, and what the store declares.
#[test]
fn test_schema_prints_the_stores_tag_rules() {
    let env = Env::new("calm-otter-fox");
    env.seed("Ship it");
    declare(
        &env,
        &[r#"tagma.enum:"area:x"=a"#, r#"tagma.enum:"area:x"=b"#],
        &[],
    );
    let ran = env.run(&["schema", "--json"]);
    assert!(ran.ok, "{}", ran.stderr);
    let doc: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    let targets = doc["targets"].as_array().unwrap();
    let find = |name: &str| {
        targets
            .iter()
            .find(|t| t["target"] == name)
            .unwrap_or_else(|| panic!("{name} not in {doc}"))
    };
    assert_eq!(find("area:x")["enum"], serde_json::json!(["a", "b"]));
    assert_eq!(find("triage:level")["scalar"], true);
    assert_eq!(
        find("triage:level")["range"],
        serde_json::json!({"min": 1, "max": 5})
    );
    assert_eq!(find("triage:blocks-release")["type"], "semver");
}
