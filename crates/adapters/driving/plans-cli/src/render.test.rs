//! The renderings beyond what the command tests read.

use plans_application::{ImportReport, ImportedPlan, PlanStatus, PlanView};
use presentation::{render, Format, Render};

use super::*;

fn text(view: &impl Render) -> String {
    let mut out = Vec::new();
    render(&mut out, Format::Text, view).unwrap();
    String::from_utf8(out).unwrap()
}

fn json(view: &impl Render) -> serde_json::Value {
    let mut out = Vec::new();
    render(&mut out, Format::Json, view).unwrap();
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn a_superseded_plans_json_names_its_successor() {
    let plan = PlanView {
        harp_id: "old".into(),
        status: PlanStatus::Superseded,
        superseded_by: "new".into(),
        ..PlanView::default()
    };
    let shown = json(&Moved(&plan));
    assert_eq!(shown["superseded_by"], "new");
    assert_eq!(shown["status"], "superseded");
    assert_eq!(text(&Moved(&plan)), "old superseded\n");
}

#[test]
fn an_import_says_which_plans_were_new_and_which_copied_in() {
    let plan = |id: &str, new, external| ImportedPlan {
        harp_id: id.into(),
        path: format!(".taskloom/plans/{id}.plan.md"),
        external,
        new,
        governs: vec!["task-a".into()],
    };
    let report = ImportReport {
        plans: vec![plan("a", true, true), plan("b", false, false)],
        retired: vec![("task-a".into(), "x".into())],
        ..ImportReport::default()
    };
    let imported = Imported {
        report: &report,
        dry_run: false,
    };
    assert_eq!(
        text(&imported),
        "a .taskloom/plans/a.plan.md (new, copied in): governs task-a\n\
         b .taskloom/plans/b.plan.md: governs task-a\n\
         1 ctxloom:plan tag retired\n"
    );
    let shown = json(&imported);
    assert_eq!(shown["dry_run"], false);
    assert_eq!(shown["plans"][0]["external"], true);
    assert_eq!(shown["plans"][1]["new"], false);
    assert_eq!(shown["dead"], serde_json::json!([]));
}
