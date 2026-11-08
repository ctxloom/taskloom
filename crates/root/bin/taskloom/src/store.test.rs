//! Acceptance: `taskloom manage store export` and `fold-digest`, the real
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
    let ran = env.run(&["manage", "store", "export"]);
    assert!(ran.ok, "{}", ran.stderr);
    let records = lines(&ran.stdout);
    let header = &records[0]["header"];
    assert_eq!(header["format"], "angzarr-store-jsonl");
    assert_eq!(header["generation"], angzarr_store_transfer::GENERATION);
    assert_eq!(header["store"], "angzarr-store-sqlite");
    assert_eq!(header["ledger"], serde_json::json!([1, 2, 3, 4, 5, 6]));
    let pages: Vec<&Value> = records.iter().filter_map(|r| r.get("page")).collect();
    let added: Vec<&Value> = pages
        .iter()
        .map(|p| &p["page"]["event"])
        .filter(|payload| payload["@type"] == "/ctxloom.tasks.v1.TaskAdded")
        .collect();
    let mut texts: Vec<&str> = added.iter().filter_map(|p| p["text"].as_str()).collect();
    texts.sort_unstable();
    assert_eq!(texts, ["first", "second"], "each task's addition, expanded");
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
    let ran = env.run(&["manage", "store", "export", "--project", "jolly-mute-magma"]);
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
    let export = env.run(&["manage", "store", "export"]);
    assert!(export.ok, "{}", export.stderr);
    let exported = lines(&export.stdout);
    let streams: Vec<(Value, Value)> = exported.last().unwrap()["end"]["streams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["cover"].clone(), s["head"].clone()))
        .collect();
    let ran = env.run(&["manage", "store", "fold-digest"]);
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
    // taskloom's aggregates take no snapshot, so no line holds a state:
    // a finding against the plan's section 5.3.2.
    assert!(digest.iter().all(|d| d.get("state").is_none()));
    assert_eq!(
        env.run(&["manage", "store", "fold-digest"]).stdout,
        ran.stdout,
        "the same bytes again"
    );
}

/// The file is angzarr-store-transfer's format, by its own schema: every
/// line reads as one of its records (which refuse a key they do not know),
/// the first is the header and the last the end, and the counts agree.
#[test]
fn test_a_store_export_reads_back_through_the_transfers_own_records() {
    use angzarr_store_transfer::record::Record;
    let env = Env::new("inept-sixth-stick");
    seeded(&env);
    let ran = env.run(&["manage", "store", "export"]);
    assert!(ran.ok, "{}", ran.stderr);
    let records: Vec<Record> = ran
        .stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}")))
        .collect();
    let Some(Record::Header(header)) = records.first() else {
        panic!("a header first: {records:?}");
    };
    assert_eq!(header.format, angzarr_store_transfer::FORMAT);
    assert_eq!(header.generation, angzarr_store_transfer::GENERATION);
    let Some(Record::End(end)) = records.last() else {
        panic!("an end last: {records:?}");
    };
    assert_eq!(end.records.0 as usize, records.len() - 2);
    let pages = records
        .iter()
        .filter(|r| matches!(r, Record::Page(_)))
        .count() as u64;
    let counted: u64 = end.streams.iter().map(|s| s.pages.0).sum();
    assert_eq!(
        pages, counted,
        "the pages the end counts are the pages written"
    );
}
