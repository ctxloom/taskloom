//! The renderings beyond what the command tests read.

use angzarr_presentation::{render, Format, Render};
use plans_application::{CheckReport, Finding, Plan};

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

fn plan() -> Plan {
    Plan::parse(
        "/tree/old.plan.md",
        "---\nid: old-one\nstatus: active\n---\n# Old\n",
    )
}

#[test]
fn a_plans_json_names_what_supersedes_it() {
    let mut superseded = plan();
    superseded.status = "superseded".into();
    superseded.superseded_by = vec!["new-one".into()];
    let shown = json(&Written(&superseded));
    assert_eq!(shown["superseded_by"], serde_json::json!(["new-one"]));
    assert_eq!(shown["status"], "superseded");
    assert_eq!(
        text(&Written(&superseded)),
        "old-one [superseded] Old (/tree/old.plan.md)\n"
    );
}

#[test]
fn a_lists_json_is_an_array_and_its_text_a_line_a_plan() {
    let plans = vec![plan(), plan()];
    assert_eq!(json(&Listed(&plans)).as_array().map(Vec::len), Some(2));
    assert_eq!(text(&Listed(&plans)).lines().count(), 2);
    assert_eq!(text(&Listed(&[])), "");
    assert_eq!(json(&Listed(&[])), serde_json::json!([]));
}

#[test]
fn a_check_with_a_finding_is_not_ok_and_says_it_in_the_catalog() {
    let catalog = Catalog::en_us(&[crate::EN_US, plans_application::EN_US]).unwrap();
    let report = CheckReport {
        findings: vec![Finding::NoId {
            path: "/tree/bare.plan.md".into(),
        }],
    };
    let view = Checked {
        report: &report,
        catalog: &catalog,
    };
    assert_eq!(json(&view)["ok"], false);
    assert_eq!(
        text(&view),
        "/tree/bare.plan.md: the file has no id (taskloom gives it one the first time it writes the file)\n"
    );
}
