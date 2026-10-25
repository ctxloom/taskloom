//! Records read from a directory of logs on a filesystem of the test's own
//! (`Rooted::temp`), its home `/home/u`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use annals::next::Timestamp;
use annals_diagnostic::Diagnostic;
use file_system::{Fs, Mode, Perms, Rooted};
use ports::{JsonlOp, JsonlRecord, JsonlTaskLogs};

use crate::{parse, JsonlTaskLogDir, LogError, MISSING_TS_INSTANT};

const ADD: &str = r#"{"op":"add","task":"swift-amber","text":"Write the tour","status":"To Do","trigger":"t","session":"calm-otter","repair_of":"abc","tags":["b","a"],"ts":"2026-04-01T12:30:45.123456789Z"}"#;

/// A home on a filesystem of its own.
struct Home {
    fs: Arc<dyn Fs>,
}

impl Home {
    fn new() -> Home {
        Home {
            fs: Arc::new(Rooted::temp().unwrap()),
        }
    }

    fn path(&self) -> PathBuf {
        PathBuf::from("/home/u")
    }

    fn fs(&self) -> Arc<dyn Fs> {
        Arc::clone(&self.fs)
    }

    fn logs(&self) -> JsonlTaskLogDir {
        JsonlTaskLogDir::in_home(self.fs(), Some(&self.path()))
    }

    fn mkdir(&self, path: &Path) {
        self.fs.create_dir_all(path, Mode::DIR).unwrap();
    }

    fn write(&self, path: &Path, bytes: &[u8]) {
        self.fs
            .write_atomic(path, bytes, Perms::Exact(Mode::FILE))
            .unwrap();
    }
}

/// A home whose log directory holds `files`, and its logs.
fn logs(files: &[(&str, &str)]) -> (Home, JsonlTaskLogDir) {
    let home = Home::new();
    let dir = home.path().join(".ctxloom").join("tasks");
    home.mkdir(&dir);
    for (name, text) in files {
        home.write(&dir.join(name), text.as_bytes());
    }
    let logs = home.logs();
    (home, logs)
}

fn only(text: &str) -> JsonlRecord {
    let mut records = parse("p", text).unwrap();
    assert_eq!(records.len(), 1);
    records.remove(0)
}

#[test]
fn test_a_record_carries_every_field_the_log_writes() {
    assert_eq!(
        only(ADD),
        JsonlRecord {
            line: 1,
            raw: ADD.to_string(),
            op: JsonlOp::Add,
            task: "swift-amber".to_string(),
            text: "Write the tour".to_string(),
            status: "To Do".to_string(),
            trigger: "t".to_string(),
            session: "calm-otter".to_string(),
            repair_of: "abc".to_string(),
            tags: vec!["b".to_string(), "a".to_string()],
            ts: Timestamp {
                seconds: 1_775_046_645,
                nanos: 123_456_789,
            },
        }
    );
}

#[test]
fn test_absent_and_null_fields_are_empty_and_unknown_ones_ignored() {
    let record = only(r#"{"op":"status","task":"swift-amber","text":null,"tags":null,"extra":7}"#);
    assert_eq!(record.text, "");
    assert_eq!(record.status, "");
    assert!(record.tags.is_empty());
    assert_eq!(
        record.ts,
        Timestamp {
            seconds: MISSING_TS_INSTANT,
            nanos: 0
        },
        "no ts is Go's zero time"
    );
}

#[test]
fn test_missing_ts_instant_is_year_one() {
    let at = time::OffsetDateTime::from_unix_timestamp(MISSING_TS_INSTANT).unwrap();
    assert_eq!((at.year(), u8::from(at.month()), at.day()), (1, 1, 1));
}

#[test]
fn test_an_offset_instant_is_the_same_instant_in_utc() {
    let record = only(r#"{"op":"remove","task":"x","ts":"2026-04-01T14:30:45+02:00"}"#);
    assert_eq!(
        record.ts,
        Timestamp {
            seconds: 1_775_046_645,
            nanos: 0
        }
    );
}

#[test]
fn test_every_op_the_log_knows_is_read() {
    let ops: Vec<JsonlOp> = ["add", "status", "text", "remove", "tag", "untag"]
        .iter()
        .map(|op| only(&format!(r#"{{"op":"{op}","task":"x"}}"#)).op)
        .collect();
    assert_eq!(
        ops,
        [
            JsonlOp::Add,
            JsonlOp::Status,
            JsonlOp::Text,
            JsonlOp::Remove,
            JsonlOp::Tag,
            JsonlOp::Untag
        ]
    );
}

#[test]
fn test_lines_are_trimmed_numbered_from_one_and_blank_ones_skipped() {
    let text = format!(
        "\n  {ADD}  \n\n{}\n",
        r#"{"op":"remove","task":"swift-amber"}"#
    );
    let records = parse("p", &text).unwrap();
    let lines: Vec<(u32, &str)> = records.iter().map(|r| (r.line, r.raw.as_str())).collect();
    assert_eq!(
        lines,
        [(2, ADD), (4, r#"{"op":"remove","task":"swift-amber"}"#)]
    );
}

#[test]
fn test_a_malformed_line_refuses_the_log_naming_where() {
    let text = format!("{ADD}\n{{\"op\":\"add\"\n");
    let Err(LogError::Malformed { path, line, .. }) = parse("/l.jsonl", &text) else {
        panic!("refused as malformed");
    };
    assert_eq!((path.as_str(), line), ("/l.jsonl", 2));
}

#[test]
fn test_an_unparseable_time_refuses_the_log() {
    assert!(matches!(
        parse("p", r#"{"op":"add","task":"x","ts":"yesterday"}"#),
        Err(LogError::Malformed { line: 1, .. })
    ));
}

#[test]
fn test_a_record_without_an_op_refuses_the_log() {
    assert_eq!(
        parse("p", r#"{"task":"x"}"#),
        Err(LogError::NoOp {
            path: "p".to_string(),
            line: 1
        })
    );
    assert_eq!(
        parse("p", r#"{"op":"","task":"x"}"#),
        Err(LogError::NoOp {
            path: "p".to_string(),
            line: 1
        })
    );
}

#[test]
fn test_an_op_the_log_does_not_know_refuses_the_log() {
    assert_eq!(
        parse("p", r#"{"op":"archive","task":"x"}"#),
        Err(LogError::UnknownOp {
            path: "p".to_string(),
            line: 1,
            op: "archive".to_string()
        })
    );
}

#[test]
fn test_projects_are_the_logs_in_the_directory_sorted() {
    let (home, mut logs) = logs(&[
        ("zany-bold-crab.jsonl", ""),
        ("able-calm-fox.jsonl", ADD),
        ("able.jsonl", ""),
        ("able-calm-fox.jsonl.lock", ""),
        ("able-calm-fox.jsonl.premove.bak", ""),
    ]);
    home.mkdir(&home.path().join(".ctxloom/tasks/dir.jsonl"));
    home.fs
        .symlink_dir(
            &home.path().join(".ctxloom/tasks"),
            &home.path().join(".ctxloom/tasks/linked.jsonl"),
        )
        .unwrap();
    assert_eq!(
        logs.projects().unwrap(),
        ["able", "able-calm-fox", "zany-bold-crab"]
    );
}

#[test]
fn test_no_directory_is_no_projects() {
    let home = Home::new();
    assert_eq!(home.logs().projects().unwrap(), Vec::<String>::new());
}

#[test]
fn test_an_unreadable_directory_is_an_error() {
    let home = Home::new();
    home.mkdir(&home.path().join(".ctxloom"));
    home.write(&home.path().join(".ctxloom/tasks"), b"not a directory");
    let error = home.logs().projects().unwrap_err();
    assert_eq!(error.code(), "jsonl-task-log-unreadable");
}

#[test]
fn test_records_reads_the_projects_log() {
    let (_home, mut logs) = logs(&[("able-calm-fox.jsonl", ADD)]);
    let records = logs.records("able-calm-fox").unwrap().expect("a log");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].task, "swift-amber");
}

#[test]
fn test_a_project_without_a_log_has_no_records() {
    let (_home, mut logs) = logs(&[]);
    assert_eq!(logs.records("able-calm-fox").unwrap(), None);
}

#[test]
fn test_a_log_that_cannot_be_read_is_an_error() {
    let (home, mut logs) = logs(&[]);
    home.mkdir(&home.path().join(".ctxloom/tasks/able-calm-fox.jsonl"));
    let error = logs.records("able-calm-fox").unwrap_err();
    assert_eq!(error.code(), "jsonl-task-log-unreadable");
}

#[test]
fn test_a_bad_log_reports_its_path() {
    let (home, mut logs) = logs(&[("able-calm-fox.jsonl", "{")]);
    let error = logs.records("able-calm-fox").unwrap_err();
    assert_eq!(error.code(), "jsonl-task-log-malformed");
    let path = home.path().join(".ctxloom/tasks/able-calm-fox.jsonl");
    assert!(
        error
            .to_string()
            .starts_with(&format!("{}:1:", path.display())),
        "{error}"
    );
}

#[test]
fn test_bytes_that_are_not_utf8_are_replaced_as_go_replaces_them() {
    let (home, mut logs) = logs(&[]);
    let mut line = br#"{"op":"add","task":"x","text":"caf"#.to_vec();
    line.extend_from_slice(&[0xff]);
    line.extend_from_slice(br#""}"#);
    home.write(
        &home.path().join(".ctxloom/tasks/able-calm-fox.jsonl"),
        &line,
    );
    let records = logs.records("able-calm-fox").unwrap().unwrap();
    assert_eq!(records[0].text, "caf\u{fffd}");
}

#[test]
fn test_without_a_home_every_read_says_so() {
    for home in [None, Some(Path::new(""))] {
        let mut logs = JsonlTaskLogDir::in_home(Home::new().fs(), home);
        assert_eq!(
            logs.projects().unwrap_err().code(),
            "jsonl-task-log-no-home"
        );
        assert_eq!(
            logs.records("able-calm-fox").unwrap_err().code(),
            "jsonl-task-log-no-home"
        );
    }
}

#[test]
fn test_a_directory_named_outright_is_read() {
    let (home, _) = logs(&[("able-calm-fox.jsonl", ADD)]);
    let mut logs = JsonlTaskLogDir::new(home.fs(), home.path().join(".ctxloom/tasks"));
    assert_eq!(logs.projects().unwrap(), ["able-calm-fox"]);
}
