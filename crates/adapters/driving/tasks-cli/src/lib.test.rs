//! The command line over a fake service: what each command asks the
//! application for, and how its answer is written.

use std::ffi::OsString;
use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::pin::pin;
use std::task::{Context as TaskContext, Poll, Waker};

use annals_diagnostic::Report;
use tasks_application::{
    AppError, Details, Finding, Hidden, ImportReport, ImportRequest, ImportService, LintReport,
    ListQuery, Listing, NamespacePermit, NewTask, PermitChange, PermitList, PermitService,
    Permitted, PriorityNote, PriorityWarning, Project, ProjectImport, PruneRequest, PruneService,
    Pruning, Reach, Row, Scope, ShowQuery, StatusChange, StatusInfo, Statuses, Summary, TagChange,
    TagCount, TagPermits, TagSchema, Task, TaskService, TextEdit, Violation, Vocabulary, Written,
};

use presentation::Catalog;

use super::{parse, run, run_import, Command, Context, Globals, ImportGroup};

mod shapes {
    use tasks_application::{Task, Timestamp};

    pub fn task(harp: &str, status: &str) -> Task {
        Task {
            harp_id: harp.to_string(),
            text: "Ship it".to_string(),
            status: status.to_string(),
            checked: false,
            text_hash: "0123456789ab".to_string(),
            trigger: String::new(),
            origin_session: String::new(),
            tags: Vec::new(),
            created_at: Timestamp::default(),
        }
    }
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut cx = TaskContext::from_waker(Waker::noop());
    match pin!(future).poll(&mut cx) {
        Poll::Ready(out) => out,
        Poll::Pending => panic!("the fake is ready at once"),
    }
}

fn catalog() -> Catalog {
    super::catalog(&[]).expect("the catalog loads")
}

fn project() -> Project {
    Project {
        id: "jolly-mute-magma".to_string(),
        root: Some("/work/tree".to_string()),
        store: PathBuf::new(),
    }
}

/// Answers scripted per use case; requests recorded.
#[derive(Default)]
struct Fake {
    added: Vec<NewTask>,
    tagged: Vec<TagChange>,
    moved: Vec<StatusChange>,
    edits: Vec<TextEdit>,
    shows: Vec<ShowQuery>,
    summed: Vec<Scope>,
    taxonomies: Vec<Scope>,
    counts: Vec<ListQuery>,
    linted: Vec<Scope>,
    report_of_lint: Option<Result<LintReport, AppError>>,
    vocabulary: Option<Result<Vocabulary, AppError>>,
    statuses: Option<Result<Statuses, AppError>>,
    summary: Option<Result<Summary, AppError>>,
    details: Option<Result<Details, AppError>>,
    listed: Vec<ListQuery>,
    written: Option<Result<Written, AppError>>,
    listing: Option<Result<Listing, AppError>>,
    prunes: Vec<PruneRequest>,
    pruning: Option<Result<Pruning, AppError>>,
    permits: Vec<PermitChange>,
    revokes: Vec<PermitChange>,
    permitted: Option<Result<Permitted, AppError>>,
    permit_list: Option<Result<PermitList, AppError>>,
    permit_lists: usize,
}

impl TaskService for Fake {
    async fn add(&mut self, task: NewTask) -> Result<Written, AppError> {
        self.added.push(task);
        self.written.clone().expect("an add answer scripted")
    }

    async fn tag(&mut self, change: TagChange) -> Result<Written, AppError> {
        self.tagged.push(change);
        self.written.clone().expect("a tag answer scripted")
    }

    async fn list(&mut self, query: ListQuery) -> Result<Listing, AppError> {
        self.listed.push(query);
        self.listing.clone().expect("a list answer scripted")
    }

    async fn set_status(&mut self, change: StatusChange) -> Result<Written, AppError> {
        self.moved.push(change);
        self.written.clone().expect("a status answer scripted")
    }

    async fn edit(&mut self, edit: TextEdit) -> Result<Written, AppError> {
        self.edits.push(edit);
        self.written.clone().expect("an edit answer scripted")
    }

    async fn show(&mut self, query: ShowQuery) -> Result<Details, AppError> {
        self.shows.push(query);
        self.details.clone().expect("a show answer scripted")
    }

    async fn summary(&mut self, scope: Scope) -> Result<Summary, AppError> {
        self.summed.push(scope);
        self.summary.clone().expect("a summary answer scripted")
    }

    async fn statuses(&mut self, scope: Scope) -> Result<Statuses, AppError> {
        self.taxonomies.push(scope);
        self.statuses.clone().expect("a statuses answer scripted")
    }

    async fn tags(&mut self, query: ListQuery) -> Result<Vocabulary, AppError> {
        self.counts.push(query);
        self.vocabulary.clone().expect("a tags answer scripted")
    }

    async fn lint(&mut self, scope: Scope) -> Result<LintReport, AppError> {
        self.linted.push(scope);
        self.report_of_lint.clone().expect("a lint answer scripted")
    }

    // The command line reads no schema through this; the MCP server does.
    async fn tag_schema(&mut self) -> Result<TagSchema, AppError> {
        unreachable!("no command reads the tag schema")
    }

    async fn permits(&mut self) -> Result<PermitList, AppError> {
        self.permit_lists += 1;
        self.permit_list.clone().expect("a permit list scripted")
    }
}

/// The importer: an import's answer scripted, its requests recorded. The
/// task commands' service is [`Fake`], which imports nothing.
#[derive(Default)]
struct FakeImporter {
    imports: Vec<ImportRequest>,
    report: Option<Result<ImportReport, AppError>>,
}

impl ImportService for FakeImporter {
    async fn import(&mut self, request: ImportRequest) -> Result<ImportReport, AppError> {
        self.imports.push(request);
        self.report.clone().expect("an import answer scripted")
    }
}

impl PruneService for Fake {
    async fn prune(&mut self, request: PruneRequest) -> Result<Pruning, AppError> {
        self.prunes.push(request);
        self.pruning.clone().expect("a prune answer scripted")
    }
}

impl PermitService for Fake {
    async fn permit(&mut self, change: PermitChange) -> Result<Permitted, AppError> {
        self.permits.push(change);
        self.permitted.clone().expect("a permit answer scripted")
    }

    async fn revoke(&mut self, change: PermitChange) -> Result<Permitted, AppError> {
        self.revokes.push(change);
        self.permitted.clone().expect("a revoke answer scripted")
    }
}

fn written(task: Task) -> Option<Result<Written, AppError>> {
    Some(Ok(Written {
        dead_letters: Vec::new(),
        shown: tasks_application::Shown::default(),
        project: project(),
        task,
    }))
}

struct Ran {
    code: u8,
    out: String,
    err: String,
}

fn run_on(fake: &mut Fake, terminal: bool, args: &[&str]) -> Ran {
    let context = Context {
        session: "cross-proc-harp".to_string(),
        stdout_is_terminal: terminal,
        version: "1.2.3".to_string(),
        lock_protocol: 2,
    };
    run_in(&context, fake, args)
}

/// The command line as a binary mounts these commands: the global flags
/// over the task commands and `import`.
#[derive(clap::Parser, Debug)]
#[command(name = super::PROG, about = "Manage the per-project task store")]
struct Cli {
    #[command(flatten)]
    globals: Globals,
    #[command(subcommand)]
    command: Line,
}

#[derive(clap::Subcommand, Debug)]
enum Line {
    #[command(flatten)]
    Tasks(Command),
    #[command(flatten)]
    Import(ImportGroup),
}

/// `args` (after the program's name) parsed as a binary parses them, and
/// answered by `answer`, or by the parser when they name no command.
fn ran(
    context: &Context,
    args: &[&str],
    answer: impl FnOnce(Line, Globals, &Catalog, &mut dyn Write, &mut dyn Write) -> u8,
) -> Ran {
    let args = std::iter::once("taskloom")
        .chain(args.iter().copied())
        .map(OsString::from);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let catalog = catalog();
    let code = match parse::<Cli>(args, context, &catalog, &mut out, &mut err) {
        Ok(cli) => answer(cli.command, cli.globals, &catalog, &mut out, &mut err),
        Err(code) => code,
    };
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

fn run_in(context: &Context, fake: &mut Fake, args: &[&str]) -> Ran {
    ran(context, args, |line, globals, catalog, out, err| {
        let Line::Tasks(command) = line else {
            panic!("not a task command: {args:?}")
        };
        ready(run(command, globals, context, catalog, fake, out, err))
    })
}

/// A task command `args` (the program's name first) parsed.
fn parsed(args: &[&str]) -> (Command, Globals) {
    let cli = <Cli as clap::Parser>::try_parse_from(args).expect("parsed");
    let Line::Tasks(command) = cli.command else {
        panic!("not a task command: {args:?}")
    };
    (command, cli.globals)
}

fn tagged_task() -> Task {
    let mut task = shapes::task("swift-amber", "To Do");
    task.tags = vec![tag("a"), tag("b")];
    task
}

// ------------------------------------------------------------ add

#[test]
fn test_add_joins_its_words_and_passes_its_flags_and_the_session() {
    let mut fake = Fake {
        written: written(shapes::task("swift-amber", "Deferred")),
        ..Fake::default()
    };
    let ran = run_on(
        &mut fake,
        true,
        &[
            "--project",
            "pinned",
            "add",
            "Ship",
            "it",
            "--status",
            "Deferred",
            "--trigger",
            "later",
            "--tag",
            "a",
            "--tag",
            "b",
            "--config-set",
            "tag_schema=x",
            "--config-set",
            "homing=home",
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        fake.added,
        [NewTask {
            project: Some("pinned".to_string()),
            text: "Ship it".to_string(),
            status: Some("Deferred".to_string()),
            trigger: Some("later".to_string()),
            tags: vec!["a".to_string(), "b".to_string()],
            origin_session: Some("cross-proc-harp".to_string()),
            request_id: None,
        }]
    );
    assert_eq!(ran.err, "taskloom: project /work/tree (jolly-mute-magma)\n");
    assert_eq!(ran.out, "swift-amber\tDeferred\tShip it\n");
}

// A pinned project the registry holds no path for has no root: the notice
// names it by its id alone, with no gap where a root would be.
#[test]
fn test_a_write_to_a_project_with_no_root_is_named_by_its_id() {
    let mut fake = Fake {
        written: Some(Ok(Written {
            dead_letters: Vec::new(),
            shown: tasks_application::Shown::default(),
            project: Project {
                root: None,
                ..project()
            },
            task: shapes::task("swift-amber", "To Do"),
        })),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["--project", "pinned", "add", "Ship it"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(ran.err, "taskloom: project jolly-mute-magma\n");
}

#[test]
fn test_add_with_no_flags_leaves_status_trigger_and_tags_out() {
    let mut fake = Fake {
        written: written(shapes::task("swift-amber", "To Do")),
        ..Fake::default()
    };
    run_on(&mut fake, true, &["add", "Ship it"]);
    let added = &fake.added[0];
    assert_eq!(added.project, None);
    assert_eq!(added.status, None);
    assert_eq!(added.trigger, None);
    assert!(added.tags.is_empty());
}

// A flag given empty is given: passed on as given, for the use case to
// refuse, never read as the flag left out.
#[test]
fn test_add_passes_a_flag_given_empty_on_as_given() {
    let mut fake = Fake {
        written: written(shapes::task("swift-amber", "To Do")),
        ..Fake::default()
    };
    run_on(
        &mut fake,
        true,
        &[
            "add",
            "Ship it",
            "--status",
            "",
            "--trigger",
            "",
            "--tag",
            "",
        ],
    );
    let added = &fake.added[0];
    assert_eq!(added.status.as_deref(), Some(""));
    assert_eq!(added.trigger.as_deref(), Some(""));
    assert_eq!(added.tags, [""]);
}

// CTXLOOM_SESSION_HARP set empty is unset, as an empty variable of the
// environment is: no session, never an empty one.
#[test]
fn test_add_in_an_empty_session_is_added_in_none() {
    let mut fake = Fake {
        written: written(shapes::task("swift-amber", "To Do")),
        ..Fake::default()
    };
    let context = Context {
        session: String::new(),
        ..Context::default()
    };
    run_in(&context, &mut fake, &["add", "Ship it"]);
    assert_eq!(fake.added[0].origin_session, None);
}

#[test]
fn test_off_a_terminal_add_answers_in_json() {
    let mut fake = Fake {
        written: written(shapes::task("swift-amber", "To Do")),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, false, &["add", "Ship it"]);
    let value: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(value["harp_id"], "swift-amber");
}

#[test]
fn test_an_explicit_format_wins_over_the_terminal() {
    let answer = || written(shapes::task("swift-amber", "To Do"));
    for (terminal, args) in [
        (true, &["--format", "json", "add", "x"][..]),
        (true, &["add", "x", "--json"][..]),
        (true, &["--json", "--format", "text", "add", "x"][..]),
    ] {
        let mut fake = Fake {
            written: answer(),
            ..Fake::default()
        };
        let ran = run_on(&mut fake, terminal, args);
        assert!(ran.out.starts_with('{'), "{args:?}: {}", ran.out);
    }
    let mut fake = Fake {
        written: answer(),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, false, &["--format", "text", "add", "x"]);
    assert_eq!(ran.out, "swift-amber\tTo Do\tShip it\n");
}

#[test]
fn test_add_needs_text() {
    let mut fake = Fake::default();
    let ran = run_on(&mut fake, true, &["add"]);
    assert_eq!(ran.code, 1);
    assert!(ran.err.contains("required"), "{}", ran.err);
    assert!(fake.added.is_empty());
}

#[test]
fn test_a_failed_add_says_why_on_stderr_and_exits_1() {
    let mut fake = Fake {
        written: Some(Err(AppError::HarpsExhausted { attempts: 100 })),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["add", "x"]);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.out, "");
    assert_eq!(ran.err, "taskloom: no free task id after 100 tries\n");
}

// ------------------------------------------------------------ tag

#[test]
fn test_tag_passes_its_adds_and_removes_and_answers_with_the_tags() {
    let mut fake = Fake {
        written: written(tagged_task()),
        ..Fake::default()
    };
    let ran = run_on(
        &mut fake,
        true,
        &[
            "tag",
            "swift-amber",
            "--add",
            "a",
            "--add",
            "b",
            "--remove",
            "c",
        ],
    );
    assert_eq!(ran.code, 0);
    assert_eq!(
        fake.tagged,
        [TagChange {
            project: None,
            harp_id: "swift-amber".to_string(),
            add: vec!["a".to_string(), "b".to_string()],
            remove: vec!["c".to_string()],
            request_id: None,
        }]
    );
    assert_eq!(ran.err, "taskloom: project /work/tree (jolly-mute-magma)\n");
    assert_eq!(ran.out, "swift-amber\tTo Do\ta,b\n");
}

#[test]
fn test_tag_in_json_is_the_task() {
    let mut fake = Fake {
        written: written(tagged_task()),
        ..Fake::default()
    };
    let ran = run_on(
        &mut fake,
        false,
        &["--project", "p", "tag", "swift-amber", "--add", "a"],
    );
    assert_eq!(fake.tagged[0].project.as_deref(), Some("p"));
    let value: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(value["tags"], serde_json::json!(["a", "b"]));
}

#[test]
fn test_a_failed_tag_exits_1() {
    let mut fake = Fake {
        written: Some(Err(AppError::Vanished {
            harp_id: "x".to_string(),
        })),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["tag", "x", "--add", "a"]);
    assert_eq!(ran.code, 1);
    assert!(ran.err.starts_with("taskloom: task x"), "{}", ran.err);
}

// ------------------------------------------------------------ status

#[test]
fn test_status_passes_its_id_status_and_trigger_and_answers_like_add() {
    let mut fake = Fake {
        written: written(shapes::task("swift-amber", "Deferred")),
        ..Fake::default()
    };
    let ran = run_on(
        &mut fake,
        true,
        &[
            "--project",
            "p",
            "status",
            "swift-amber",
            "Deferred",
            "--trigger",
            "the release",
            "--config-set",
            "tag_schema=x",
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        fake.moved,
        [StatusChange {
            project: Some("p".to_string()),
            harp_id: "swift-amber".to_string(),
            status: Some("Deferred".to_string()),
            trigger: Some("the release".to_string()),
            clear_trigger: false,
            request_id: None,
        }]
    );
    assert_eq!(ran.out, "swift-amber\tDeferred\tShip it\n");
    assert_eq!(ran.err, "taskloom: project /work/tree (jolly-mute-magma)\n");
    let ran = run_on(&mut fake, false, &["status", "swift-amber", "Done"]);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json["harp_id"], "swift-amber");
    assert_eq!(fake.moved[1].trigger, None, "no trigger given");
    run_on(
        &mut fake,
        true,
        &["status", "swift-amber", "Done", "--trigger", ""],
    );
    assert_eq!(fake.moved[2].trigger.as_deref(), Some(""), "given empty");
}

// A trigger alone defers the task; --clear-trigger clears it, alone or with
// a status, and is passed on beside a trigger for the use case to refuse.
#[test]
fn test_status_passes_a_trigger_alone_and_a_clearing_of_the_trigger() {
    let mut fake = Fake {
        written: written(shapes::task("swift-amber", "Deferred")),
        ..Fake::default()
    };
    let change = |status: Option<&str>, trigger: Option<&str>, clear_trigger| StatusChange {
        project: None,
        harp_id: "swift-amber".to_string(),
        status: status.map(str::to_string),
        trigger: trigger.map(str::to_string),
        clear_trigger,
        request_id: None,
    };
    for (args, expected) in [
        (
            &["status", "swift-amber", "--trigger", "the release"][..],
            change(None, Some("the release"), false),
        ),
        (
            &["status", "swift-amber", "--clear-trigger"][..],
            change(None, None, true),
        ),
        (
            &["status", "swift-amber", "Done", "--clear-trigger"][..],
            change(Some("Done"), None, true),
        ),
        (
            &["status", "swift-amber", "--trigger", "x", "--clear-trigger"][..],
            change(None, Some("x"), true),
        ),
    ] {
        let ran = run_on(&mut fake, true, args);
        assert_eq!(ran.code, 0, "{args:?}: {}", ran.err);
        assert_eq!(fake.moved.last(), Some(&expected), "{args:?}");
    }
}

#[test]
fn test_status_needs_an_id_and_a_change_and_says_why_it_failed() {
    let ran = run_on(&mut Fake::default(), true, &["status", "swift-amber"]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.contains("--trigger"),
        "names what it needs: {}",
        ran.err
    );
    let ran = run_on(&mut Fake::default(), true, &["status"]);
    assert_eq!(ran.code, 1);
    let mut fake = Fake {
        written: Some(Err(AppError::NoProject)),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["status", "swift-amber", "Done"]);
    assert_eq!(ran.code, 1);
    assert_eq!(
        ran.err,
        "taskloom: there is no project here yet, so no task to change\n"
    );
}

// ------------------------------------------------------------ edit

#[test]
fn test_edit_joins_its_words_and_answers_like_add() {
    let mut fake = Fake {
        written: written(shapes::task("swift-amber", "To Do")),
        ..Fake::default()
    };
    let ran = run_on(
        &mut fake,
        true,
        &[
            "--project",
            "p",
            "edit",
            "swift-amber",
            "Ship",
            "it",
            "--",
            "--now",
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        fake.edits,
        [TextEdit {
            project: Some("p".to_string()),
            harp_id: "swift-amber".to_string(),
            text: "Ship it --now".to_string(),
            request_id: None,
        }]
    );
    assert_eq!(ran.out, "swift-amber\tTo Do\tShip it\n");
    assert_eq!(ran.err, "taskloom: project /work/tree (jolly-mute-magma)\n");
    let ran = run_on(&mut fake, false, &["edit", "swift-amber", "x"]);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json["harp_id"], "swift-amber");
}

#[test]
fn test_edit_needs_text_and_says_why_it_failed() {
    let ran = run_on(&mut Fake::default(), true, &["edit", "swift-amber"]);
    assert_eq!(ran.code, 1);
    let mut fake = Fake {
        written: Some(Err(AppError::NoProject)),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["edit", "swift-amber", "x"]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.starts_with("taskloom: there is no project"),
        "{}",
        ran.err
    );
}

// ------------------------------------------------------------ show

fn details(reach: Reach, tasks: Vec<Task>) -> Option<Result<Details, AppError>> {
    let listing = listing_in(reach, tasks);
    Some(Ok(Details {
        reach: listing.reach,
        rows: listing.rows,
        shown: listing.shown,
    }))
}

#[test]
fn test_show_passes_its_ids_and_names_the_project_on_stderr() {
    let mut task = shapes::task("swift-amber", "Deferred");
    task.text = "Ship it\nwith every line".to_string();
    task.trigger = "the release".to_string();
    task.tags = vec![tag("a"), tag("b")];
    let mut fake = Fake {
        details: details(Reach::Project(project()), vec![task]),
        ..Fake::default()
    };
    let ran = run_on(
        &mut fake,
        true,
        &["--project", "p", "show", "swift-amber", "--global"],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        fake.shows,
        [ShowQuery {
            project: Some("p".to_string()),
            global: true,
            harp_ids: vec!["swift-amber".to_string()],
        }]
    );
    assert_eq!(
        ran.out,
        "[ ] swift-amber  Deferred\n    tags: a, b\n    trigger: the release\n\nShip it\nwith every line\n"
    );
    assert_eq!(ran.err, "taskloom: project /work/tree (jolly-mute-magma)\n");
}

#[test]
fn test_show_of_one_id_is_an_object_and_of_several_an_array() {
    let mut fake = Fake {
        details: details(
            Reach::Project(project()),
            vec![shapes::task("swift-amber", "To Do")],
        ),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, false, &["show", "swift-amber"]);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json["harp_id"], "swift-amber");
    assert_eq!(json["project_id"], "jolly-mute-magma");
    fake.details = details(
        Reach::Project(project()),
        vec![
            shapes::task("swift-amber", "To Do"),
            shapes::task("calm-otter", "To Do"),
        ],
    );
    let ran = run_on(&mut fake, false, &["show", "swift-amber", "calm-otter"]);
    let json: Vec<serde_json::Value> = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json[1]["harp_id"], "calm-otter");
}

#[test]
fn test_show_across_projects_heads_each_task_with_its_project() {
    let every = Reach::Every {
        projects: 1,
        fallback: None,
    };
    let mut fake = Fake {
        details: details(
            every,
            vec![
                shapes::task("swift-amber", "To Do"),
                shapes::task("calm-otter", "Done"),
            ],
        ),
        ..Fake::default()
    };
    let ran = run_on(
        &mut fake,
        true,
        &["show", "swift-amber", "calm-otter", "--global"],
    );
    assert_eq!(
        ran.out,
        "project: /work/tree (jolly-mute-magma)\n[ ] swift-amber  To Do\n\nShip it\n\n\
         project: /work/tree (jolly-mute-magma)\n[ ] calm-otter  Done\n\nShip it\n"
    );
    assert_eq!(ran.err, "");
}

#[test]
fn test_show_needs_an_id_and_says_which_were_not_found() {
    let ran = run_on(&mut Fake::default(), true, &["show"]);
    assert_eq!(ran.code, 1);
    let mut fake = Fake {
        details: Some(Err(AppError::NotFound {
            harp_ids: vec!["nope".to_string()],
        })),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["show", "nope"]);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.out, "");
    assert_eq!(
        ran.err,
        "taskloom: no task holds this harp id: \"nope\" (see `taskloom list`)\n"
    );
}

// ------------------------------------------------------------ summary

fn summary(reach: Reach) -> Option<Result<Summary, AppError>> {
    let rows = listing_in(
        reach.clone(),
        vec![shapes::task("swift-amber", "In Progress")],
    )
    .rows;
    Some(Ok(Summary {
        reach,
        counts: [("To Do".to_string(), 2), ("In Progress".to_string(), 1)]
            .into_iter()
            .collect(),
        in_progress: rows,
    }))
}

#[test]
fn test_summary_counts_each_status_then_names_those_in_progress() {
    let mut fake = Fake {
        summary: summary(Reach::Project(project())),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["--project", "p", "summary"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        fake.summed,
        [Scope {
            project: Some("p".to_string()),
            ..Scope::default()
        }]
    );
    assert_eq!(
        ran.out,
        "In Progress\t1\nTo Do\t2\n\nIn-progress: swift-amber\n"
    );
    assert_eq!(ran.err, "");
    let ran = run_on(&mut fake, false, &["summary"]);
    assert_eq!(
        ran.out,
        r#"{
  "counts": {
    "In Progress": 1,
    "To Do": 2
  },
  "in_progress": [
    {
      "harp_id": "swift-amber",
      "project": "/work/tree (jolly-mute-magma)"
    }
  ]
}
"#
    );
}

#[test]
fn test_summary_across_projects_names_each_task_with_its_project() {
    let mut fake = Fake {
        summary: summary(Reach::Every {
            projects: 2,
            fallback: None,
        }),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["summary", "--global"]);
    assert!(fake.summed[0].global);
    assert_eq!(
        ran.out,
        "Projects: 2 (--global)\n\nIn Progress\t1\nTo Do\t2\n\n\
         In-progress: swift-amber (/work/tree (jolly-mute-magma))\n"
    );
    let quiet = Summary {
        reach: Reach::NoProject,
        counts: Default::default(),
        in_progress: Vec::new(),
    };
    fake.summary = Some(Ok(quiet));
    let ran = run_on(&mut fake, true, &["summary"]);
    assert_eq!(ran.out, "");
    assert_eq!(ran.err, "taskloom: no project here yet: nothing to list\n");
    let ran = run_on(&mut fake, false, &["summary"]);
    assert_eq!(ran.out, "{\n  \"counts\": {},\n  \"in_progress\": []\n}\n");
}

// ------------------------------------------------------------ statuses

fn statuses(reach: Reach, names: &[&str]) -> Option<Result<Statuses, AppError>> {
    Some(Ok(Statuses {
        reach,
        statuses: names
            .iter()
            .enumerate()
            .map(|(order, name)| StatusInfo::of(name, order))
            .collect(),
    }))
}

#[test]
fn test_statuses_are_a_line_each_or_gos_array() {
    let names = ["In Progress", "Deferred", "Done", "Blocked"];
    let mut fake = Fake {
        statuses: statuses(Reach::NoProject, &names),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["--project", "p", "statuses", "--global"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        fake.taxonomies,
        [Scope {
            project: Some("p".to_string()),
            global: true,
        }]
    );
    assert_eq!(
        ran.out,
        "0\tIn Progress\n1\tDeferred\trequires-trigger\n2\tDone\tterminal\n3\tBlocked\n"
    );
    assert_eq!(ran.err, "", "no project: the defaults, nothing to say");
    let ran = run_on(&mut fake, false, &["statuses"]);
    let json: Vec<serde_json::Value> = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(
        json[1],
        serde_json::json!({"name": "Deferred", "order": 1, "terminal": false, "requires_trigger": true})
    );
    let keys: Vec<&String> = json[0].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["name", "order", "requires_trigger", "terminal"]);
}

#[test]
fn test_statuses_read_across_projects_for_want_of_one_say_so() {
    let every = Reach::Every {
        projects: 1,
        fallback: Some("/tmp/x".to_string()),
    };
    let mut fake = Fake {
        statuses: statuses(every, &["To Do"]),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["statuses"]);
    assert!(
        ran.err.starts_with("taskloom: no project here: /tmp/x"),
        "{}",
        ran.err
    );
    fake.statuses = statuses(Reach::Project(project()), &["To Do"]);
    let ran = run_on(&mut fake, true, &["statuses"]);
    assert_eq!(ran.err, "");
}

// ------------------------------------------------------------ tags

fn vocabulary(
    reach: Reach,
    counts: &[(&str, usize, usize)],
) -> Option<Result<Vocabulary, AppError>> {
    Some(Ok(Vocabulary {
        reach,
        tags: counts
            .iter()
            .map(|&(t, active, total)| TagCount {
                tag: tasks_application::parse_tag(t).unwrap().unwrap(),
                active,
                total,
            })
            .collect(),
    }))
}

#[test]
fn test_tags_passes_its_filters_and_aligns_its_counts() {
    let mut fake = Fake {
        vocabulary: vocabulary(
            Reach::Project(project()),
            &[("docs", 1, 2), ("triage:kind=chore", 10, 120)],
        ),
        ..Fake::default()
    };
    let ran = run_on(
        &mut fake,
        true,
        &[
            "--project",
            "p",
            "tags",
            "--status",
            "@open,Done",
            "--term",
            "ship",
            "--tag-query",
            "x",
            "--global",
            "--config-set",
            "tag_schema=x",
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        fake.counts,
        [ListQuery {
            project: Some("p".to_string()),
            statuses: vec!["@open".to_string(), "Done".to_string()],
            tag_query: Some("x".to_string()),
            term: Some("ship".to_string()),
            global: true,
            ..ListQuery::default()
        }]
    );
    assert_eq!(
        ran.out,
        "Project: /work/tree (jolly-mute-magma)\n\n\
         docs                 1 active    2 total\n\
         triage:kind=chore   10 active  120 total\n"
    );
    let ran = run_on(&mut fake, false, &["tags"]);
    let json: Vec<serde_json::Value> = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(
        json[1],
        serde_json::json!({"tag": "triage:kind=chore", "active": 10, "total": 120})
    );
}

#[test]
fn test_tags_with_none_in_use_says_how_to_apply_one() {
    let mut fake = Fake {
        vocabulary: vocabulary(Reach::NoProject, &[]),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["tags"]);
    assert_eq!(
        ran.out,
        "(no tags in use \u{2014} apply one with `taskloom tag <harp-id> --add <tag>`)\n"
    );
    assert_eq!(ran.err, "taskloom: no project here yet: nothing to list\n");
    assert_eq!(run_on(&mut fake, false, &["tags"]).out, "[]\n");
    fake.vocabulary = vocabulary(
        Reach::Every {
            projects: 3,
            fallback: None,
        },
        &[],
    );
    let ran = run_on(&mut fake, true, &["tags", "--global"]);
    assert!(
        ran.out.starts_with("Projects: 3 (--global)\n\n(no tags"),
        "{}",
        ran.out
    );
}

// ------------------------------------------------------------ lint

fn lint_report(reach: Reach, checked: usize, found: usize) -> Option<Result<LintReport, AppError>> {
    let violation = |harp: &str| Violation {
        project: Some("jolly-mute-magma".to_string()),
        harp_id: harp.to_string(),
        finding: Finding::NotInEnum {
            target: "area:x".to_string(),
            value: "c".to_string(),
            members: "a, b".to_string(),
        },
    };
    Some(Ok(LintReport {
        reach,
        violations: ["swift-amber", "calm-otter"][..found]
            .iter()
            .map(|h| violation(h))
            .collect(),
        checked_targets: checked,
    }))
}

#[test]
fn test_lint_says_each_violation_and_exits_1_for_any() {
    let mut fake = Fake {
        report_of_lint: lint_report(Reach::Project(project()), 3, 2),
        ..Fake::default()
    };
    let ran = run_on(
        &mut fake,
        true,
        &[
            "--project",
            "p",
            "lint",
            "--global",
            "--config-set",
            "tag_schema=x",
        ],
    );
    assert_eq!(ran.code, 1);
    assert_eq!(
        fake.linted,
        [Scope {
            project: Some("p".to_string()),
            global: true,
        }]
    );
    assert_eq!(
        ran.out,
        "swift-amber: area:x=c is not one of the declared enum values: a, b\n\
         calm-otter: area:x=c is not one of the declared enum values: a, b\n"
    );
    assert_eq!(ran.err, "taskloom: 2 tag-standard violations found\n");
    let ran = run_on(&mut fake, false, &["lint"]);
    assert_eq!(ran.code, 1);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json["checked_targets"], 3);
    let first = &json["violations"][0];
    assert_eq!(first["harp_id"], "swift-amber");
    assert_eq!(first["project_id"], "jolly-mute-magma");
    assert_eq!(
        first["reason"],
        "area:x=c is not one of the declared enum values: a, b"
    );
    assert_eq!(
        first["finding"]["code"],
        "tasks-application-lint-not-in-enum"
    );
    assert_eq!(first["finding"]["args"]["value"], "c");
    let envelope: serde_json::Value = serde_json::from_str(&ran.err).unwrap();
    assert_eq!(envelope["code"], "tasks-cli-lint-violations");
}

#[test]
fn test_lint_across_projects_names_each_violations_project() {
    let every = Reach::Every {
        projects: 2,
        fallback: None,
    };
    let mut fake = Fake {
        report_of_lint: lint_report(every, 3, 1),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["lint", "--global"]);
    assert_eq!(
        ran.out,
        "swift-amber (jolly-mute-magma): area:x=c is not one of the declared enum values: a, b\n"
    );
}

#[test]
fn test_a_clean_lint_exits_0_and_one_that_checked_nothing_exits_1() {
    let mut fake = Fake {
        report_of_lint: lint_report(Reach::Project(project()), 3, 0),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["lint"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(ran.out, "no tag-standard violations found\n");
    assert_eq!(ran.err, "");
    let ran = run_on(&mut fake, false, &["lint"]);
    assert_eq!(
        ran.out,
        "{\n  \"violations\": [],\n  \"checked_targets\": 3\n}\n"
    );
    fake.report_of_lint = lint_report(Reach::Project(project()), 0, 0);
    let ran = run_on(&mut fake, true, &["lint"]);
    assert_eq!(ran.code, 1);
    assert_eq!(
        ran.out,
        "0 targets checked: the tag schema declares no enum or range for lint to check\n"
    );
    assert!(ran.err.contains("lint checked no target"), "{}", ran.err);
    fake.report_of_lint = lint_report(Reach::Project(project()), 0, 1);
    let ran = run_on(&mut fake, true, &["lint"]);
    assert!(ran.out.starts_with("0 targets checked"), "{}", ran.out);
    assert!(
        ran.out.contains("swift-amber: area:x=c"),
        "both: {}",
        ran.out
    );
    assert!(ran.err.contains("lint checked no target"), "{}", ran.err);
}

// ------------------------------------------------------------ list

fn listing(tasks: Vec<Task>) -> Option<Result<Listing, AppError>> {
    Some(Ok(listing_in(Reach::Project(project()), tasks)))
}

/// A listing of `tasks`, all in [`project`], as reached by `reach`.
fn listing_in(reach: Reach, tasks: Vec<Task>) -> Listing {
    Listing {
        reach,
        rows: tasks
            .into_iter()
            .map(|task| Row {
                project: project(),
                task,
            })
            .collect(),
        shown: tasks_application::Shown::default(),
        hidden: Hidden::default(),
        searched: false,
        omitted: 0,
        priorities: None,
        priority_notes: Vec::new(),
    }
}

#[test]
fn test_list_passes_its_filters() {
    let mut fake = Fake {
        listing: listing(Vec::new()),
        ..Fake::default()
    };
    run_on(
        &mut fake,
        true,
        &[
            "--project",
            "p",
            "list",
            "--status",
            "Done,To Do",
            "--status",
            "Deferred",
            "--all",
            "--tag-query",
            "urgent/not",
            "--term",
            "docs",
            "--global",
            "--sort",
            "priority",
            "--limit",
            "5",
            "--compact",
        ],
    );
    assert_eq!(
        fake.listed,
        [ListQuery {
            project: Some("p".to_string()),
            statuses: vec![
                "Done".to_string(),
                "To Do".to_string(),
                "Deferred".to_string()
            ],
            all: true,
            tag_query: Some("urgent/not".to_string()),
            term: Some("docs".to_string()),
            global: true,
            limit: Some(5),
            sort: Some("priority".to_string()),
        }]
    );
    let mut fake = Fake {
        listing: listing(Vec::new()),
        ..Fake::default()
    };
    run_on(&mut fake, true, &["list"]);
    assert_eq!(fake.listed, [ListQuery::default()]);
}

// A filter given empty is given: passed on as given, for the use case to
// refuse, never read as the filter left out.
#[test]
fn test_list_tags_and_prune_pass_a_filter_given_empty_on_as_given() {
    let mut fake = Fake {
        listing: listing(Vec::new()),
        vocabulary: vocabulary(Reach::NoProject, &[]),
        pruning: Some(Ok(pruning(true, &[]))),
        ..Fake::default()
    };
    run_on(
        &mut fake,
        true,
        &["list", "--term", "", "--tag-query", "", "--sort", ""],
    );
    let given = Some(String::new());
    assert_eq!(fake.listed[0].term, given);
    assert_eq!(fake.listed[0].tag_query, given);
    assert_eq!(fake.listed[0].sort, given);
    run_on(&mut fake, true, &["tags", "--term", "", "--tag-query", ""]);
    assert_eq!(fake.counts[0].term, given);
    assert_eq!(fake.counts[0].tag_query, given);
    run_on(&mut fake, true, &["prune", "--tag-query", "", "--yes"]);
    assert_eq!(fake.prunes[0].tag_query, given);
}

// A limit of 0 is given: passed on as given, for the use case to refuse as
// blank, never read as no limit (which is the limit left out); the help
// says so.
#[test]
fn test_list_passes_a_limit_of_zero_on_as_given() {
    let mut fake = Fake {
        listing: listing(Vec::new()),
        ..Fake::default()
    };
    run_on(&mut fake, true, &["list", "--limit", "0"]);
    assert_eq!(fake.listed[0].limit, Some(0));
    let help = run_on(&mut Fake::default(), true, &["list", "--help"]);
    assert!(
        help.out.contains("leave it out for no limit; 0 is refused"),
        "{}",
        help.out
    );
}

#[test]
fn test_list_as_text_names_the_project_then_the_table() {
    let mut fake = Fake {
        listing: listing(vec![shapes::task("swift-amber", "To Do")]),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["list"]);
    assert_eq!(ran.code, 0);
    assert_eq!(
        ran.out,
        "Project: /work/tree (jolly-mute-magma)\n\n[ ] swift-amber  To Do        Ship it\n"
    );
    assert_eq!(ran.err, "");
}

#[test]
fn test_list_as_json_is_an_array_of_tasks() {
    let mut fake = Fake {
        listing: listing(vec![
            shapes::task("swift-amber", "To Do"),
            shapes::task("calm-otter", "To Do"),
        ]),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, false, &["list"]);
    let value: Vec<serde_json::Value> = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(value.len(), 2);
    assert_eq!(value[1]["harp_id"], "calm-otter");
}

#[test]
fn test_list_where_there_is_no_project_says_so_and_lists_nothing() {
    for (terminal, shown) in [(true, "(no tasks)\n"), (false, "[]\n")] {
        let mut fake = Fake {
            listing: Some(Ok(listing_in(Reach::NoProject, Vec::new()))),
            ..Fake::default()
        };
        let ran = run_on(&mut fake, terminal, &["list"]);
        assert_eq!(ran.code, 0);
        assert_eq!(ran.out, shown);
        assert_eq!(ran.err, "taskloom: no project here yet: nothing to list\n");
    }
}

#[test]
fn test_list_across_projects_is_headed_by_their_count_and_each_project() {
    let every = Reach::Every {
        projects: 2,
        fallback: None,
    };
    let mut fake = Fake {
        listing: Some(Ok(listing_in(
            every,
            vec![shapes::task("swift-amber", "To Do")],
        ))),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["list", "--global"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        ran.out,
        "Projects: 2 (--global)\n\n\
         Project: /work/tree (jolly-mute-magma)\n\
         [ ] swift-amber  To Do        Ship it\n\n"
    );
    assert_eq!(ran.err, "", "an asked-for --global says nothing");
    let ran = run_on(&mut fake, false, &["list", "--global"]);
    let rows: Vec<serde_json::Value> = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(rows[0]["harp_id"], "swift-amber");
    assert_eq!(rows[0]["created_at"], "1970-01-01T00:00:00Z");
    assert_eq!(rows[0]["project_id"], "jolly-mute-magma");
    assert_eq!(rows[0]["project_dir"], "/work/tree");
}

#[test]
fn test_list_across_projects_for_want_of_one_says_why_on_stderr() {
    let every = Reach::Every {
        projects: 0,
        fallback: Some("/tmp/x".to_string()),
    };
    let mut fake = Fake {
        listing: Some(Ok(listing_in(every, Vec::new()))),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["list"]);
    assert_eq!(ran.out, "Projects: 0 (--global)\n\n(no tasks)\n");
    assert!(
        ran.err
            .starts_with("taskloom: no project here: /tmp/x is in no git repository"),
        "{}",
        ran.err
    );
}

#[test]
fn test_list_compact_is_each_tasks_headline_in_json() {
    let mut task = shapes::task("swift-amber", "Done");
    task.checked = true;
    task.text = "First line\nmore".to_string();
    task.tags = vec![tag("a")];
    let mut fake = Fake {
        listing: Some(Ok(listing_in(Reach::Project(project()), vec![task]))),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, false, &["list", "--compact"]);
    assert_eq!(
        ran.out,
        r#"[
  {
    "harp_id": "swift-amber",
    "status": "Done",
    "checked": true,
    "tags": [
      "a"
    ],
    "headline": "First line"
  }
]
"#
    );
    fake.listing = Some(Ok(listing_in(
        Reach::Every {
            projects: 1,
            fallback: None,
        },
        vec![shapes::task("calm-otter", "To Do")],
    )));
    let ran = run_on(&mut fake, false, &["list", "--compact", "--global"]);
    let rows: Vec<serde_json::Value> = serde_json::from_str(&ran.out).unwrap();
    let keys: Vec<&String> = rows[0].as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "checked",
            "harp_id",
            "headline",
            "project_dir",
            "project_id",
            "status",
            "tags"
        ],
        "the compact row and its project"
    );
    let ran = run_on(&mut fake, true, &["list", "--compact"]);
    assert!(
        ran.out.contains("[ ] calm-otter"),
        "text ignores it: {}",
        ran.out
    );
}

#[test]
fn test_list_warns_on_stderr_when_its_priority_ranking_means_little() {
    let mut sorted = listing_in(Reach::Project(project()), Vec::new());
    sorted.priorities = Some(Vec::new());
    sorted.priority_notes = vec![PriorityNote {
        project: Some("p".to_string()),
        warning: PriorityWarning::AllTied { scored: 0, live: 2 },
    }];
    sorted.omitted = 4;
    let mut fake = Fake {
        listing: Some(Ok(sorted)),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["list", "--sort", "priority"]);
    assert_eq!(
        ran.err,
        "taskloom: project p: --sort priority is meaningless here: every active task ties at the same raw priority score (only 0 of 2 active tasks carry a tag any priority_fn/decay_fn formula actually reads) \u{2014} the ranking reflects nothing\n\
         taskloom: 4 more tasks omitted by --limit: raise or drop it to see them (summary's counts are unaffected)\n"
    );
}

#[test]
fn test_list_says_on_stderr_what_a_search_hid_and_a_limit_left_out() {
    let mut searched = listing_in(Reach::Project(project()), Vec::new());
    searched.searched = true;
    searched.hidden = Hidden {
        completed: 2,
        deferred: 1,
    };
    searched.omitted = 4;
    let mut fake = Fake {
        listing: Some(Ok(searched.clone())),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["list", "--term", "x"]);
    assert_eq!(
        ran.err,
        "taskloom: 3 more matching tasks hidden by the default view (2 completed, 1 deferred): add --all to include them\n\
         taskloom: 4 more tasks omitted by --limit: raise or drop it to see them (summary's counts are unaffected)\n"
    );
    searched.searched = false;
    searched.omitted = 0;
    fake.listing = Some(Ok(searched.clone()));
    let ran = run_on(&mut fake, true, &["list"]);
    assert_eq!(ran.err, "", "an unsearched listing hides by design");
    searched.searched = true;
    searched.hidden = Hidden::default();
    fake.listing = Some(Ok(searched));
    let ran = run_on(&mut fake, true, &["list", "--term", "x"]);
    assert_eq!(ran.err, "", "a search that hid nothing says nothing");
}

#[test]
fn test_a_failed_list_exits_1() {
    let mut fake = Fake {
        listing: Some(Err(AppError::HarpsExhausted { attempts: 1 })),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["list"]);
    assert_eq!(ran.code, 1);
    assert!(ran.err.starts_with("taskloom: "), "{}", ran.err);
}

// ------------------------------------------------------------ usage

#[test]
fn test_help_is_written_to_stdout_and_exits_0() {
    let ran = run_on(&mut Fake::default(), true, &["--help"]);
    assert_eq!(ran.code, 0);
    assert!(ran.out.contains("Usage: taskloom"), "{}", ran.out);
    assert_eq!(ran.err, "");
}

#[test]
fn test_an_unknown_command_is_a_usage_error_exiting_1() {
    let ran = run_on(&mut Fake::default(), true, &["frobnicate"]);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.out, "");
    assert!(ran.err.contains("frobnicate"), "{}", ran.err);
}

// ------------------------------------------------------------ output

/// A writer that refuses everything.
struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "closed",
        ))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn test_output_that_cannot_be_written_fails_with_why() {
    let context = Context {
        stdout_is_terminal: true,
        ..Context::default()
    };
    for args in [&["taskloom", "add", "x"][..], &["taskloom", "list"][..]] {
        let mut fake = Fake {
            written: written(shapes::task("swift-amber", "To Do")),
            listing: listing(Vec::new()),
            ..Fake::default()
        };
        let mut err = Vec::new();
        let (command, globals) = parsed(args);
        let code = ready(run(
            command,
            globals,
            &context,
            &catalog(),
            &mut fake,
            &mut Broken,
            &mut err,
        ));
        assert_eq!(code, 1);
        assert_eq!(
            String::from_utf8(err).unwrap().lines().last(),
            Some("taskloom: cannot write the output: closed"),
            "{args:?}"
        );
    }
    let mut fake = Fake {
        listing: listing(Vec::new()),
        ..Fake::default()
    };
    let mut err = Vec::new();
    let (command, globals) = parsed(&["taskloom", "--json", "list"]);
    let code = ready(run(
        command,
        globals,
        &context,
        &catalog(),
        &mut fake,
        &mut Broken,
        &mut err,
    ));
    assert_eq!(code, 1);
    let envelope: serde_json::Value = serde_json::from_slice(&err).unwrap();
    assert_eq!(envelope["code"], "tasks-cli-output");
    assert_eq!(envelope["message"], "cannot write the output: closed");
}

// ------------------------------------------------------------ errors

#[test]
fn test_a_text_error_is_its_catalog_message_not_its_english() {
    let mut fake = Fake {
        written: Some(Err(AppError::Rejected(Report::new(
            "tasks-text-required",
            "the English fallback",
        )))),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["add", "x"]);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.err, "taskloom: a task needs text\n");
}

#[test]
fn test_a_json_error_is_an_envelope_on_stderr_with_its_cause_nested() {
    let mut fake = Fake {
        written: Some(Err(AppError::GaveUp {
            attempts: 10,
            error: Report::new(
                "annals-store-sql-unavailable",
                "the database failed: locked",
            )
            .arg("detail", "locked"),
        })),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, false, &["add", "x"]);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.out, "", "stdout holds answers only");
    let envelope: serde_json::Value = serde_json::from_str(&ran.err).unwrap();
    assert_eq!(
        envelope,
        serde_json::json!({
            "code": "tasks-application-gave-up",
            "args": {
                "attempts": 10,
                "error": {
                    "code": "annals-store-sql-unavailable",
                    "args": {"detail": "locked"},
                    "message": "the database failed: locked",
                },
            },
            "message": "gave up after 10 attempts: the database failed: locked",
        })
    );
}

#[test]
fn test_a_usage_error_in_json_is_an_envelope() {
    for (terminal, args) in [
        (false, &["frobnicate"][..]),
        (true, &["--json", "frobnicate"][..]),
        (true, &["--format", "json", "frobnicate"][..]),
        (true, &["--format=json", "frobnicate"][..]),
        (true, &["--format", "json", "--format", "yaml", "list"][..]),
    ] {
        let ran = run_on(&mut Fake::default(), terminal, args);
        assert_eq!(ran.code, 1, "{args:?}");
        assert_eq!(ran.out, "", "{args:?}");
        let envelope: serde_json::Value =
            serde_json::from_str(&ran.err).unwrap_or_else(|e| panic!("{args:?}: {e}: {}", ran.err));
        assert_eq!(envelope["code"], "tasks-cli-usage", "{args:?}");
        assert_eq!(envelope["message"], envelope["args"]["detail"], "{args:?}");
    }
}

#[test]
fn test_a_usage_error_in_text_is_clap_s_own_words() {
    for args in [
        &["frobnicate"][..],
        &["--format", "text", "--json-ish"][..],
        &["--format=text", "frobnicate"][..],
        &["frobnicate", "--", "--json"][..],
        &["frobnicate", "--format"][..],
    ] {
        let ran = run_on(&mut Fake::default(), true, args);
        assert_eq!(ran.code, 1, "{args:?}");
        assert!(ran.err.starts_with("error: "), "{args:?}: {}", ran.err);
    }
}

// ------------------------------------------------------------ import

fn imported(lost: usize) -> ProjectImport {
    ProjectImport {
        project_id: "jolly-mute-magma".to_string(),
        records: 3,
        tasks: 1,
        applied: 3,
        lost: (0..lost)
            .map(|line| tasks_application::Lost {
                line: u32::try_from(line).unwrap(),
                op: "status",
                harp_id: "swift-amber".to_string(),
                text: String::new(),
                why: tasks_application::Why::TaskNotImported,
            })
            .collect(),
        ..ProjectImport::default()
    }
}

fn report(projects: Vec<ProjectImport>) -> Option<Result<ImportReport, AppError>> {
    Some(Ok(ImportReport {
        projects,
        dead_letters: Vec::new(),
    }))
}

/// `args` run as the binary runs them: `import` mounted beside the task
/// commands, its importer reached only once `import` is parsed.
fn import_on(importer: &mut FakeImporter, terminal: bool, args: &[&str]) -> Ran {
    let context = Context {
        stdout_is_terminal: terminal,
        ..Context::default()
    };
    ran(&context, args, |line, globals, catalog, out, err| {
        let Line::Import(ImportGroup::Import) = line else {
            panic!("not import: {args:?}")
        };
        match globals.format(&context, catalog, err) {
            Ok(format) => ready(run_import(
                globals.project,
                format,
                catalog,
                importer,
                out,
                err,
            )),
            Err(code) => code,
        }
    })
}

#[test]
fn test_import_imports_every_project_and_says_what_it_did() {
    let mut fake = FakeImporter {
        report: report(vec![imported(0)]),
        ..FakeImporter::default()
    };
    let ran = import_on(&mut fake, true, &["import"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(fake.imports, [ImportRequest { project: None }]);
    assert_eq!(
        ran.out,
        "jolly-mute-magma: 3 records, 1 tasks added, 3 records imported\n"
    );
    assert_eq!(ran.err, "");
}

#[test]
fn test_import_of_one_project_names_it() {
    let mut fake = FakeImporter {
        report: report(vec![imported(0)]),
        ..FakeImporter::default()
    };
    import_on(
        &mut fake,
        true,
        &["import", "--project", "jolly-mute-magma"],
    );
    assert_eq!(
        fake.imports,
        [ImportRequest {
            project: Some("jolly-mute-magma".to_string())
        }]
    );
}

#[test]
fn test_import_answers_json_off_a_terminal() {
    let mut fake = FakeImporter {
        report: report(vec![imported(0)]),
        ..FakeImporter::default()
    };
    let ran = import_on(&mut fake, false, &["import"]);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json["complete"], true);
    assert_eq!(json["projects"][0]["project_id"], "jolly-mute-magma");
}

#[test]
fn test_import_that_left_records_behind_fails_after_saying_which() {
    let mut fake = FakeImporter {
        report: report(vec![imported(2)]),
        ..FakeImporter::default()
    };
    let ran = import_on(&mut fake, true, &["import"]);
    assert_eq!(ran.code, 1);
    assert!(ran.out.contains("not imported: line 1"), "{}", ran.out);
    assert_eq!(
        ran.err,
        "taskloom: not everything was imported: 0 projects stopped, 2 records not imported\n"
    );
}

#[test]
fn test_import_that_stopped_in_a_project_fails() {
    let mut stopped = imported(0);
    stopped.failed = Some(annals_diagnostic::Report::new("x-bad", "bad log"));
    let mut fake = FakeImporter {
        report: report(vec![stopped, imported(0)]),
        ..FakeImporter::default()
    };
    let ran = import_on(&mut fake, true, &["import"]);
    assert_eq!(ran.code, 1);
    assert_eq!(
        ran.err,
        "taskloom: not everything was imported: 1 projects stopped, 0 records not imported\n"
    );
}

#[test]
fn test_import_that_cannot_start_fails_with_why() {
    let mut fake = FakeImporter {
        report: Some(Err(AppError::NoProject)),
        ..FakeImporter::default()
    };
    let ran = import_on(&mut fake, true, &["import"]);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.out, "");
    assert!(
        ran.err.starts_with("taskloom: there is no project"),
        "{}",
        ran.err
    );
}

// ------------------------------------------------------------ companion

/// The reference loadout document (`fixtures/go/loadout.yaml`, which
/// `scripts/go-companion-fixtures.sh` copies from its source), with the
/// reviewed factual changes applied.
#[path = "../fixtures/reviewed.rs"]
mod reviewed;

#[test]
fn test_version_in_json_is_gos_shape_with_the_lock_protocol() {
    let ran = run_on(&mut Fake::default(), true, &["version", "--format", "json"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        ran.out,
        "{\n  \"name\": \"taskloom\",\n  \"version\": \"1.2.3\",\n  \"lock_protocol\": 2\n}\n"
    );
    assert_eq!(ran.err, "");
}

#[test]
fn test_version_in_text_is_the_bare_version() {
    let ran = run_on(&mut Fake::default(), true, &["version"]);
    assert_eq!((ran.code, ran.out.as_str()), (0, "1.2.3\n"));
}

#[test]
fn test_a_version_with_no_lock_protocol_leaves_it_out_as_go_does() {
    let context = Context {
        version: "1.2.3".to_string(),
        ..Context::default()
    };
    let ran = run_in(&context, &mut Fake::default(), &["version", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"name": "taskloom", "version": "1.2.3"})
    );
}

#[test]
fn test_the_loadout_is_gos_reviewed_byte_for_byte() {
    let reviewed = reviewed::loadout();
    for args in [&["loadout"][..], &["loadout", "--format", "yaml"]] {
        let ran = run_on(&mut Fake::default(), false, args);
        assert_eq!(ran.code, 0, "{args:?}: {}", ran.err);
        assert!(
            ran.out == reviewed,
            "{args:?}: the loadout differs from Go's, reviewed"
        );
        assert_eq!(ran.err, "");
    }
}

#[test]
fn test_the_loadout_in_another_format_is_refused_and_prints_nothing() {
    for (args, asked) in [
        (&["loadout", "--json"][..], "json"),
        (&["loadout", "--format", "json"], "json"),
        (&["--format", "text", "loadout"], "text"),
    ] {
        let ran = run_on(&mut Fake::default(), true, args);
        assert_eq!(ran.code, 1, "{args:?}");
        assert_eq!(ran.out, "", "{args:?}");
        assert!(
            ran.err.contains(&format!("not {asked}")),
            "{args:?}: {}",
            ran.err
        );
    }
}

#[test]
fn test_yaml_is_only_the_loadout_s() {
    for args in [
        &["list", "--format", "yaml"][..],
        &["version", "--format", "yaml"],
    ] {
        let ran = run_on(&mut Fake::default(), true, args);
        assert_eq!(ran.code, 1, "{args:?}");
        assert_eq!(ran.out, "", "{args:?}");
        assert_eq!(
            ran.err, "taskloom: the format yaml is not one of text or json\n",
            "{args:?}"
        );
    }
}

fn tag(key: &str) -> tasks_application::Tag {
    tasks_application::Tag {
        namespace: None,
        key: key.to_string(),
        value: None,
    }
}

// ------------------------------------------------------------ prune

fn pruning(applied: bool, taken: &[&str]) -> Pruning {
    Pruning {
        dead_letters: Vec::new(),
        project: Some(project()),
        statuses: vec!["Done".to_string()],
        before: tasks_application::Timestamp {
            seconds: 1_767_225_600,
            nanos: 0,
        },
        tag_query: None,
        applied,
        pruned: taken
            .iter()
            .map(|harp| tasks_application::Pruned {
                task: shapes::task(harp, "Done"),
                status_set_at: tasks_application::Timestamp::default(),
            })
            .collect(),
        events_removed: 2 * taken.len() as u64,
        tasks_remaining: 1,
        dangling: vec![tasks_application::Dangling {
            harp_id: "keeper".to_string(),
            pruned: "swift-amber".to_string(),
            via: tasks_application::Via::Tag("relates:swift-amber".to_string()),
        }],
        shown: tasks_application::Shown::default(),
    }
}

#[test]
fn test_prune_by_default_reports_done_tasks_older_than_seven_days() {
    let mut fake = Fake {
        pruning: Some(Ok(pruning(false, &[]))),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["prune"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        fake.prunes,
        [PruneRequest {
            project: None,
            statuses: vec!["Done".to_string()],
            older_than: "7d".to_string(),
            tag_query: None,
            apply: false,
        }]
    );
    assert_eq!(ran.err, "", "nothing would be taken: nothing to say");
}

#[test]
fn test_prune_passes_its_flags() {
    let mut fake = Fake {
        pruning: Some(Ok(pruning(true, &["swift-amber"]))),
        ..Fake::default()
    };
    let ran = run_on(
        &mut fake,
        true,
        &[
            "--project",
            "pinned",
            "prune",
            "--older-than",
            "30d",
            "--status",
            "Done,Archived",
            "--status",
            "To Do",
            "--tag-query",
            "release",
            "--yes",
            "--config-set",
            "tag_schema=x",
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        fake.prunes,
        [PruneRequest {
            project: Some("pinned".to_string()),
            statuses: vec![
                "Done".to_string(),
                "Archived".to_string(),
                "To Do".to_string()
            ],
            older_than: "30d".to_string(),
            tag_query: Some("release".to_string()),
            apply: true,
        }]
    );
    assert_eq!(ran.err, "", "applied: no report-only notice");
    assert!(
        ran.out
            .starts_with("Project: /work/tree (jolly-mute-magma)\n"),
        "{}",
        ran.out
    );
}

#[test]
fn test_a_prune_report_that_would_take_something_says_how_to_apply_it() {
    let mut fake = Fake {
        pruning: Some(Ok(pruning(false, &["swift-amber"]))),
        ..Fake::default()
    };
    let ran = run_on(
        &mut fake,
        true,
        &["prune", "--status", "To Do", "--tag-query", "level<=2"],
    );
    assert_eq!(ran.code, 0);
    assert_eq!(
        ran.err,
        "taskloom: removed nothing: this was a report, not a removal; to apply exactly this plan, \
         run it again with --yes: taskloom prune --older-than 7d --status 'To Do' --tag-query 'level<=2' --yes\n"
    );
}

#[test]
fn test_off_a_terminal_a_prune_is_one_json_document_and_its_notices_are_on_stderr() {
    let mut fake = Fake {
        pruning: Some(Ok(pruning(false, &["swift-amber"]))),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, false, &["prune"]);
    let value: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(value["pruned"][0]["harp_id"], "swift-amber");
    assert_eq!(
        value["dangling_references"][0]["via"],
        "tag relates:swift-amber"
    );
    assert!(ran.err.contains("removed nothing"), "{}", ran.err);
}

#[test]
fn test_a_prune_where_there_is_no_project_says_so_on_stderr() {
    let mut answer = pruning(false, &[]);
    answer.project = None;
    answer.dangling.clear();
    let mut fake = Fake {
        pruning: Some(Ok(answer)),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["prune"]);
    assert_eq!(ran.code, 0);
    assert_eq!(ran.err, "taskloom: no project here yet: nothing to prune\n");
    assert_eq!(
        ran.out,
        "Nothing to prune (status Done, last status change before 2026-01-01 00:00 UTC).\n"
    );
}

#[test]
fn test_a_refused_prune_says_why_and_exits_1() {
    let refusal = annals_diagnostic::Report::new(
        "tasks-application-prune-age-unreadable",
        "the age soon is unreadable",
    )
    .arg("age", "soon");
    let mut fake = Fake {
        pruning: Some(Err(AppError::Prune(refusal))),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["prune", "--older-than", "soon"]);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.out, "");
    assert_eq!(
        ran.err,
        "taskloom: the age soon is neither an offset (30d, 12w, 720h) nor a date (2026-01-01)\n",
        "the catalog's message"
    );
}

// ------------------------------------------------------------ dead letters

fn dead_letters() -> Vec<tasks_application::DeadLetter> {
    vec![tasks_application::DeadLetter {
        line: "DEAD LETTER (a bug): x".to_string(),
    }]
}

// A write's dead letters are said on stderr, after the project, in text and
// JSON alike: stdout is the task as ever.
#[test]
fn test_a_writes_dead_letters_are_said_on_stderr() {
    for terminal in [true, false] {
        let mut written = written(shapes::task("swift-amber", "To Do"));
        if let Some(Ok(w)) = &mut written {
            w.dead_letters = dead_letters();
        }
        let mut fake = Fake {
            written,
            ..Fake::default()
        };
        let ran = run_on(&mut fake, terminal, &["add", "Ship it"]);
        assert_eq!(ran.code, 0, "{}", ran.err);
        assert_eq!(
            ran.err,
            "taskloom: project /work/tree (jolly-mute-magma)\ntaskloom: DEAD LETTER (a bug): x\n"
        );
        assert!(!ran.out.contains("DEAD LETTER"), "{}", ran.out);
    }
}

// A failed write's dead letters are said on stderr before its error, in
// text and JSON alike; the JSON error envelope is the failure's as ever.
#[test]
fn test_a_failed_writes_dead_letters_are_said_on_stderr_before_its_error() {
    let failed = AppError::DeadLettered {
        error: Box::new(AppError::NoProject),
        dead_letters: dead_letters(),
    };
    for (terminal, error) in [
        (
            true,
            "taskloom: there is no project here yet, so no task to change\n",
        ),
        (false, "\"code\": \"tasks-application-no-project\""),
    ] {
        let mut fake = Fake {
            written: Some(Err(failed.clone())),
            ..Fake::default()
        };
        let ran = run_on(&mut fake, terminal, &["edit", "swift-amber", "x"]);
        assert_eq!(ran.code, 1);
        let (letter, rest) = ran.err.split_once('\n').unwrap();
        assert_eq!(letter, "taskloom: DEAD LETTER (a bug): x");
        assert!(rest.contains(error), "{}", ran.err);
        assert!(!rest.contains("DEAD LETTER"), "{}", ran.err);
    }
}

#[test]
fn test_a_prunes_dead_letters_are_said_on_stderr() {
    let mut pruning = pruning(false, &[]);
    pruning.dead_letters = dead_letters();
    let mut fake = Fake {
        pruning: Some(Ok(pruning)),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, false, &["prune"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(ran.err, "taskloom: DEAD LETTER (a bug): x\n");
}

#[test]
fn test_an_imports_dead_letters_are_said_on_stderr() {
    let mut report = report(vec![imported(0)]);
    if let Some(Ok(r)) = &mut report {
        r.dead_letters = dead_letters();
    }
    let mut fake = FakeImporter {
        report,
        ..FakeImporter::default()
    };
    let ran = import_on(&mut fake, true, &["import"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(ran.err, "taskloom: DEAD LETTER (a bug): x\n");
}

// ------------------------------------------------------------ permit, revoke

fn permitted(permit: &str, changed: bool) -> Option<Result<Permitted, AppError>> {
    Some(Ok(Permitted {
        permit: permit.to_string(),
        changed,
    }))
}

fn change(permit: &str) -> PermitChange {
    PermitChange {
        permit: permit.to_string(),
    }
}

#[test]
fn test_permit_passes_its_permit_on_and_says_what_changed() {
    let mut fake = Fake {
        permitted: permitted("area:cli", true),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["permit", "area:cli"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(fake.permits, [change("area:cli")]);
    assert_eq!(ran.out, "permitted area:cli\n");
    let mut fake = Fake {
        permitted: permitted("area:cli", false),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["permit", "area:cli"]);
    assert_eq!(ran.out, "area:cli was already permitted\n");
    let ran = run_on(&mut fake, false, &["permit", "area:cli"]);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"permit": "area:cli", "changed": false})
    );
}

#[test]
fn test_revoke_passes_its_permit_on_and_says_so() {
    let mut fake = Fake {
        permitted: permitted("area:", true),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["revoke", "area:"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(fake.revokes, [change("area:")]);
    assert!(fake.permits.is_empty());
    assert_eq!(ran.out, "revoked area:\n");
}

// Ruling 4, as a person sees it: the refusal names every task still
// carrying the permit, and the command fails.
#[test]
fn test_a_refused_revoke_names_the_tasks_still_carrying_it() {
    let mut fake = Fake {
        permitted: Some(Err(AppError::PermitInUse {
            permit: "area:cli".to_string(),
            harp_ids: vec!["calm-otter".to_string(), "swift-amber".to_string()],
        })),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["revoke", "area:cli"]);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.out, "");
    assert_eq!(
        ran.err,
        "taskloom: area:cli cannot be revoked: these tasks still carry it: \"calm-otter\", \"swift-amber\" (retag them first)\n"
    );
}

// ------------------------------------------------------------ permits

/// `area:` closed to cli and mcp, `relates:` open and the flat `urgent`,
/// with two schema targets.
fn listed(permits: Option<TagPermits>) -> Option<Result<PermitList, AppError>> {
    Some(Ok(PermitList {
        permits,
        schema_targets: vec!["triage:kind".to_string(), "triage:level".to_string()],
    }))
}

fn usual_permits() -> TagPermits {
    TagPermits {
        namespaces: [
            (
                "area".to_string(),
                NamespacePermit::Closed(["cli", "mcp"].map(String::from).into()),
            ),
            ("relates".to_string(), NamespacePermit::Open),
        ]
        .into(),
        flat: ["urgent".to_string()].into(),
    }
}

// Ruling 9: a person reads what the project permits, one line a
// namespace (its keys, or that it is open), then the flat tags and the
// targets the schema permits without a permit.
#[test]
fn test_permits_lists_the_namespaces_flat_tags_and_schema_targets() {
    let mut fake = Fake {
        permit_list: listed(Some(usual_permits())),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["permits"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(fake.permit_lists, 1);
    assert_eq!(
        ran.out,
        "Namespaces:\n  area:     cli, mcp\n  relates:  (open: any key)\nFlat tags: urgent\nPermitted by the tag schema: triage:kind, triage:level\n"
    );
    let mut fake = Fake {
        permit_list: listed(Some(TagPermits::default())),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["permits"]);
    assert_eq!(
        ran.out,
        "Namespaces: (none)\nFlat tags: (none)\nPermitted by the tag schema: triage:kind, triage:level\n"
    );
}

// Opt-in (ruling 6), as a person reads it: no permits declared is not
// "nothing permitted", and the listing says which, and how to start.
#[test]
fn test_permits_where_none_are_declared_says_every_tag_is_permitted() {
    let mut fake = Fake {
        permit_list: listed(None),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["permits"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        ran.out,
        "No tag permits: this project declares none, so every tag the tag schema admits is permitted (`taskloom permit <permit>` starts them).\nPermitted by the tag schema: triage:kind, triage:level\n"
    );
}

// In JSON, the taskloom://tag-permits resource's document
// (tasks-presentation's PermitsJson), so one jq filter reads both.
#[test]
fn test_permits_in_json_is_the_resources_document() {
    let mut fake = Fake {
        permit_list: listed(Some(usual_permits())),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, false, &["permits"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "declared": true,
            "namespaces": [
                {"namespace": "area", "open": false, "keys": ["cli", "mcp"]},
                {"namespace": "relates", "open": true, "keys": []},
            ],
            "tags": ["urgent"],
            "schema_targets": ["triage:kind", "triage:level"],
        })
    );
    let mut fake = Fake {
        permit_list: listed(None),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, false, &["permits"]);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json["declared"], false);
    assert_eq!(json["namespaces"], serde_json::json!([]));
}

// A namespace closed to no key (a person's `area: []`) permits none of
// its keys, and the listing says so rather than leave the column blank.
#[test]
fn test_permits_says_a_namespace_closed_to_no_key_permits_none() {
    let mut fake = Fake {
        permit_list: listed(Some(TagPermits {
            namespaces: [(
                "area".to_string(),
                NamespacePermit::Closed(Default::default()),
            )]
            .into(),
            flat: Default::default(),
        })),
        ..Fake::default()
    };
    let ran = run_on(&mut fake, true, &["permits"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        ran.out,
        "Namespaces:\n  area:  (no key)\nFlat tags: (none)\nPermitted by the tag schema: triage:kind, triage:level\n"
    );
}

// `--request-id <id>` on every write names the request's key, passed on
// as given (a blank one, and any on an add until fix O, for the use case
// to refuse); left out, none. Nothing on the command line mints one.
#[test]
fn test_a_write_passes_its_request_id_on_as_given() {
    let mut fake = Fake {
        written: written(shapes::task("swift-amber", "To Do")),
        ..Fake::default()
    };
    for args in [
        &["add", "Ship it", "--request-id", "r-1"][..],
        &["status", "swift-amber", "Done", "--request-id", "r-2"],
        &["tag", "swift-amber", "--add", "x", "--request-id", "r-3"],
        &["edit", "swift-amber", "New", "--request-id", ""],
        &["add", "Ship it"],
        &["status", "swift-amber", "Done"],
        &["tag", "swift-amber", "--add", "x"],
        &["edit", "swift-amber", "New"],
    ] {
        let ran = run_on(&mut fake, true, args);
        assert_eq!(ran.code, 0, "{args:?}: {}", ran.err);
    }
    let added: Vec<_> = fake.added.iter().map(|t| t.request_id.as_deref()).collect();
    assert_eq!(added, [Some("r-1"), None]);
    let moved: Vec<_> = fake.moved.iter().map(|c| c.request_id.as_deref()).collect();
    assert_eq!(moved, [Some("r-2"), None]);
    let tagged: Vec<_> = fake
        .tagged
        .iter()
        .map(|c| c.request_id.as_deref())
        .collect();
    assert_eq!(tagged, [Some("r-3"), None]);
    let edited: Vec<_> = fake.edits.iter().map(|e| e.request_id.as_deref()).collect();
    assert_eq!(edited, [Some(""), None]);
}

// The help says what a request id does, and that an add's is refused
// until fix O.
#[test]
fn test_the_help_says_what_a_request_id_does_and_that_an_adds_is_not_yet() {
    for write in ["status", "tag", "edit"] {
        let help = run_on(&mut Fake::default(), true, &[write, "--help"]);
        assert!(
            help.out
                .contains("resend the same one when retrying it, and a request already applied is not applied again"),
            "{write}: {}",
            help.out
        );
    }
    let help = run_on(&mut Fake::default(), true, &["add", "--help"]);
    assert!(
        help.out.contains("Not yet: refused until fix O"),
        "{}",
        help.out
    );
}
