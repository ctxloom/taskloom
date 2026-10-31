//! RED: `taskloom plan` over a fake use-case layer: what each command asks
//! for, and what it writes, as text and as JSON, the VS Code fields
//! included.

use plans_application::{
    CheckReport, Finding, ImportReport, ImportedPlan, LeftTag, Link, NewPlan, PlanError, PlanQuery,
    PlanStatus, PlanText, PlanView,
};
use serde_json::{json, Value};

use super::*;

const TEXT: &str = "---\ntitle: Plans move into taskloom\n---\n# Plans\n";

fn view(id: &str, status: PlanStatus) -> PlanView {
    PlanView {
        harp_id: id.into(),
        title: format!("Title of {id}"),
        status,
        path: format!(".taskloom/plans/{id}.plan.md"),
        abs_path: format!("/tree/.taskloom/plans/{id}.plan.md"),
        name: id.into(),
        content_hash: "ab".repeat(32),
        origin_session: "inept-sixth-stick".into(),
        sessions: vec!["inept-sixth-stick".into(), "calm-dusty-heron".into()],
        governs: vec!["task-a".into()],
        supersedes: vec![],
        superseded_by: String::new(),
    }
}

/// Answers from `plans`; records what it was asked.
#[derive(Default)]
struct Fake {
    plans: Vec<PlanView>,
    report: CheckReport,
    import: ImportReport,
    asked: Vec<String>,
}

impl Plans for Fake {
    fn add(&mut self, new: NewPlan) -> Result<PlanView, PlanError> {
        self.asked.push(format!("{new:?}"));
        Ok(view("plan-1", new.status))
    }
    fn set_status(&mut self, plan: &str, status: PlanStatus) -> Result<PlanView, PlanError> {
        self.asked.push(format!("status {plan} {status:?}"));
        Ok(view(plan, status))
    }
    fn link(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.asked.push(format!("link {plan} {link:?}"));
        if link == Link::Governs("ghost".into()) {
            return Err(PlanError::NoSuchTask("ghost".into()));
        }
        Ok(view(plan, PlanStatus::Active))
    }
    fn unlink(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.asked.push(format!("unlink {plan} {link:?}"));
        Ok(view(plan, PlanStatus::Active))
    }
    fn list(&self, query: &PlanQuery) -> Vec<PlanView> {
        self.plans.clone()
    }
    fn show(&self, id_or_path: &str) -> Result<PlanText, PlanError> {
        self.plans
            .iter()
            .find(|p| p.harp_id == id_or_path || p.abs_path == id_or_path)
            .map(|p| PlanText {
                plan: p.clone(),
                content: Some(TEXT.into()),
            })
            .ok_or_else(|| PlanError::NoSuchPlan(id_or_path.into()))
    }
    fn check(&self) -> CheckReport {
        self.report.clone()
    }
    fn accept(&mut self, plan: &str) -> Result<PlanView, PlanError> {
        self.asked.push(format!("accept {plan}"));
        Ok(view(plan, PlanStatus::Active))
    }
    fn import(&mut self, dry_run: bool) -> ImportReport {
        self.asked.push(format!("import dry_run={dry_run}"));
        self.import.clone()
    }
}

struct Ran {
    code: u8,
    out: String,
    err: String,
}

impl Ran {
    fn json(&self) -> Value {
        serde_json::from_str(&self.out).unwrap_or_else(|e| panic!("{e}: {}", self.out))
    }
}

/// Run `taskloom <args>` on `fake`, stdout a terminal when `terminal`.
fn run_on(fake: &mut Fake, terminal: bool, args: &[&str]) -> Ran {
    let mut argv = vec!["taskloom"];
    argv.extend_from_slice(args);
    let context = Context {
        session: "inept-sixth-stick".into(),
        stdout_is_terminal: terminal,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(&argv, &context, fake, &mut out, &mut err);
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

fn two_plans() -> Fake {
    Fake {
        plans: vec![
            view("plan-1", PlanStatus::Active),
            view("plan-2", PlanStatus::Draft),
        ],
        ..Fake::default()
    }
}

// ---- add ----

#[test]
fn add_passes_title_path_status_governs_and_the_session() {
    let mut fake = Fake::default();
    let ran = run_on(
        &mut fake,
        true,
        &[
            "plan",
            "add",
            "--title",
            "T",
            "--path",
            "docs/x.plan.md",
            "--status",
            "active",
            "--governs",
            "task-a",
            "--governs",
            "task-b",
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    let expected = NewPlan {
        title: "T".into(),
        path: Some("docs/x.plan.md".into()),
        status: PlanStatus::Active,
        governs: vec!["task-a".into(), "task-b".into()],
        session: "inept-sixth-stick".into(),
    };
    assert_eq!(fake.asked, vec![format!("{expected:?}")]);
}

#[test]
fn add_in_text_names_the_plan_and_its_file() {
    let ran = run_on(&mut Fake::default(), true, &["plan", "add", "--title", "T"]);
    assert_eq!(ran.out, "added plan-1 .taskloom/plans/plan-1.plan.md\n");
}

#[test]
fn add_in_json_is_the_plan_object() {
    let ran = run_on(
        &mut Fake::default(),
        false,
        &["plan", "add", "--title", "T"],
    );
    assert_eq!(ran.json()["id"], "plan-1");
    assert_eq!(ran.json()["status"], "draft");
}

#[test]
fn add_refuses_an_unknown_status_as_a_usage_error() {
    let mut fake = Fake::default();
    let ran = run_on(
        &mut fake,
        true,
        &["plan", "add", "--title", "T", "--status", "nope"],
    );
    assert_eq!(ran.code, 2);
    assert!(fake.asked.is_empty());
}

// ---- list ----

#[test]
fn list_json_keeps_the_vscode_fields_and_adds_the_plans_own() {
    let ran = run_on(&mut two_plans(), true, &["plan", "list", "--json"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    let first = &ran.json()[0];
    // The VS Code contract (plans-data.ts): path absolute, name, title,
    // session (the origin session), sessions (the frontmatter list).
    assert_eq!(first["path"], "/tree/.taskloom/plans/plan-1.plan.md");
    assert_eq!(first["name"], "plan-1");
    assert_eq!(first["title"], "Title of plan-1");
    assert_eq!(first["session"], "inept-sixth-stick");
    assert_eq!(
        first["sessions"],
        json!(["inept-sixth-stick", "calm-dusty-heron"])
    );
    // The plan's own.
    assert_eq!(first["id"], "plan-1");
    assert_eq!(first["rel_path"], ".taskloom/plans/plan-1.plan.md");
    assert_eq!(first["status"], "active");
    assert_eq!(first["content_hash"], "ab".repeat(32));
    assert_eq!(first["governs"], json!(["task-a"]));
    assert_eq!(first["supersedes"], json!([]));
    assert_eq!(first["superseded_by"], Value::Null);
}

#[test]
fn list_json_with_no_plans_is_an_empty_array_and_sessions_never_null() {
    let ran = run_on(&mut Fake::default(), false, &["plan", "list"]);
    assert_eq!(ran.json(), json!([]));
    let mut fake = two_plans();
    fake.plans[0].sessions.clear();
    assert_eq!(
        run_on(&mut fake, false, &["plan", "list"]).json()[0]["sessions"],
        json!([])
    );
}

#[test]
fn list_text_is_a_line_per_plan() {
    let ran = run_on(&mut two_plans(), true, &["plan", "list"]);
    assert_eq!(
        ran.out,
        "plan-1 [active] Title of plan-1 (.taskloom/plans/plan-1.plan.md)\n\
         plan-2 [draft] Title of plan-2 (.taskloom/plans/plan-2.plan.md)\n"
    );
}

/// The fake ignores the query, so this asks what `list` sends instead.
#[derive(Default)]
struct Querying(std::cell::RefCell<Vec<PlanQuery>>, Fake);

impl Plans for Querying {
    fn add(&mut self, new: NewPlan) -> Result<PlanView, PlanError> {
        self.1.add(new)
    }
    fn set_status(&mut self, plan: &str, status: PlanStatus) -> Result<PlanView, PlanError> {
        self.1.set_status(plan, status)
    }
    fn link(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.1.link(plan, link)
    }
    fn unlink(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.1.unlink(plan, link)
    }
    fn list(&self, query: &PlanQuery) -> Vec<PlanView> {
        self.0.borrow_mut().push(query.clone());
        Vec::new()
    }
    fn show(&self, id_or_path: &str) -> Result<PlanText, PlanError> {
        self.1.show(id_or_path)
    }
    fn check(&self) -> CheckReport {
        self.1.check()
    }
    fn accept(&mut self, plan: &str) -> Result<PlanView, PlanError> {
        self.1.accept(plan)
    }
    fn import(&mut self, dry_run: bool) -> ImportReport {
        self.1.import(dry_run)
    }
}

#[test]
fn list_sends_its_status_and_governs_filters() {
    let mut querying = Querying::default();
    let argv = [
        "taskloom",
        "plan",
        "list",
        "--status",
        "active",
        "--status",
        "draft",
        "--governs",
        "task-a",
    ];
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(
        &argv,
        &Context::default(),
        &mut querying,
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0);
    assert_eq!(
        querying.0.borrow().as_slice(),
        &[PlanQuery {
            statuses: vec![PlanStatus::Active, PlanStatus::Draft],
            governs: Some("task-a".into()),
        }]
    );
}

// ---- show ----

#[test]
fn show_off_a_terminal_without_json_writes_the_markdown_raw_for_vscode() {
    let ran = run_on(
        &mut two_plans(),
        false,
        &["plan", "show", "/tree/.taskloom/plans/plan-1.plan.md"],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(ran.out, TEXT);
}

#[test]
fn show_json_is_the_plan_object_with_its_content() {
    let ran = run_on(
        &mut two_plans(),
        true,
        &["plan", "show", "plan-1", "--json"],
    );
    let shown = ran.json();
    assert_eq!(shown["id"], "plan-1");
    assert_eq!(shown["path"], "/tree/.taskloom/plans/plan-1.plan.md");
    assert_eq!(shown["governs"], json!(["task-a"]));
    assert_eq!(shown["content"], TEXT);
}

#[test]
fn show_of_an_unknown_plan_exits_1_naming_it_on_stderr() {
    let ran = run_on(&mut two_plans(), true, &["plan", "show", "nope"]);
    assert_eq!(ran.code, 1);
    assert!(ran.out.is_empty());
    assert!(ran.err.contains("nope"), "{}", ran.err);
}

// ---- status ----

#[test]
fn status_sends_the_parsed_status_and_says_it_in_text_and_json() {
    let mut fake = two_plans();
    let ran = run_on(&mut fake, true, &["plan", "status", "plan-2", "Active"]);
    assert_eq!(fake.asked, vec!["status plan-2 Active"]);
    assert_eq!(ran.out, "plan-2 active\n");
    let ran = run_on(
        &mut two_plans(),
        true,
        &["plan", "status", "plan-2", "done", "--json"],
    );
    assert_eq!(ran.json()["status"], "done");
}

// ---- link and unlink ----

#[test]
fn link_governs_sends_one_link_per_task_and_text_names_each() {
    let mut fake = two_plans();
    let ran = run_on(
        &mut fake,
        true,
        &[
            "plan",
            "link",
            "plan-1",
            "--governs",
            "task-a",
            "--governs",
            "task-b",
        ],
    );
    assert_eq!(
        fake.asked,
        vec![
            r#"link plan-1 Governs("task-a")"#,
            r#"link plan-1 Governs("task-b")"#
        ]
    );
    assert_eq!(ran.out, "plan-1 governs task-a\nplan-1 governs task-b\n");
}

#[test]
fn link_supersedes_and_unlink_supersedes_send_their_link() {
    let mut fake = two_plans();
    let ran = run_on(
        &mut fake,
        true,
        &["plan", "link", "plan-2", "--supersedes", "plan-1"],
    );
    assert_eq!(ran.out, "plan-2 supersedes plan-1\n");
    let ran = run_on(
        &mut fake,
        true,
        &["plan", "unlink", "plan-2", "--supersedes", "plan-1"],
    );
    assert_eq!(ran.out, "plan-2 no longer supersedes plan-1\n");
    assert_eq!(
        fake.asked,
        vec![
            r#"link plan-2 Supersedes("plan-1")"#,
            r#"unlink plan-2 Supersedes("plan-1")"#
        ]
    );
}

#[test]
fn link_with_neither_governs_nor_supersedes_is_a_usage_error() {
    let mut fake = two_plans();
    assert_eq!(run_on(&mut fake, true, &["plan", "link", "plan-1"]).code, 2);
    assert!(fake.asked.is_empty());
}

#[test]
fn a_refused_link_exits_1_with_the_reason_on_stderr_and_json_envelope() {
    let ran = run_on(
        &mut two_plans(),
        true,
        &["plan", "link", "plan-1", "--governs", "ghost"],
    );
    assert_eq!(ran.code, 1);
    assert!(ran.err.contains("ghost"), "{}", ran.err);
    let ran = run_on(
        &mut two_plans(),
        false,
        &["plan", "link", "plan-1", "--governs", "ghost"],
    );
    let envelope: Value = serde_json::from_str(&ran.err).unwrap();
    assert_eq!(envelope["code"], "plans_application.no_such_task");
}

// ---- check ----

fn drifted() -> Fake {
    Fake {
        report: CheckReport {
            findings: vec![
                Finding::Missing {
                    plan: "plan-1".into(),
                    path: ".taskloom/plans/plan-1.plan.md".into(),
                },
                Finding::HashMismatch {
                    plan: "plan-2".into(),
                    path: "docs/x.plan.md".into(),
                    recorded: "a".repeat(64),
                    actual: "b".repeat(64),
                },
                Finding::External {
                    plan: "plan-3".into(),
                    path: "/home/b/v1.plan.md".into(),
                },
            ],
        },
        ..Fake::default()
    }
}

#[test]
fn check_text_is_a_line_per_finding_and_exits_1_on_drift() {
    let ran = run_on(&mut drifted(), true, &["plan", "check"]);
    assert_eq!(ran.code, 1);
    assert_eq!(
        ran.out,
        "missing: plan-1 .taskloom/plans/plan-1.plan.md\n\
         changed: plan-2 docs/x.plan.md (recorded aaaaaaaaaaaa, now bbbbbbbbbbbb)\n\
         external: plan-3 /home/b/v1.plan.md\n"
    );
}

#[test]
fn check_json_names_each_finding_by_kind() {
    let ran = run_on(&mut drifted(), false, &["plan", "check"]);
    assert_eq!(ran.code, 1);
    assert_eq!(
        ran.json(),
        json!({
            "ok": false,
            "findings": [
                {"kind": "missing", "plan": "plan-1", "path": ".taskloom/plans/plan-1.plan.md"},
                {"kind": "hash_mismatch", "plan": "plan-2", "path": "docs/x.plan.md",
                 "recorded": "a".repeat(64), "actual": "b".repeat(64)},
                {"kind": "external", "plan": "plan-3", "path": "/home/b/v1.plan.md"},
            ],
        })
    );
}

#[test]
fn check_with_nothing_found_exits_0_and_says_so() {
    let ran = run_on(&mut Fake::default(), true, &["plan", "check"]);
    assert_eq!(ran.code, 0);
    assert_eq!(ran.out, "plans: no drift\n");
    let ran = run_on(&mut Fake::default(), false, &["plan", "check"]);
    assert_eq!(ran.json(), json!({"ok": true, "findings": []}));
}

#[test]
fn check_with_only_an_external_finding_exits_0() {
    let mut fake = drifted();
    fake.report.findings.drain(..2);
    assert_eq!(run_on(&mut fake, true, &["plan", "check"]).code, 0);
}

#[test]
fn check_accept_records_the_hash_and_then_checks() {
    let mut fake = Fake::default();
    let ran = run_on(&mut fake, true, &["plan", "check", "--accept", "plan-2"]);
    assert_eq!(fake.asked, vec!["accept plan-2"]);
    assert_eq!(ran.code, 0);
}

// ---- import ----

fn imported() -> Fake {
    Fake {
        import: ImportReport {
            plans: vec![ImportedPlan {
                harp_id: "plan-1".into(),
                path: "docs/design.plan.md".into(),
                external: false,
                new: true,
                governs: vec!["task-a".into(), "task-b".into()],
            }],
            dead: vec![LeftTag {
                value: "/home/b/gone.plan.md".into(),
                tasks: vec!["task-c".into()],
            }],
            not_plans: vec![LeftTag {
                value: "docs/README.md".into(),
                tasks: vec!["task-d".into()],
            }],
            retired: vec![
                ("task-a".into(), "docs/design.plan.md".into()),
                ("task-b".into(), "docs/design.plan.md".into()),
            ],
        },
        ..Fake::default()
    }
}

#[test]
fn import_text_reports_plans_dead_paths_non_plans_and_retired_tags() {
    let mut fake = imported();
    let ran = run_on(&mut fake, true, &["plan", "import"]);
    assert_eq!(fake.asked, vec!["import dry_run=false"]);
    assert_eq!(
        ran.out,
        "plan-1 docs/design.plan.md (new): governs task-a, task-b\n\
         dead: /home/b/gone.plan.md (task-c): tag kept\n\
         not a plan: docs/README.md (task-d): tag kept\n\
         2 ctxloom:plan tags retired\n"
    );
}

#[test]
fn import_with_a_dead_path_exits_1_so_a_script_sees_what_was_left() {
    assert_eq!(run_on(&mut imported(), true, &["plan", "import"]).code, 1);
    let mut clean = imported();
    clean.import.dead.clear();
    clean.import.not_plans.clear();
    assert_eq!(run_on(&mut clean, true, &["plan", "import"]).code, 0);
}

#[test]
fn import_json_is_the_whole_report() {
    let ran = run_on(&mut imported(), false, &["plan", "import", "--dry-run"]);
    let report = ran.json();
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["plans"][0]["id"], "plan-1");
    assert_eq!(report["plans"][0]["governs"], json!(["task-a", "task-b"]));
    assert_eq!(
        report["dead"][0],
        json!({"value": "/home/b/gone.plan.md", "tasks": ["task-c"]})
    );
    assert_eq!(report["not_plans"][0]["value"], "docs/README.md");
    assert_eq!(
        report["retired"][0],
        json!({"task": "task-a", "value": "docs/design.plan.md"})
    );
}
