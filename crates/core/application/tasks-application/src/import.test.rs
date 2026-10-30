//! The import, over fake JSONL logs and angzarr-ports' in-memory store, run
//! through angzarr-embedded's kernel as the binary runs it over SQLite.

use crate::english::English;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use angzarr_embedded::next::Stream;
use angzarr_embedded_diagnostic::{Arg, Diagnostic, Report};
use ports::Timestamp;
use ports::{JsonlOp, JsonlRecord, JsonlTaskLogs};
use tasks::{TaskAggregate, TaskState};

use super::{anomaly_key, record_command_id, rfc3339_nano, Step};
use crate::fakes::{
    ready, FakeClock, FakeDelay, FakeIdentity, FakeNames, FakeStores, Fault, Flaky,
};
use crate::{
    cover, ImportReport, ImportRequest, ImportService, Importer, Lost, NewTask, ProjectImport,
    Service, TaskService, Why,
};

const PROJECT: &str = "jolly-mute-magma";
const HARP: &str = "swift-amber";

/// The logs by project id, read in the order given; failing on cue.
#[derive(Default)]
struct FakeLogs {
    logs: BTreeMap<String, Vec<JsonlRecord>>,
    projects_fail: Option<Report>,
    records_fail: Option<Report>,
    read: Vec<String>,
}

impl JsonlTaskLogs for FakeLogs {
    fn projects(&mut self) -> Result<Vec<String>, Report> {
        match &self.projects_fail {
            Some(report) => Err(report.clone()),
            None => Ok(self.logs.keys().cloned().collect()),
        }
    }

    fn records(&mut self, project_id: &str) -> Result<Option<Vec<JsonlRecord>>, Report> {
        self.read.push(project_id.to_string());
        if let Some(report) = &self.records_fail {
            return Err(report.clone());
        }
        Ok(self.logs.get(project_id).cloned())
    }
}

type Fake = Importer<FakeIdentity, FakeStores, FakeDelay, FakeLogs>;

fn service(records: Vec<JsonlRecord>) -> Fake {
    over(FakeStores::default(), records)
}

/// The importer of `records`, as the one project's log, into `stores`.
fn over(stores: FakeStores, records: Vec<JsonlRecord>) -> Fake {
    let mut logs = FakeLogs::default();
    logs.logs.insert(PROJECT.to_string(), records);
    Importer::new(FakeIdentity::default(), stores, FakeDelay::default(), logs)
}

fn import(service: &mut Fake) -> ImportReport {
    ready(service.import(ImportRequest::default())).unwrap()
}

/// The one project's part of `report`.
fn only(report: &ImportReport) -> &ProjectImport {
    assert_eq!(report.projects.len(), 1, "{report:?}");
    &report.projects[0]
}

fn at(seconds: i64) -> Timestamp {
    Timestamp {
        seconds: 1_770_000_000 + seconds,
        nanos: 0,
    }
}

/// The task `harp` as its stream holds it.
fn state(service: &mut Fake, harp: &str) -> Option<TaskState> {
    let slice = ready(service.stores.store.read_from(&cover(harp), 0)).unwrap()?;
    let stream =
        Stream::<TaskAggregate>::replay(cover(harp), &slice.pages, &crate::registry()).unwrap();
    Some(stream.state().state().clone())
}

/// How many events `harp`'s stream holds.
fn events(service: &mut Fake, harp: &str) -> usize {
    ready(service.stores.store.read_from(&cover(harp), 0))
        .unwrap()
        .map_or(0, |slice| slice.pages.len())
}

/// A record of `op` to `harp` at `second`, its line unlike any other's
/// (a clone repeats it).
fn record(op: JsonlOp, harp: &str, second: i64) -> JsonlRecord {
    let mut record = JsonlRecord {
        line: 0,
        raw: String::new(),
        op,
        task: harp.to_string(),
        text: String::new(),
        status: String::new(),
        trigger: String::new(),
        session: String::new(),
        repair_of: String::new(),
        tags: Vec::new(),
        ts: at(second),
    };
    static LINES: AtomicUsize = AtomicUsize::new(0);
    record.raw = format!("line {}", LINES.fetch_add(1, Ordering::Relaxed));
    record
}

fn add(harp: &str, text: &str, second: i64) -> JsonlRecord {
    JsonlRecord {
        text: text.to_string(),
        status: "To Do".to_string(),
        session: "calm-otter".to_string(),
        ..record(JsonlOp::Add, harp, second)
    }
}

fn status(harp: &str, status: &str, trigger: &str, second: i64) -> JsonlRecord {
    JsonlRecord {
        status: status.to_string(),
        trigger: trigger.to_string(),
        ..record(JsonlOp::Status, harp, second)
    }
}

fn tags(op: JsonlOp, harp: &str, tags: &[&str], second: i64) -> JsonlRecord {
    JsonlRecord {
        tags: tags.iter().map(|t| t.to_string()).collect(),
        ..record(op, harp, second)
    }
}

fn text(harp: &str, text: &str, second: i64) -> JsonlRecord {
    JsonlRecord {
        text: text.to_string(),
        ..record(JsonlOp::Text, harp, second)
    }
}

/// `records`, numbered as lines from 1 in the order given.
fn numbered(mut records: Vec<JsonlRecord>) -> Vec<JsonlRecord> {
    for (i, record) in records.iter_mut().enumerate() {
        record.line = u32::try_from(i + 1).unwrap();
    }
    records
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

// ------------------------------------------------------------ fidelity

#[test]
fn test_an_add_imports_the_task_with_its_harp_session_and_time() {
    let mut service = service(numbered(vec![JsonlRecord {
        status: "In Progress".to_string(),
        trigger: " later ".to_string(),
        tags: strings(&["b", " a", "b", ""]),
        ..add(HARP, " Write the tour ", 1)
    }]));
    let report = import(&mut service);
    let project = only(&report);
    assert_eq!(project.project_id, PROJECT);
    assert_eq!((project.records, project.tasks, project.applied), (1, 1, 1));
    let task = state(&mut service, HARP).expect("imported");
    assert_eq!(task.harp_id, HARP);
    assert_eq!(task.text, "Write the tour");
    assert_eq!(task.status, "In Progress");
    // Go keeps a trigger given with any status; here only a Deferred task
    // holds one (the human's rulings on L, 2026-10-27), so it is left out of
    // the add, and kept by the import for a later deferral that names none.
    assert_eq!(task.trigger, "", "held only while Deferred");
    assert_eq!(spelled(&task.tags), strings(&["a", "b"]));
    assert_eq!(task.origin_session, "calm-otter");
    assert_eq!(task.created_at, at(1), "Go's instant, not the clock's");
    assert_eq!(task.status_set_at, at(1));
    assert_eq!(task.deferred_since, None);
    assert!(!task.removed);
    assert_eq!(service.identity.pins, [Some(PROJECT.to_string())]);
    assert_eq!(service.stores.opened, [PROJECT]);
}

#[test]
fn test_an_add_with_no_status_is_to_do() {
    let mut service = service(vec![JsonlRecord {
        status: String::new(),
        ..add(HARP, "Ship it", 1)
    }]);
    import(&mut service);
    assert_eq!(state(&mut service, HARP).unwrap().status, "To Do");
}

#[test]
fn test_status_changes_keep_the_logged_instants() {
    let mut service = service(vec![
        add(HARP, "Revisit", 1),
        status(HARP, "Deferred", "after the release", 5),
        status(HARP, "In Progress", "", 9),
    ]);
    import(&mut service);
    let task = state(&mut service, HARP).unwrap();
    assert_eq!(task.status, "In Progress");
    // Reverses the reviewed "kept on leaving Deferred" (the human's ruling
    // on L, 2026-10-27): leaving Deferred clears the trigger.
    assert_eq!(task.trigger, "", "cleared on leaving Deferred");
    assert_eq!(task.status_set_at, at(9));
    assert_eq!(task.deferred_since, Some(at(5)));
    assert_eq!(task.created_at, at(1));
}

// Go's empty string is a value left out: an add's status, trigger and
// session, and a status change's trigger.
#[test]
fn test_gos_empty_values_are_read_as_left_out() {
    let mut service = service(numbered(vec![
        JsonlRecord {
            status: String::new(),
            trigger: String::new(),
            session: String::new(),
            ..add(HARP, "Revisit", 1)
        },
        status(HARP, "Deferred", "after the release", 2),
        status(HARP, "Deferred", "", 3),
    ]));
    let report = import(&mut service);
    assert_eq!(only(&report).lost, [], "nothing refused as blank");
    let task = state(&mut service, HARP).unwrap();
    assert_eq!(
        (task.status.as_str(), task.trigger.as_str()),
        ("Deferred", "after the release")
    );
    assert_eq!(task.origin_session, "");
    assert_eq!(task.prior_status, "To Do");
}

// Go keeps the trigger a task leaves Deferred with, and defers it again on
// that one when a status change names none; the import does the same by
// giving it, since here leaving Deferred clears it.
#[test]
fn test_a_go_task_deferred_again_without_a_trigger_defers_on_the_one_go_held() {
    let mut service = service(numbered(vec![
        add(HARP, "Revisit", 1),
        status(HARP, "Deferred", "after the release", 2),
        status(HARP, "In Progress", "", 3),
        status(HARP, "Deferred", "", 4),
    ]));
    let report = import(&mut service);
    assert_eq!(only(&report).lost, []);
    let task = state(&mut service, HARP).unwrap();
    assert_eq!(
        (task.status.as_str(), task.trigger.as_str()),
        ("Deferred", "after the release")
    );
    assert_eq!(task.prior_status, "In Progress");
    assert_eq!(task.deferred_since, Some(at(4)));
}

// Go records a trigger given with any status; here a trigger with another
// status is refused, so the import leaves it out of the command and keeps
// it for a later deferral that names none, as Go does.
#[test]
fn test_a_go_trigger_given_with_another_status_is_kept_for_a_later_deferral() {
    let mut service = service(numbered(vec![
        JsonlRecord {
            status: "In Progress".to_string(),
            trigger: "after the release".to_string(),
            ..add(HARP, "Revisit", 1)
        },
        status(HARP, "Done", "after v2", 2),
        status(HARP, "Deferred", "", 3),
    ]));
    let report = import(&mut service);
    assert_eq!(only(&report).lost, [], "no trigger refused with a status");
    let task = state(&mut service, HARP).unwrap();
    assert_eq!(
        (task.status.as_str(), task.trigger.as_str()),
        ("Deferred", "after v2")
    );
    assert_eq!(task.prior_status, "Done");
}

#[test]
fn test_an_add_deferred_records_when() {
    let mut service = service(vec![JsonlRecord {
        status: "Deferred".to_string(),
        trigger: "after the release".to_string(),
        ..add(HARP, "Revisit", 3)
    }]);
    import(&mut service);
    assert_eq!(
        state(&mut service, HARP).unwrap().deferred_since,
        Some(at(3))
    );
}

#[test]
fn test_edits_tags_untags_and_a_removal_are_imported() {
    let mut service = service(vec![
        add(HARP, "Draft", 1),
        text(HARP, "Final", 2),
        tags(JsonlOp::Tag, HARP, &["x", "y"], 3),
        tags(JsonlOp::Untag, HARP, &["x"], 4),
        add("calm-otter", "Gone", 5),
        record(JsonlOp::Remove, "calm-otter", 6),
    ]);
    let report = import(&mut service);
    assert_eq!(only(&report).applied, 6);
    assert_eq!(only(&report).tasks, 2);
    let task = state(&mut service, HARP).unwrap();
    assert_eq!(task.text, "Final");
    assert_eq!(spelled(&task.tags), strings(&["y"]));
    assert_eq!(
        task.status_set_at,
        at(1),
        "an edit or retag is no status change"
    );
    let gone = state(&mut service, "calm-otter").unwrap();
    assert!(gone.removed);
    assert_eq!(gone.text, "Gone");
}

#[test]
fn test_records_replay_in_time_order_and_the_log_breaks_ties() {
    // A union merge can put a later record first in the file; two records
    // at one instant apply in the file's order.
    let mut service = service(numbered(vec![
        status(HARP, "Done", "", 5),
        add(HARP, "Ship it", 1),
        status(HARP, "In Progress", "", 7),
        status(HARP, "Archived", "", 7),
    ]));
    let report = import(&mut service);
    assert!(only(&report).lost.is_empty(), "{report:?}");
    let task = state(&mut service, HARP).unwrap();
    assert_eq!(task.status, "Archived");
    assert_eq!(task.status_set_at, at(7));
}

#[test]
fn test_a_line_repeated_byte_for_byte_counts_once() {
    let first = tags(JsonlOp::Untag, HARP, &["x"], 4);
    let mut service = service(vec![
        add(HARP, "Ship it", 1),
        tags(JsonlOp::Tag, HARP, &["x"], 2),
        first.clone(),
        tags(JsonlOp::Tag, HARP, &["x"], 5),
        first,
    ]);
    let report = import(&mut service);
    let project = only(&report);
    assert_eq!(project.records, 5);
    assert_eq!(project.repeated, 1);
    assert_eq!(project.applied, 4);
    assert_eq!(
        spelled(&state(&mut service, HARP).unwrap().tags),
        strings(&["x"])
    );
}

#[test]
fn test_records_go_ignores_are_ignored() {
    let mut service = service(vec![
        status("never-added", "Done", "", 1),
        add(HARP, "Ship it", 2),
        record(JsonlOp::Remove, HARP, 3),
        text(HARP, "After removal", 4),
        record(JsonlOp::Remove, HARP, 5),
    ]);
    let report = import(&mut service);
    let project = only(&report);
    assert_eq!(project.ignored, 3);
    assert_eq!(project.applied, 2);
    assert!(project.lost.is_empty());
    assert_eq!(state(&mut service, "never-added"), None);
    assert_eq!(state(&mut service, HARP).unwrap().text, "Ship it");
}

#[test]
fn test_a_retag_that_changes_nothing_records_nothing() {
    let mut service = service(vec![
        JsonlRecord {
            tags: strings(&["x"]),
            ..add(HARP, "Ship it", 1)
        },
        tags(JsonlOp::Tag, HARP, &["x", " "], 2),
        tags(JsonlOp::Untag, HARP, &["absent"], 3),
        tags(JsonlOp::Tag, HARP, &[" "], 4),
    ]);
    let report = import(&mut service);
    assert_eq!(only(&report).unchanged, 3);
    assert_eq!(only(&report).applied, 1);
    assert_eq!(events(&mut service, HARP), 1);
}

// A logged tag that is not a tag in tagma's grammar is reported and left
// out; the rest of its record imports. The fold still counts it.
#[test]
fn test_a_logged_tag_outside_the_grammar_is_reported_and_the_rest_imported() {
    let mut service = service(numbered(vec![
        JsonlRecord {
            tags: strings(&["ok", "has space"]),
            ..add(HARP, "Ship it", 1)
        },
        tags(JsonlOp::Tag, HARP, &["a/b", "y"], 2),
        tags(JsonlOp::Tag, HARP, &["c/d"], 3),
        tags(JsonlOp::Untag, HARP, &["has space"], 4),
    ]));
    let report = import(&mut service);
    let project = only(&report);
    let lost: Vec<(u32, String)> = project
        .lost
        .iter()
        .map(|l| (l.line, l.why.to_string()))
        .collect();
    assert_eq!(lost.len(), 4, "{lost:?}");
    assert_eq!(
        lost.iter().map(|(line, _)| *line).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert!(lost[0].1.contains("has space"), "{lost:?}");
    assert!(project
        .lost
        .iter()
        .all(|l| l.why.code() == "tasks-application-tag-unparseable"));
    let named: Vec<&str> = project.lost[0]
        .why
        .args()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(named, ["tag", "ext_err_txt"], "the why is the refusal's");
    assert_eq!(project.applied, 2, "the add, and the tag of y");
    assert_eq!(project.unchanged, 0, "each record changed Go's tags");
    assert_eq!(
        spelled(&state(&mut service, HARP).unwrap().tags),
        ["ok", "y"]
    );
}

// The log keeps tags as strings, so it can hold one tag twice, spelled two
// ways; an untag of one spelling leaves the other, and the task still
// carries the tag. A real log has this (`sig:"a.b"` beside `sig:a.b`).
#[test]
fn test_a_task_keeps_a_tag_the_log_still_holds_under_another_spelling() {
    let mut service = service(numbered(vec![
        JsonlRecord {
            tags: strings(&["sig:\"a.b\""]),
            ..add(HARP, "Ship it", 1)
        },
        tags(JsonlOp::Tag, HARP, &["sig:a.b"], 2),
        tags(JsonlOp::Untag, HARP, &["sig:a.b"], 3),
        tags(JsonlOp::Untag, HARP, &["sig:\"a.b\""], 4),
    ]));
    let report = import(&mut service);
    let project = only(&report);
    assert!(project.lost.is_empty());
    assert_eq!(
        project.unchanged, 2,
        "one tag, spelled again, then one of two"
    );
    assert_eq!(project.applied, 2, "the add, and the last untag");
    assert!(state(&mut service, HARP).unwrap().tags.is_empty());
}

// ------------------------------------------------------------ anomalies

#[test]
fn test_a_displaced_duplicate_add_is_reported_and_its_records_go_to_the_first() {
    let mut service = service(numbered(vec![
        add(HARP, "First", 1),
        JsonlRecord {
            session: "other-session".to_string(),
            ..add(HARP, "Second", 2)
        },
        status(HARP, "Done", "", 3),
    ]));
    let report = import(&mut service);
    let project = only(&report);
    assert_eq!(
        project.lost,
        [Lost {
            line: 2,
            op: "add",
            harp_id: HARP.to_string(),
            text: "Second".to_string(),
            why: Why::Displaced,
        }]
    );
    assert_eq!(project.tasks, 1);
    let task = state(&mut service, HARP).unwrap();
    assert_eq!(
        (task.text.as_str(), task.status.as_str()),
        ("First", "Done")
    );
    assert!(!report.complete());
}

#[test]
fn test_a_repaired_duplicate_add_imports_under_its_fresh_harp() {
    let displaced = add(HARP, "Second", 2);
    let repair = JsonlRecord {
        repair_of: anomaly_key(&displaced),
        raw: "repair".to_string(),
        ..add("calm-otter", "Second", 2)
    };
    let mut service = service(vec![add(HARP, "First", 1), displaced, repair]);
    let report = import(&mut service);
    let project = only(&report);
    assert_eq!(project.repaired, 1);
    assert_eq!(project.tasks, 2);
    assert!(project.lost.is_empty());
    assert!(report.complete());
    let repaired = state(&mut service, "calm-otter").unwrap();
    assert_eq!(repaired.text, "Second");
    assert_eq!(repaired.created_at, at(2), "the displaced add's own time");
}

#[test]
fn test_a_repair_found_after_its_anomaly_still_counts() {
    // Every repair is collected before an anomaly is judged.
    let displaced = add(HARP, "Second", 5);
    let repair = JsonlRecord {
        repair_of: anomaly_key(&displaced),
        ..add("calm-otter", "Second", 9)
    };
    let mut service = service(vec![add(HARP, "First", 1), displaced, repair]);
    assert_eq!(only(&import(&mut service)).repaired, 1);
}

#[test]
fn test_a_record_to_a_task_that_was_not_imported_is_reported() {
    let mut service = service(numbered(vec![
        add(HARP, "   ", 1),
        status(HARP, "Done", "", 2),
        tags(JsonlOp::Tag, HARP, &["x"], 3),
        record(JsonlOp::Remove, HARP, 4),
        text(HARP, "after", 5),
    ]));
    let report = import(&mut service);
    let project = only(&report);
    let why: Vec<(u32, &Why)> = project.lost.iter().map(|l| (l.line, &l.why)).collect();
    let Why::Refused(refused) = why[0].1 else {
        panic!("refused: {why:?}");
    };
    assert_eq!(refused.code(), "tasks-text-required");
    assert_eq!(
        why[1..],
        [
            (2, &Why::TaskNotImported),
            (3, &Why::TaskNotImported),
            (4, &Why::TaskNotImported),
        ]
    );
    assert_eq!(project.ignored, 1, "after its removal, Go ignores it too");
    assert_eq!(project.tasks, 0);
    assert_eq!(state(&mut service, HARP), None);
}

#[test]
fn test_a_record_the_domain_refuses_is_reported_and_the_task_goes_on() {
    let mut service = service(numbered(vec![
        add(HARP, "Ship it", 1),
        status(HARP, "Deferred", "", 2),
        status(HARP, "@open", "", 3),
        text(HARP, "Shipped", 4),
    ]));
    let report = import(&mut service);
    let codes: Vec<(u32, &str)> = only(&report)
        .lost
        .iter()
        .map(|l| match &l.why {
            Why::Refused(r) => (l.line, r.code()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        codes,
        [(2, "tasks-trigger-required"), (3, "tasks-status-reserved")]
    );
    let task = state(&mut service, HARP).unwrap();
    assert_eq!(
        (task.status.as_str(), task.text.as_str()),
        ("To Do", "Shipped")
    );
}

#[test]
fn test_a_harp_a_task_here_already_holds_is_reported_and_left_alone() {
    let mut tasks = Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        FakeNames::of(&[HARP]),
        FakeClock::default(),
        FakeDelay::default(),
    );
    let native = NewTask {
        text: "Native".to_string(),
        ..NewTask::default()
    };
    ready(tasks.add(native)).unwrap();
    let records = vec![add(HARP, "From Go", 1), text(HARP, "Go edit", 2)];
    let mut service = over(tasks.stores, records);
    let report = import(&mut service);
    let lost: Vec<&Why> = only(&report).lost.iter().map(|l| &l.why).collect();
    assert_eq!(lost, [&Why::HarpTaken, &Why::TaskNotImported]);
    assert_eq!(state(&mut service, HARP).unwrap().text, "Native");
}

// ------------------------------------------------------------ re-running

#[test]
fn test_importing_again_duplicates_nothing() {
    let records = vec![
        add(HARP, "Ship it", 1),
        tags(JsonlOp::Tag, HARP, &["x"], 2),
        status(HARP, "Done", "", 3),
    ];
    let mut service = service(records);
    import(&mut service);
    let before = events(&mut service, HARP);
    let again = import(&mut service);
    let project = only(&again);
    assert_eq!((project.applied, project.tasks, project.already), (0, 0, 3));
    assert_eq!(events(&mut service, HARP), before);
    assert!(again.complete());
}

#[test]
fn test_an_import_that_stopped_part_way_resumes() {
    let records = vec![
        add(HARP, "Ship it", 1),
        status(HARP, "In Progress", "", 2),
        status(HARP, "Done", "", 3),
    ];
    let mut service = service(records);
    service.backoff.attempts = 1;
    // The add lands, but its answer is lost and no retry is allowed: the
    // import stops there.
    service.stores.store = Flaky::failing(&[Fault::LandsThenFails]);
    let first = import(&mut service);
    let stopped = only(&first);
    assert_eq!(
        stopped.failed.as_ref().map(|r| r.code()),
        Some("tasks-application-gave-up")
    );
    assert_eq!(stopped.applied, 0);
    assert!(!first.complete());
    let second = import(&mut service);
    let resumed = only(&second);
    assert_eq!((resumed.already, resumed.applied, resumed.tasks), (1, 2, 0));
    assert_eq!(state(&mut service, HARP).unwrap().status, "Done");
    assert_eq!(events(&mut service, HARP), 3);
}

#[test]
fn test_records_appended_since_are_imported_next_time() {
    let mut service = service(vec![add(HARP, "Ship it", 1)]);
    import(&mut service);
    let log = service.jsonl_logs.logs.get_mut(PROJECT).unwrap();
    log.push(status(HARP, "Done", "", 2));
    let report = import(&mut service);
    assert_eq!((only(&report).already, only(&report).applied), (1, 1));
    assert_eq!(state(&mut service, HARP).unwrap().status, "Done");
}

#[test]
fn test_a_record_imports_under_the_command_id_its_line_derives() {
    let add = add(HARP, "Ship it", 1);
    let id = record_command_id(&add.raw);
    let mut service = service(vec![add]);
    import(&mut service);
    let cover = cover(HARP);
    let stream = angzarr_embedded::next::StreamRef::try_from(&cover).unwrap();
    let id = angzarr_embedded::next::angzarr_ports::Uuid::try_from(&id).unwrap();
    let found = ready(angzarr_embedded::next::EventStore::find_command(
        &service.stores.store.inner,
        stream,
        id,
    ))
    .unwrap();
    assert_eq!(found.len(), 1);
}

#[test]
fn test_a_command_id_is_the_lines_and_only_its() {
    assert_eq!(record_command_id("a"), record_command_id("a"));
    assert_ne!(record_command_id("a"), record_command_id("b"));
    assert_ne!(
        record_command_id("a"),
        angzarr_embedded::next::ids::fact_command_id("a"),
        "prefixed, so no other fact's id"
    );
}

#[test]
fn test_a_command_id_keeps_the_prefix_imported_stores_already_hold() {
    assert_eq!(
        record_command_id("a"),
        angzarr_embedded::next::ids::fact_command_id("ctxloom-go-task-log:a"),
        "a store imported before the rename finds its records again"
    );
}

// ------------------------------------------------------------ projects

#[test]
fn test_every_project_with_a_log_is_imported_in_turn() {
    let mut service = service(vec![add(HARP, "One", 1)]);
    service.jsonl_logs.logs.insert(
        "able-calm-fox".to_string(),
        vec![add("calm-otter", "Two", 1)],
    );
    let report = import(&mut service);
    let ids: Vec<&str> = report
        .projects
        .iter()
        .map(|p| p.project_id.as_str())
        .collect();
    assert_eq!(ids, ["able-calm-fox", PROJECT]);
    assert_eq!(service.jsonl_logs.read, ["able-calm-fox", PROJECT]);
    assert_eq!(
        service.identity.pins,
        [Some("able-calm-fox".to_string()), Some(PROJECT.to_string())]
    );
    assert_eq!(state(&mut service, "calm-otter").unwrap().text, "Two");
}

#[test]
fn test_a_named_project_alone_is_imported() {
    let mut service = service(vec![add(HARP, "One", 1)]);
    service.jsonl_logs.logs.insert(
        "able-calm-fox".to_string(),
        vec![add("calm-otter", "Two", 1)],
    );
    let request = ImportRequest {
        project: Some(PROJECT.to_string()),
    };
    let report = ready(service.import(request)).unwrap();
    assert_eq!(only(&report).project_id, PROJECT);
    assert_eq!(service.jsonl_logs.read, [PROJECT]);
}

#[test]
fn test_a_named_project_without_a_log_fails() {
    let mut service = service(Vec::new());
    let request = ImportRequest {
        project: Some("tpyo".to_string()),
    };
    let report = ready(service.import(request)).unwrap();
    let failed = only(&report).failed.clone().expect("failed");
    assert_eq!(failed.code(), "tasks-application-no-jsonl-log");
    assert_eq!(
        failed.english(),
        "Go taskloom has no task log for project tpyo"
    );
    assert!(matches!(&failed.args()[..], [("project", Arg::Str(s))] if s == "tpyo"));
    assert!(!report.complete());
}

#[test]
fn test_an_empty_log_opens_no_store() {
    let mut service = service(Vec::new());
    let report = import(&mut service);
    assert_eq!(only(&report).records, 0);
    assert!(only(&report).failed.is_none());
    assert!(service.identity.pins.is_empty());
    assert!(service.stores.opened.is_empty());
    assert!(report.complete());
}

#[test]
fn test_logs_that_cannot_be_listed_fail_the_import() {
    let mut service = service(Vec::new());
    service.jsonl_logs.projects_fail = Some(Report::new("jsonl-task-log-unreadable"));
    let error = ready(service.import(ImportRequest::default())).unwrap_err();
    assert_eq!(error.code(), "jsonl-task-log-unreadable");
}

#[test]
fn test_a_log_that_cannot_be_read_fails_its_project() {
    let mut service = service(vec![add(HARP, "One", 1)]);
    service.jsonl_logs.records_fail = Some(Report::new("jsonl-task-log-malformed"));
    let report = import(&mut service);
    assert_eq!(
        only(&report).failed.as_ref().map(|r| r.code()),
        Some("jsonl-task-log-malformed")
    );
    assert!(service.stores.opened.is_empty());
}

#[test]
fn test_a_project_that_cannot_resolve_fails() {
    let mut service = service(vec![add(HARP, "One", 1)]);
    service.identity.fails = Some(Report::new("project-registry-invalid-id"));
    let report = import(&mut service);
    assert_eq!(
        only(&report).failed.as_ref().map(|r| r.code()),
        Some("project-registry-invalid-id")
    );
}

#[test]
fn test_a_store_that_does_not_open_fails_the_project() {
    let mut service = service(vec![add(HARP, "One", 1)]);
    service.stores.store.open_fails = true;
    let report = import(&mut service);
    assert_eq!(
        only(&report).failed.as_ref().map(|r| r.code()),
        Some("angzarr-ports-store-unavailable")
    );
}

#[test]
fn test_the_import_future_is_send() {
    fn is_send<T: Send>(_: &T) {}
    let mut service = service(Vec::new());
    is_send(&service.import(ImportRequest::default()));
}

// ------------------------------------------------------------ keys

#[test]
fn test_the_anomaly_key_is_gos() {
    // The anomaly key the log's repair wrote for the same event.
    let mut record = add(HARP, "Write the tour", 0);
    record.ts = Timestamp {
        seconds: 1_775_046_645,
        nanos: 123_456_000,
    };
    assert_eq!(anomaly_key(&record), "ac654bfdf930");
    record.text = " padded ".to_string();
    record.session = String::new();
    record.ts.nanos = 0;
    assert_eq!(anomaly_key(&record), "f44d6594623d");
}

#[test]
fn test_instants_are_written_as_gos_rfc3339_nano() {
    let cases = [
        (-62_135_596_800, 0, "0001-01-01T00:00:00Z"),
        (951_868_799, 5, "2000-02-29T23:59:59.000000005Z"),
        (-86_400, 100_000_000, "1969-12-31T00:00:00.1Z"),
        (0, 0, "1970-01-01T00:00:00Z"),
        (-2_203_977_600, 0, "1900-02-28T00:00:00Z"),
        (-2_203_891_200, 0, "1900-03-01T00:00:00Z"),
        (4_107_542_400, 0, "2100-03-01T00:00:00Z"),
        (1_709_164_800, 0, "2024-02-29T00:00:00Z"),
        (1_677_628_800, 0, "2023-03-01T00:00:00Z"),
        (1_704_067_199, 999_999_999, "2023-12-31T23:59:59.999999999Z"),
        (1_775_046_645, 123_456_000, "2026-04-01T12:30:45.123456Z"),
        (951_782_400, 0, "2000-02-29T00:00:00Z"),
        (951_868_800, 0, "2000-03-01T00:00:00Z"),
        (978_307_199, 0, "2000-12-31T23:59:59Z"),
    ];
    for (seconds, nanos, go) in cases {
        assert_eq!(rfc3339_nano(Timestamp { seconds, nanos }), go, "{seconds}");
    }
}

#[test]
fn test_a_step_is_a_command_or_why_not() {
    // The fold's verdicts, apart from any store.
    let records = vec![
        add(HARP, "Ship it", 1),
        tags(JsonlOp::Untag, HARP, &["x"], 2),
    ];
    let mut fold = super::Fold::new(&records);
    assert!(matches!(fold.step(&records[0]), Step::Run(_)));
    assert_eq!(fold.step(&records[1]), Step::Unchanged);
    assert_eq!(fold.step(&record(JsonlOp::Text, "other", 3)), Step::Ignored);
}

// ------------------------------------------------------------ reasons

#[test]
fn test_each_reason_renders_its_english_and_a_refusal_is_its_rejection() {
    let english: Vec<String> = [Why::Displaced, Why::HarpTaken, Why::TaskNotImported]
        .iter()
        .map(|w| w.english())
        .collect();
    assert_eq!(
        english,
        [
            "another task holds this harp in Go's log: run Go's `taskloom repair`, then import again",
            "a task not imported from this record already holds this harp",
            "its task was not imported",
        ]
    );
    let refused = Why::Refused(Report::new("tasks-status-reserved").arg("status", "@x"));
    assert_eq!(
        refused.english(),
        "the status @x is not allowed: '@' is reserved for status classes"
    );
    assert_eq!(refused.code(), "tasks-status-reserved");
    assert_eq!(refused.args().len(), 1);
    assert!(Why::Displaced.args().is_empty());
}

/// Each tag in its canonical spelling.
fn spelled(tags: &[tasks::Tag]) -> Vec<String> {
    tags.iter().map(ToString::to_string).collect()
}

// An import returns the dead letters written while it ran (the kernel
// writes them; here the test writes one to the log it is handed, as the
// tasks domain can be made to produce none), each once.
#[test]
fn test_an_import_returns_the_dead_letters_written_while_it_ran() {
    let mut service = service(vec![add("swift-amber", "Ship it", 1)]);
    angzarr_embedded::next::DeadLetterLog::write(&service.dead_letters, "DEAD LETTER: import");
    let report = import(&mut service);
    let line = "DEAD LETTER: import".to_string();
    assert_eq!(report.dead_letters, [crate::DeadLetter { line }]);
    assert!(import(&mut service).dead_letters.is_empty(), "each once");
}
