//! The renderings beyond what the command tests read.

use angzarr_presentation::{render, Format, Render};
use plans_application::{PlanStatus, PlanView};

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
fn a_governed_task_gone_is_a_finding_in_text_and_json() {
    let report = plans_application::CheckReport {
        findings: vec![plans_application::Finding::MissingTask {
            plan: "plan-1".into(),
            task: "task-a".into(),
        }],
    };
    assert_eq!(text(&Checked(&report)), "missing task: plan-1 task-a\n");
    assert_eq!(
        json(&Checked(&report)),
        serde_json::json!({"ok": false, "findings": [
            {"kind": "missing_task", "plan": "plan-1", "task": "task-a"}
        ]})
    );
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
