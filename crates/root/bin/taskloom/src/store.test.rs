//! Acceptance: `taskloom store export` and `store fold-digest`, the real
//! binary in a temporary home (store-export-import.plan.md, D7 (a)): the
//! project's database written whole as JSONL and left as it was, and its
//! streams' digest, one line each, the same every time.

use std::path::PathBuf;

use serde_json::Value;

use super::{disk, Env};

/// A project of two tasks, added through the binary: its database.
fn seeded(env: &Env) -> PathBuf {
    env.seed("first");
    env.seed("second");
    let stores = env.stores();
    assert_eq!(stores.len(), 1, "one project");
    stores[0].clone()
}

fn lines(text: &str) -> Vec<Value> {
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}")))
        .collect()
}

#[test]
fn test_store_export_writes_the_projects_database_as_jsonl_and_leaves_it_as_it_was() {
    let env = Env::new("inept-sixth-stick");
    let database = seeded(&env);
    let before = disk::read(&database).unwrap();
    let ran = env.run(&["store", "export"]);
    assert!(ran.ok, "{}", ran.stderr);
    let records = lines(&ran.stdout);
    let header = &records[0]["header"];
    assert_eq!(header["format"], "angzarr-store-jsonl");
    assert_eq!(header["generation"], "20261019");
    assert_eq!(header["store"], "angzarr-store-sqlite");
    assert_eq!(header["ledger"], serde_json::json!([1, 2, 3, 4, 5, 6]));
    let pages: Vec<&Value> = records.iter().filter_map(|r| r.get("page")).collect();
    let added: Vec<&Value> = pages
        .iter()
        .map(|p| &p["page"]["payload"])
        .filter(|payload| payload["@type"] == "/ctxloom.tasks.v1.TaskAdded")
        .collect();
    assert_eq!(added.len(), 2, "each task's addition, expanded");
    assert_eq!(added[0]["text"].as_str().map(str::len), Some(5));
    let end = &records.last().unwrap()["end"];
    let between = records.len() - 2;
    assert_eq!(end["records"], between.to_string());
    let streams = end["streams"].as_array().unwrap().len();
    assert_eq!(
        ran.stderr,
        format!("taskloom: exported {between} records over {streams} streams\n")
    );
    assert_eq!(
        disk::read(&database).unwrap(),
        before,
        "the database as it was"
    );
}

#[test]
fn test_store_export_of_a_project_with_no_database_is_refused() {
    let env = Env::new("inept-sixth-stick");
    let ran = env.run(&["store", "export", "--project", "jolly-mute-magma"]);
    assert!(!ran.ok);
    assert_eq!(ran.stdout, "");
    assert_eq!(
        ran.stderr,
        "taskloom: project jolly-mute-magma has no database to export\n"
    );
}

#[test]
fn test_store_fold_digest_is_a_line_per_stream_and_the_same_twice() {
    let env = Env::new("inept-sixth-stick");
    seeded(&env);
    let export = env.run(&["store", "export"]);
    assert!(export.ok, "{}", export.stderr);
    let exported = lines(&export.stdout);
    let streams: Vec<(Value, Value)> = exported.last().unwrap()["end"]["streams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["cover"].clone(), s["head"].clone()))
        .collect();
    let ran = env.run(&["store", "fold-digest"]);
    assert!(ran.ok, "{}", ran.stderr);
    let digest = lines(&ran.stdout);
    let folded: Vec<(Value, Value)> = digest
        .iter()
        .map(|d| (d["cover"].clone(), d["head"].clone()))
        .collect();
    assert_eq!(
        folded, streams,
        "a line per stream the export holds, at its head, in its order"
    );
    let tasks = digest
        .iter()
        .filter(|d| d["state"]["@type"] == "/ctxloom.tasks.v1.TaskState")
        .count();
    assert_eq!(tasks, 2, "each task folded");
    assert_eq!(
        env.run(&["store", "fold-digest"]).stdout,
        ran.stdout,
        "the same bytes again"
    );
}
