//! `taskloom plans` over a fake use-case layer: what each command asks for,
//! and what it writes, as text and as JSON, the VS Code fields included.

use std::future::Future;

use angzarr_diagnostic::Diagnostic;
use angzarr_embedded::testing::block_on_ready as ready;
use angzarr_presentation::{Catalog, Format};
use clap::Parser;
use plans_application::{
    CheckReport, Finding, FrontError, Link, NewPlan, Plan, PlanError, PlanQuery, PlanText, SetPlan,
    Snapshot, Stamp,
};
use serde_json::{json, Value};

use super::*;

/// The command line as a binary mounting the group parses it.
#[derive(clap::Subcommand, Debug)]
enum Group_ {
    #[command(subcommand)]
    Plans(Command),
}

#[derive(Parser, Debug)]
#[command(name = "taskloom", disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    group: Group_,
}

fn plan(id: &str, status: &str) -> Plan {
    Plan {
        path: format!("/tree/{id}.plan.md"),
        name: id.to_string(),
        id: Some(id.to_string()),
        title: format!("Title of {id}"),
        written_title: None,
        written_status: None,
        status: status.to_string(),
        tags: vec!["area:cli".to_string()],
        governs: vec!["swift-amber-falcon".to_string()],
        supersedes: Vec::new(),
        sessions: vec!["one".to_string(), "two".to_string()],
        superseded_by: Vec::new(),
        unreadable: None,
    }
}

/// What the fake was asked, and what it answers.
#[derive(Default)]
struct Fake {
    asked: Vec<String>,
    queries: Vec<PlanQuery>,
    listed: Vec<Plan>,
    failure: Option<PlanError>,
    report: CheckReport,
}

impl Fake {
    fn answer(&mut self, asked: String) -> Result<Plan, PlanError> {
        self.asked.push(asked);
        match self.failure.clone() {
            Some(error) => Err(error),
            None => Ok(plan("quiet-amber-falcon", "active")),
        }
    }
}

impl Plans for Fake {
    fn list(&mut self, query: &PlanQuery) -> impl Future<Output = Result<Vec<Plan>, PlanError>> {
        self.queries.push(query.clone());
        let answer = match self.failure.clone() {
            Some(error) => Err(error),
            None => Ok(self.listed.clone()),
        };
        async move { answer }
    }

    fn show(&mut self, name: &str) -> impl Future<Output = Result<PlanText, PlanError>> {
        let answer = self.answer(format!("show {name}")).map(|plan| PlanText {
            plan,
            content: "# the file\n".to_string(),
        });
        async move { answer }
    }

    fn add(&mut self, new: NewPlan) -> impl Future<Output = Result<Plan, PlanError>> {
        let answer = self.answer(format!(
            "add {:?} {:?} {:?} {:?}",
            new.title, new.path, new.status, new.governs
        ));
        async move { answer }
    }

    fn set(
        &mut self,
        plan: &str,
        change: SetPlan,
    ) -> impl Future<Output = Result<Plan, PlanError>> {
        let answer = self.answer(format!("set {plan} {:?} {:?}", change.title, change.status));
        async move { answer }
    }

    fn tag(
        &mut self,
        plan: &str,
        tag: &str,
        add: bool,
    ) -> impl Future<Output = Result<Plan, PlanError>> {
        let answer = self.answer(format!("tag {plan} {tag} {add}"));
        async move { answer }
    }

    fn link(
        &mut self,
        plan: &str,
        link: Link,
        linking: bool,
    ) -> impl Future<Output = Result<Plan, PlanError>> {
        let answer = self.answer(format!("link {plan} {link:?} {linking}"));
        async move { answer }
    }

    fn check(&mut self) -> impl Future<Output = Result<CheckReport, PlanError>> {
        let answer = match self.failure.clone() {
            Some(error) => Err(error),
            None => Ok(self.report.clone()),
        };
        async move { answer }
    }

    fn stamp(&mut self, path: &str, session: &str) -> Result<Stamp, PlanError> {
        self.asked.push(format!("stamp {path} {session}"));
        match self.failure.clone() {
            Some(error) => Err(error),
            None => Ok(Stamp::Stamped),
        }
    }

    fn snapshot(&mut self) -> Result<Snapshot, PlanError> {
        Ok(Snapshot::new())
    }
}

struct Ran {
    code: u8,
    out: String,
    err: String,
}

fn catalog() -> Catalog {
    let mut sources = vec![EN_US];
    sources.extend(plans_application::CATALOGS);
    sources.extend([tag_rules_catalog()]);
    Catalog::en_us(&sources).unwrap()
}

fn tag_rules_catalog() -> (&'static str, &'static str) {
    ("tag-rules", "")
}

fn run_as(format: Format, args: &[&str], fake: &mut Fake) -> Ran {
    let cli = Cli::try_parse_from(
        ["taskloom", "plans"]
            .into_iter()
            .chain(args.iter().copied()),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let Group_::Plans(command) = cli.group;
    let settings = Settings {
        format,
        session: "this-session".to_string(),
        prog: "taskloom",
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = ready(super::run(
        command,
        &settings,
        &catalog(),
        fake,
        &mut out,
        &mut err,
    ));
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

fn json_of(args: &[&str], fake: &mut Fake) -> Value {
    let ran = run_as(Format::Json, args, fake);
    assert_eq!(ran.code, 0, "{}", ran.err);
    serde_json::from_str(&ran.out).unwrap()
}

#[test]
fn list_asks_for_what_the_flags_say_and_answers_the_extensions_fields() {
    let mut fake = Fake {
        listed: vec![plan("quiet-amber-falcon", "active")],
        ..Fake::default()
    };
    let listed = json_of(
        &[
            "list",
            "--status",
            "active,done",
            "--governs",
            "swift-amber-falcon",
            "--tag-query",
            "area:cli",
        ],
        &mut fake,
    );
    assert_eq!(
        fake.queries,
        [PlanQuery {
            statuses: vec!["active".into(), "done".into()],
            governs: Some("swift-amber-falcon".into()),
            tag_query: Some("area:cli".into()),
        }]
    );
    let first = &listed[0];
    assert_eq!(first["path"], "/tree/quiet-amber-falcon.plan.md");
    assert_eq!(first["name"], "quiet-amber-falcon");
    assert_eq!(first["title"], "Title of quiet-amber-falcon");
    assert_eq!(first["session"], "one");
    assert_eq!(first["sessions"], json!(["one", "two"]));
    assert_eq!(first["id"], "quiet-amber-falcon");
    assert_eq!(first["status"], "active");
    assert_eq!(first["tags"], json!(["area:cli"]));
    assert_eq!(first["governs"], json!(["swift-amber-falcon"]));
    assert_eq!(first["supersedes"], json!([]));
    let text = run_as(Format::Text, &["list"], &mut fake);
    assert_eq!(
        text.out,
        "quiet-amber-falcon [active] Title of quiet-amber-falcon (/tree/quiet-amber-falcon.plan.md)\n"
    );
}

#[test]
fn a_plan_with_no_id_is_null_in_json_and_a_dash_in_text() {
    let mut bare = plan("x", "draft");
    bare.id = None;
    bare.sessions = Vec::new();
    let mut fake = Fake {
        listed: vec![bare],
        ..Fake::default()
    };
    let listed = json_of(&["list"], &mut fake);
    assert_eq!(listed[0]["id"], Value::Null);
    assert_eq!(listed[0]["session"], "");
    let text = run_as(Format::Text, &["list"], &mut fake);
    assert!(text.out.starts_with("- [draft]"), "{}", text.out);
}

#[test]
fn show_is_the_file_raw_in_text_and_the_metadata_and_content_in_json() {
    let mut fake = Fake::default();
    let text = run_as(Format::Text, &["show", "quiet-amber-falcon"], &mut fake);
    assert_eq!(text.out, "# the file\n");
    let shown = json_of(&["show", "docs/p.plan.md"], &mut fake);
    assert_eq!(shown["content"], "# the file\n");
    assert_eq!(shown["id"], "quiet-amber-falcon");
    assert_eq!(
        fake.asked,
        ["show quiet-amber-falcon", "show docs/p.plan.md"]
    );
}

#[test]
fn add_set_and_tag_pass_their_arguments_and_answer_the_plan() {
    let mut fake = Fake::default();
    let added = json_of(
        &[
            "add",
            "--title",
            "T",
            "--path",
            "a.plan.md",
            "--status",
            "active",
            "--governs",
            "g1",
            "--governs",
            "g2",
        ],
        &mut fake,
    );
    assert_eq!(added["id"], "quiet-amber-falcon");
    json_of(
        &["set", "p", "--title", "New", "--status", "done"],
        &mut fake,
    );
    json_of(&["tag", "add", "p", "area:cli"], &mut fake);
    json_of(&["tag", "remove", "p", "area:cli"], &mut fake);
    assert_eq!(
        fake.asked,
        [
            r#"add "T" Some("a.plan.md") Some("active") ["g1", "g2"]"#,
            r#"set p Some("New") Some("done")"#,
            "tag p area:cli true",
            "tag p area:cli false",
        ]
    );
}

#[test]
fn link_makes_one_call_per_link_and_unlink_unmakes() {
    let mut fake = Fake::default();
    json_of(
        &["link", "p", "--governs", "g1", "--governs", "g2"],
        &mut fake,
    );
    json_of(&["link", "p", "--supersedes", "q"], &mut fake);
    json_of(&["unlink", "p", "--governs", "g1"], &mut fake);
    assert_eq!(
        fake.asked,
        [
            r#"link p Governs("g1") true"#,
            r#"link p Governs("g2") true"#,
            r#"link p Supersedes("q") true"#,
            r#"link p Governs("g1") false"#,
        ]
    );
    let none = run_as(Format::Json, &["link", "p"], &mut fake);
    assert_eq!(none.code, 2);
    assert!(none.err.contains("--governs"), "{}", none.err);
    assert_eq!(fake.asked.len(), 4, "nothing asked");
}

#[test]
fn a_supersedes_beside_a_governs_is_a_usage_error() {
    assert!(Cli::try_parse_from([
        "taskloom",
        "plans",
        "link",
        "p",
        "--governs",
        "g",
        "--supersedes",
        "q"
    ])
    .is_err());
}

#[test]
fn the_use_cases_refusals_exit_2_when_invalid_on_their_own_content_and_1_otherwise() {
    let mut fake = Fake {
        failure: Some(PlanError::UnknownStatus {
            status: "nonsense".into(),
        }),
        ..Fake::default()
    };
    let ran = run_as(
        Format::Text,
        &["set", "p", "--status", "nonsense"],
        &mut fake,
    );
    assert_eq!(ran.code, 2);
    assert!(ran.err.contains("nonsense"), "{}", ran.err);
    let json = run_as(
        Format::Json,
        &["set", "p", "--status", "nonsense"],
        &mut fake,
    );
    assert_eq!(json.code, 2);
    let envelope: Value = serde_json::from_str(&json.err).unwrap();
    assert_eq!(envelope["code"], "plans-application-unknown-status");

    fake.failure = Some(PlanError::NoSuchPlan("p".into()));
    for args in [
        vec!["show", "p"],
        vec!["list"],
        vec!["check"],
        vec!["tag", "add", "p", "x"],
    ] {
        assert_eq!(run_as(Format::Text, &args, &mut fake).code, 1, "{args:?}");
    }
}

#[test]
fn check_is_exit_0_and_clean_or_exit_1_naming_each_finding() {
    let mut fake = Fake::default();
    let clean = run_as(Format::Json, &["check"], &mut fake);
    assert_eq!(clean.code, 0);
    assert_eq!(
        serde_json::from_str::<Value>(&clean.out).unwrap(),
        json!({"ok": true, "findings": []})
    );
    fake.report = CheckReport {
        findings: vec![Finding::Frontmatter {
            path: "/tree/bad.plan.md".into(),
            error: FrontError::Unclosed,
        }],
    };
    let found = run_as(Format::Json, &["check"], &mut fake);
    assert_eq!(found.code, 1);
    let doc: Value = serde_json::from_str(&found.out).unwrap();
    assert_eq!(doc["ok"], false);
    assert_eq!(
        doc["findings"][0]["code"],
        "plans-application-finding-frontmatter"
    );
    assert_eq!(doc["findings"][0]["args"]["path"], "/tree/bad.plan.md");
    let text = run_as(Format::Text, &["check"], &mut fake);
    assert_eq!(text.code, 1);
    assert!(text.out.contains("/tree/bad.plan.md"), "{}", text.out);
    assert_eq!(
        run_as(Format::Text, &["check"], &mut Fake::default()).out,
        "plans: no findings\n"
    );
}

#[test]
fn a_write_that_cannot_be_made_to_the_output_is_exit_1() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("closed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let cli = Cli::try_parse_from(["taskloom", "plans", "list"]).unwrap();
    let Group_::Plans(command) = cli.group;
    let settings = Settings {
        format: Format::Json,
        session: String::new(),
        prog: "taskloom",
    };
    let mut err = Vec::new();
    let code = ready(super::run(
        command,
        &settings,
        &catalog(),
        &mut Fake::default(),
        &mut Broken,
        &mut err,
    ));
    assert_eq!(code, 1);
    let envelope: Value = serde_json::from_slice(&err).unwrap();
    assert_eq!(envelope["code"], "plans-cli-output");
}

#[test]
fn errors_are_diagnostics() {
    assert_eq!(
        CliError::NoLink { command: "link" }.code(),
        "plans-cli-no-link"
    );
}
