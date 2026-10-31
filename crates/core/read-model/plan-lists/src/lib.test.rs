//! RED: the plan list, folded from plan streams.

use plans::{AddPlan, PlanEvent, PlanStatus};

use super::*;

const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn added(id: &str, title: &str) -> PlanEvent {
    PlanEvent::PlanAdded(AddPlan {
        harp_id: id.to_string(),
        title: title.to_string(),
        status: PlanStatus::Draft,
        path: format!(".taskloom/plans/{id}.plan.md"),
        content_hash: HASH.to_string(),
        origin_session: String::new(),
    })
}

fn ids(rows: Vec<&PlanRow>) -> Vec<&str> {
    rows.into_iter().map(|r| r.harp_id.as_str()).collect()
}

/// Three plans: `b` governs two tasks, `a` one of them; `c` supersedes `a`.
fn three() -> PlanList {
    let mut list = PlanList::new();
    list.fold("b", &added("b", "Bravo"));
    list.fold("a", &added("a", "Alpha"));
    list.fold("c", &added("c", "Charlie"));
    list.fold("b", &PlanEvent::Governed("task-one".into()));
    list.fold("b", &PlanEvent::Governed("task-two".into()));
    list.fold("a", &PlanEvent::Governed("task-one".into()));
    list.fold("c", &PlanEvent::Superseded("a".into()));
    list.fold(
        "a",
        &PlanEvent::MarkedSuperseded {
            by: "c".into(),
            from: PlanStatus::Draft,
        },
    );
    list
}

#[test]
fn list_holds_every_plan_sorted_by_harp_id_not_by_arrival() {
    assert_eq!(ids(three().plans()), vec!["a", "b", "c"]);
}

#[test]
fn a_row_carries_what_its_stream_recorded() {
    let list = three();
    let b = list.get("b").unwrap();
    assert_eq!(b.title, "Bravo");
    assert_eq!(b.status, PlanStatus::Draft);
    assert_eq!(b.path, ".taskloom/plans/b.plan.md");
    assert_eq!(b.content_hash, HASH);
    assert_eq!(b.governs, vec!["task-one", "task-two"]);
    let a = list.get("a").unwrap();
    assert_eq!(a.status, PlanStatus::Superseded);
    assert_eq!(a.superseded_by, "c");
    assert_eq!(list.get("c").unwrap().supersedes, vec!["a"]);
    assert!(list.get("zz").is_none());
}

#[test]
fn streams_folded_in_any_interleaving_give_the_same_list() {
    let mut other = PlanList::new();
    other.fold("c", &added("c", "Charlie"));
    other.fold("c", &PlanEvent::Superseded("a".into()));
    other.fold("a", &added("a", "Alpha"));
    other.fold("a", &PlanEvent::Governed("task-one".into()));
    other.fold(
        "a",
        &PlanEvent::MarkedSuperseded {
            by: "c".into(),
            from: PlanStatus::Draft,
        },
    );
    other.fold("b", &added("b", "Bravo"));
    other.fold("b", &PlanEvent::Governed("task-two".into()));
    other.fold("b", &PlanEvent::Governed("task-one".into()));
    assert_eq!(other.plans(), three().plans());
}

#[test]
fn governs_reads_both_ways_plan_to_tasks_and_task_to_plans() {
    let list = three();
    assert_eq!(list.get("b").unwrap().governs, vec!["task-one", "task-two"]);
    assert_eq!(ids(list.governing("task-one")), vec!["a", "b"]);
    assert_eq!(ids(list.governing("task-two")), vec!["b"]);
    assert!(list.governing("task-none").is_empty());
}

#[test]
fn an_ungoverned_task_leaves_the_reverse_index_too() {
    let mut list = three();
    list.fold("b", &PlanEvent::Ungoverned("task-two".into()));
    assert!(list.governing("task-two").is_empty());
    assert_eq!(list.get("b").unwrap().governs, vec!["task-one"]);
}

#[test]
fn with_status_filters_and_status_changes_move_a_row() {
    let mut list = three();
    list.fold(
        "b",
        &PlanEvent::StatusChanged {
            from: PlanStatus::Draft,
            to: PlanStatus::Active,
        },
    );
    assert_eq!(ids(list.with_status(PlanStatus::Active)), vec!["b"]);
    assert_eq!(ids(list.with_status(PlanStatus::Draft)), vec!["c"]);
    assert_eq!(ids(list.with_status(PlanStatus::Superseded)), vec!["a"]);
}

#[test]
fn by_path_finds_a_plan_by_its_recorded_path() {
    let list = three();
    assert_eq!(
        list.by_path(".taskloom/plans/c.plan.md").unwrap().harp_id,
        "c"
    );
    assert!(list.by_path("docs/none.plan.md").is_none());
}

#[test]
fn rehashed_replaces_the_rows_hash() {
    let mut list = three();
    let new = "1".repeat(64);
    list.fold("a", &PlanEvent::Rehashed(new.clone()));
    assert_eq!(list.get("a").unwrap().content_hash, new);
}

#[test]
fn supersedes_reaches_follows_the_chain_transitively() {
    let mut list = three();
    list.fold("d", &added("d", "Delta"));
    list.fold("d", &PlanEvent::Superseded("c".into()));
    assert!(list.supersedes_reaches("d", "a"));
    assert!(list.supersedes_reaches("c", "a"));
    assert!(list.supersedes_reaches("a", "a"));
    assert!(!list.supersedes_reaches("a", "d"));
    assert!(!list.supersedes_reaches("b", "a"));
}
