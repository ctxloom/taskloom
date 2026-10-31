//! RED: `taskloom plan` over a fake use-case layer: what each command asks
//! for, and what it writes, as text and as JSON, the VS Code fields
//! included.

use std::future::Future;

use clap::Parser;
use plans_application::{
    CheckReport, Finding, ImportReport, ImportedPlan, LeftTag, Link, NewPlan, PlanError, PlanQuery,
    PlanStatus, PlanText, PlanView,
};
use presentation::{Catalog, Format};
use serde_json::{json, Value};

use super::*;

/// What the binary reads from its process for the group, besides its
/// arguments.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Context {
    session: String,
    stdout_is_terminal: bool,
}

/// The command line as a binary mounting the group parses it: its global
/// format flags, and `plan`.
#[derive(Parser, Debug)]
#[command(name = "taskloom")]
struct Cli {
    #[command(subcommand)]
    group: Group,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    format: Option<String>,
}

/// Poll `future` once: every fake answers at once.
fn ready<F: Future>(future: F) -> F::Output {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match std::pin::pin!(future).poll(&mut cx) {
        std::task::Poll::Ready(out) => out,
        std::task::Poll::Pending => panic!("the fakes answer at once"),
    }
}

/// `args` (the program's name first) parsed as a binary mounting the group
/// does, the format chosen as it chooses, and the group run on `plans`
/// with the catalog the binary would hand it: the exit code.
fn run(
    args: &[&str],
    context: &Context,
    plans: &mut impl Plans,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let cli = Cli::try_parse_from(args).unwrap_or_else(|e| panic!("{e}"));
    let settings = Settings {
        format: Format::choose(
            cli.json,
            cli.format.map(|f| f.parse().unwrap()),
            context.stdout_is_terminal,
        ),
        session: context.session.clone(),
        prog: "taskloom",
    };
    let mut sources = vec![EN_US];
    sources.extend(plans_application::CATALOGS);
    let catalog = Catalog::en_us(&sources).unwrap();
    ready(super::run(cli.group, &settings, &catalog, plans, out, err))
}

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
    async fn add(&mut self, new: NewPlan) -> Result<PlanView, PlanError> {
        self.asked.push(format!("{new:?}"));
        Ok(view("plan-1", new.status))
    }
    async fn set_status(&mut self, plan: &str, status: PlanStatus) -> Result<PlanView, PlanError> {
        self.asked.push(format!("status {plan} {status:?}"));
        Ok(view(plan, status))
    }
    async fn link(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.asked.push(format!("link {plan} {link:?}"));
        if link == Link::Governs("ghost".into()) {
            return Err(PlanError::NoSuchTask("ghost".into()));
        }
        Ok(view(plan, PlanStatus::Active))
    }
    async fn unlink(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.asked.push(format!("unlink {plan} {link:?}"));
        Ok(view(plan, PlanStatus::Active))
    }
    async fn list(&mut self, _query: &PlanQuery) -> Result<Vec<PlanView>, PlanError> {
        Ok(self.plans.clone())
    }
    async fn show(&mut self, id_or_path: &str) -> Result<PlanText, PlanError> {
        self.plans
            .iter()
            .find(|p| p.harp_id == id_or_path || p.abs_path == id_or_path)
            .map(|p| PlanText {
                plan: p.clone(),
                content: Some(TEXT.into()),
            })
            .ok_or_else(|| PlanError::NoSuchPlan(id_or_path.into()))
    }
    async fn check(&mut self) -> Result<CheckReport, PlanError> {
        Ok(self.report.clone())
    }
    async fn import(&mut self, dry_run: bool) -> Result<ImportReport, PlanError> {
        self.asked.push(format!("import dry_run={dry_run}"));
        Ok(self.import.clone())
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
    async fn add(&mut self, new: NewPlan) -> Result<PlanView, PlanError> {
        self.1.add(new).await
    }
    async fn set_status(&mut self, plan: &str, status: PlanStatus) -> Result<PlanView, PlanError> {
        self.1.set_status(plan, status).await
    }
    async fn link(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.1.link(plan, link).await
    }
    async fn unlink(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.1.unlink(plan, link).await
    }
    async fn list(&mut self, query: &PlanQuery) -> Result<Vec<PlanView>, PlanError> {
        self.0.borrow_mut().push(query.clone());
        Ok(Vec::new())
    }
    async fn show(&mut self, id_or_path: &str) -> Result<PlanText, PlanError> {
        self.1.show(id_or_path).await
    }
    async fn check(&mut self) -> Result<CheckReport, PlanError> {
        self.1.check().await
    }
    async fn import(&mut self, dry_run: bool) -> Result<ImportReport, PlanError> {
        self.1.import(dry_run).await
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

/// Ruling (c), 2026-10-30: `plan show` follows the CLI's convention, JSON
/// off a terminal; the markdown, raw, is its text (`--format text`, which
/// the VS Code extension passes).
#[test]
fn show_off_a_terminal_writes_json_and_its_text_is_the_markdown_raw() {
    let ran = run_on(
        &mut two_plans(),
        false,
        &["plan", "show", "/tree/.taskloom/plans/plan-1.plan.md"],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(ran.json()["content"], TEXT);
    let ran = run_on(
        &mut two_plans(),
        false,
        &[
            "plan",
            "show",
            "/tree/.taskloom/plans/plan-1.plan.md",
            "--format",
            "text",
        ],
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
    assert_eq!(envelope["code"], "plans-application-no-such-task");
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
                // Ruling (a): a changed hash is no finding; the drafted
                // hash mismatch and external plan are gone.
                Finding::Unregistered {
                    path: ".taskloom/plans/stray.plan.md".into(),
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
         unregistered: .taskloom/plans/stray.plan.md\n"
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
                {"kind": "unregistered", "path": ".taskloom/plans/stray.plan.md"},
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

/// Ruling (a): every finding fails check, an unregistered file alone too.
#[test]
fn check_with_only_an_unregistered_file_exits_1() {
    let mut fake = drifted();
    fake.report.findings.drain(..1);
    assert_eq!(run_on(&mut fake, true, &["plan", "check"]).code, 1);
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

// ---- beyond the reviewed tests ----

/// Refuses its second link; dead-letters one line.
#[derive(Default)]
struct Refusing(Fake, u32);

impl Plans for Refusing {
    async fn add(&mut self, new: NewPlan) -> Result<PlanView, PlanError> {
        self.0.add(new).await
    }
    async fn set_status(&mut self, plan: &str, status: PlanStatus) -> Result<PlanView, PlanError> {
        self.0.set_status(plan, status).await
    }
    async fn link(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.1 += 1;
        match self.1 {
            1 => self.0.link(plan, link).await,
            _ => Err(PlanError::NoSuchTask("task-b".into())),
        }
    }
    async fn unlink(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.0.unlink(plan, link).await
    }
    async fn list(&mut self, query: &PlanQuery) -> Result<Vec<PlanView>, PlanError> {
        self.0.list(query).await
    }
    async fn show(&mut self, id_or_path: &str) -> Result<PlanText, PlanError> {
        Ok(PlanText {
            plan: view(id_or_path, PlanStatus::Draft),
            content: None,
        })
    }
    async fn check(&mut self) -> Result<CheckReport, PlanError> {
        self.0.check().await
    }
    async fn import(&mut self, dry_run: bool) -> Result<ImportReport, PlanError> {
        self.0.import(dry_run).await
    }
    fn dead_letters(&mut self) -> Vec<plans_application::DeadLetter> {
        vec![plans_application::DeadLetter {
            line: "DEAD-LETTER x".into(),
        }]
    }
}

fn run_refusing(terminal: bool, args: &[&str]) -> Ran {
    let mut argv = vec!["taskloom"];
    argv.extend_from_slice(args);
    let context = Context {
        session: String::new(),
        stdout_is_terminal: terminal,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(
        &argv,
        &context,
        &mut Refusing::default(),
        &mut out,
        &mut err,
    );
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

#[test]
fn a_link_refused_part_way_says_the_links_made_and_the_refusal() {
    let ran = run_refusing(
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
    assert_eq!(ran.code, 1);
    assert_eq!(ran.out, "plan-1 governs task-a\n");
    assert!(ran.err.contains("task-b"), "{}", ran.err);
}

#[test]
fn every_dead_letter_is_said_on_stderr() {
    let ran = run_refusing(true, &["plan", "list"]);
    assert_eq!(ran.code, 0);
    // Its message is the tasks use cases' catalog's, which the binary's
    // holds and this one does not.
    assert!(ran.err.starts_with("taskloom: "), "{}", ran.err);
    assert!(ran.err.contains("DEAD-LETTER x"), "{}", ran.err);
}

#[test]
fn show_in_text_of_a_plan_whose_file_is_gone_exits_1_naming_it() {
    let ran = run_refusing(true, &["plan", "show", "plan-1"]);
    assert_eq!(ran.code, 1);
    assert!(ran.out.is_empty());
    assert!(
        ran.err.contains(".taskloom/plans/plan-1.plan.md"),
        "{}",
        ran.err
    );
    let ran = run_refusing(false, &["plan", "show", "plan-1"]);
    assert_eq!(ran.code, 0);
    assert_eq!(ran.json()["content"], Value::Null);
}

#[test]
fn an_unknown_status_anywhere_is_a_usage_error() {
    let mut fake = two_plans();
    assert_eq!(
        run_on(&mut fake, true, &["plan", "status", "plan-1", "nope"]).code,
        2
    );
    assert_eq!(
        run_on(&mut fake, true, &["plan", "list", "--status", "nope"]).code,
        2
    );
    assert!(fake.asked.is_empty());
}

#[test]
fn unlink_governs_says_each_link_removed() {
    let mut fake = two_plans();
    let ran = run_on(
        &mut fake,
        true,
        &["plan", "unlink", "plan-1", "--governs", "task-a"],
    );
    assert_eq!(ran.out, "plan-1 no longer governs task-a\n");
    assert_eq!(
        run_on(&mut fake, true, &["plan", "unlink", "plan-1"]).code,
        2
    );
}

#[test]
fn a_dry_run_import_says_nothing_changed() {
    let ran = run_on(&mut imported(), true, &["plan", "import", "--dry-run"]);
    assert!(
        ran.out
            .ends_with("2 ctxloom:plan tags would be retired (dry run: nothing changed)\n"),
        "{}",
        ran.out
    );
}
