//! RED: the plans use cases over fake ports: path rules, hash recording,
//! links checked against other streams, drift findings, and the import of
//! `ctxloom:plan` tags.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use angzarr_embedded_diagnostic::Report;

use super::*;
use crate::fakes::Service;

const ROOT: &str = "/tree";

/// A work tree in memory: path (relative, or absolute outside it) to text.
#[derive(Default)]
struct Files {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
}

impl Files {
    fn with(files: &[(&str, &str)]) -> Files {
        let fake = Files::default();
        for (path, text) in files {
            fake.put(path, text);
        }
        fake
    }
    fn put(&self, path: &str, text: &str) {
        self.files
            .borrow_mut()
            .insert(path.to_string(), text.as_bytes().to_vec());
    }
    fn remove(&self, path: &str) {
        self.files.borrow_mut().remove(path);
    }
    fn text(&self, path: &str) -> Option<String> {
        self.files
            .borrow()
            .get(path)
            .map(|b| String::from_utf8(b.clone()).unwrap())
    }
}

impl PlanFiles for Files {
    fn work_root(&self) -> Result<String, Report> {
        Ok(ROOT.to_string())
    }
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(path).cloned()
    }
    fn create(&self, path: &str, text: &[u8]) -> Result<bool, Report> {
        let mut files = self.files.borrow_mut();
        if files.contains_key(path) {
            return Ok(false);
        }
        files.insert(path.to_string(), text.to_vec());
        Ok(true)
    }
    fn plan_files(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{dir}/");
        self.files
            .borrow()
            .keys()
            .filter(|p| p.starts_with(&prefix) && p.ends_with(PLAN_SUFFIX))
            .filter(|p| !p[prefix.len()..].contains('/'))
            .cloned()
            .collect()
    }
}

/// A project's tasks: harp id to its (key, value) tags.
#[derive(Default)]
struct FakeTasks {
    tasks: RefCell<BTreeMap<String, BTreeSet<(String, String)>>>,
}

impl FakeTasks {
    fn with(tasks: &[(&str, &[&str])]) -> FakeTasks {
        let fake = FakeTasks::default();
        for (harp, plan_tags) in tasks {
            let tags = plan_tags
                .iter()
                .map(|v| (RETIRED_TAG.to_string(), v.to_string()))
                .collect();
            fake.tasks.borrow_mut().insert(harp.to_string(), tags);
        }
        fake
    }
    fn tags_of(&self, harp: &str) -> Vec<String> {
        self.tasks.borrow()[harp]
            .iter()
            .map(|(_, v)| v.clone())
            .collect()
    }
}

impl Tasks for &FakeTasks {
    async fn exists(&mut self, harp_id: &str) -> Result<bool, Report> {
        Ok(self.tasks.borrow().contains_key(harp_id))
    }
    async fn tagged(&mut self, key: &str) -> Result<Vec<(String, String)>, Report> {
        let tasks = self.tasks.borrow();
        Ok(tasks
            .iter()
            .flat_map(|(h, tags)| {
                tags.iter()
                    .filter(|(k, _)| k == key)
                    .map(move |(_, v)| (h.clone(), v.clone()))
            })
            .collect())
    }
    async fn untag(&mut self, harp_id: &str, key: &str, value: &str) -> Result<(), Report> {
        self.tasks
            .borrow_mut()
            .get_mut(harp_id)
            .unwrap()
            .remove(&(key.to_string(), value.to_string()));
        Ok(())
    }
}

/// Mints `plan-1`, `plan-2`, ...
#[derive(Default)]
struct Counter(RefCell<u32>);

impl Mint for Counter {
    fn mint(&self) -> String {
        *self.0.borrow_mut() += 1;
        format!("plan-{}", self.0.borrow())
    }
}

const TEXT: &str = "---\ntitle: Plans move into taskloom\nsessions:\n  - inept-sixth-stick\n  - calm-dusty-heron\n---\n# Plans\n";

fn new(title: &str) -> NewPlan {
    NewPlan {
        title: title.to_string(),
        ..NewPlan::default()
    }
}

// ---- pure rules ----

#[test]
fn resolve_path_keeps_a_normal_relative_path() {
    assert_eq!(
        resolve_path(ROOT, "docs/x.plan.md"),
        Ok("docs/x.plan.md".into())
    );
}

#[test]
fn resolve_path_normalizes_dot_and_dot_dot_inside_the_root() {
    assert_eq!(
        resolve_path(ROOT, "./docs/../plans//x.plan.md"),
        Ok("plans/x.plan.md".into())
    );
}

#[test]
fn resolve_path_makes_an_absolute_path_inside_the_root_relative() {
    assert_eq!(
        resolve_path(ROOT, "/tree/docs/x.plan.md"),
        Ok("docs/x.plan.md".into())
    );
}

#[test]
fn resolve_path_refuses_a_path_leaving_the_root() {
    for given in [
        "../x.plan.md",
        "docs/../../x.plan.md",
        "/elsewhere/x.plan.md",
        "/treehouse/x.plan.md",
    ] {
        assert_eq!(
            resolve_path(ROOT, given),
            Err(PathError::OutsideWorkRoot(given.into())),
            "{given}"
        );
    }
}

#[test]
fn resolve_path_refuses_a_file_that_is_not_a_plan_and_an_empty_path() {
    assert_eq!(
        resolve_path(ROOT, "docs/notes.md"),
        Err(PathError::NotAPlanFile("docs/notes.md".into()))
    );
    assert_eq!(resolve_path(ROOT, ""), Err(PathError::Empty));
}

#[test]
fn default_path_is_under_dot_taskloom_plans_named_by_harp_id() {
    assert_eq!(
        default_path("brisk-cedar-otter"),
        ".taskloom/plans/brisk-cedar-otter.plan.md"
    );
}

#[test]
fn content_hash_is_sha256_lowercase_hex() {
    assert_eq!(
        content_hash(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(content_hash(b"").len(), 64);
}

#[test]
fn frontmatter_reads_title_and_sessions_and_is_empty_without_one() {
    assert_eq!(
        frontmatter(TEXT),
        Frontmatter {
            title: "Plans move into taskloom".into(),
            sessions: vec!["inept-sixth-stick".into(), "calm-dusty-heron".into()],
        }
    );
    assert_eq!(frontmatter("# No frontmatter\n"), Frontmatter::default());
}

#[test]
fn plan_name_is_the_base_name_without_the_suffix() {
    assert_eq!(
        plan_name(".taskloom/plans/brisk-cedar-otter.plan.md"),
        "brisk-cedar-otter"
    );
    assert_eq!(plan_name("/home/b/s/v1-removal.plan.md"), "v1-removal");
}

#[test]
fn the_ctxloom_plan_tag_is_retired_and_no_other() {
    assert!(tag_is_retired("ctxloom:plan"));
    assert!(!tag_is_retired("ctxloom:session"));
    assert!(!tag_is_retired("plan"));
}

// ---- add ----

#[test]
fn add_with_no_path_writes_the_file_under_dot_taskloom_plans_and_records_its_hash() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    let plan = service.add(new("Plans move into taskloom")).unwrap();
    assert_eq!(plan.harp_id, "plan-1");
    assert_eq!(plan.path, ".taskloom/plans/plan-1.plan.md");
    assert_eq!(plan.abs_path, "/tree/.taskloom/plans/plan-1.plan.md");
    assert_eq!(plan.name, "plan-1");
    assert_eq!(plan.status, PlanStatus::Draft);
    let written = files
        .text(".taskloom/plans/plan-1.plan.md")
        .expect("the file is written");
    assert!(
        written.contains("title: Plans move into taskloom"),
        "{written}"
    );
    assert_eq!(plan.content_hash, content_hash(written.as_bytes()));
}

#[test]
fn add_with_no_path_refuses_to_overwrite_an_existing_file() {
    let files = Files::with(&[(".taskloom/plans/plan-1.plan.md", "mine")]);
    let (tasks, mint) = (FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    assert_eq!(
        service.add(new("T")),
        Err(PlanError::FileExists(
            ".taskloom/plans/plan-1.plan.md".into()
        ))
    );
    assert_eq!(
        files.text(".taskloom/plans/plan-1.plan.md").unwrap(),
        "mine"
    );
}

#[test]
fn add_with_a_path_registers_the_file_by_relative_path_and_hashes_it() {
    let files = Files::with(&[("docs/design.plan.md", TEXT)]);
    let (tasks, mint) = (FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    let plan = service
        .add(NewPlan {
            path: Some("/tree/docs/design.plan.md".into()),
            ..new("Design")
        })
        .unwrap();
    assert_eq!(plan.path, "docs/design.plan.md");
    assert_eq!(plan.content_hash, content_hash(TEXT.as_bytes()));
    assert_eq!(plan.sessions, vec!["inept-sixth-stick", "calm-dusty-heron"]);
}

#[test]
fn add_with_a_path_refuses_a_missing_file_a_path_outside_the_root_and_a_path_taken() {
    let files = Files::with(&[("docs/design.plan.md", TEXT)]);
    let (tasks, mint) = (FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    let at = |p: &str| NewPlan {
        path: Some(p.into()),
        ..new("T")
    };
    assert_eq!(
        service.add(at("docs/none.plan.md")),
        Err(PlanError::FileMissing("docs/none.plan.md".into()))
    );
    assert_eq!(
        service.add(at("../x.plan.md")),
        Err(PlanError::Path(PathError::OutsideWorkRoot(
            "../x.plan.md".into()
        )))
    );
    let first = service.add(at("docs/design.plan.md")).unwrap();
    assert_eq!(
        service.add(at("./docs/design.plan.md")),
        Err(PlanError::AlreadyRegistered {
            path: "docs/design.plan.md".into(),
            plan: first.harp_id
        })
    );
}

#[test]
fn add_records_the_session_and_its_governs_links() {
    let tasks = FakeTasks::with(&[("swift-amber-falcon", &[])]);
    let (files, mint) = (Files::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    let plan = service
        .add(NewPlan {
            governs: vec!["swift-amber-falcon".into()],
            session: "inept-sixth-stick".into(),
            ..new("T")
        })
        .unwrap();
    assert_eq!(plan.origin_session, "inept-sixth-stick");
    assert_eq!(plan.governs, vec!["swift-amber-falcon"]);
}

#[test]
fn add_governing_a_missing_task_is_refused_and_writes_no_file() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    let refused = service.add(NewPlan {
        governs: vec!["no-such-task".into()],
        ..new("T")
    });
    assert_eq!(refused, Err(PlanError::NoSuchTask("no-such-task".into())));
    assert!(files.files.borrow().is_empty());
}

// ---- links ----

fn service_with_two_plans<'a>(
    files: &'a Files,
    tasks: &'a FakeTasks,
    mint: &'a Counter,
) -> Service<&'a Files, &'a FakeTasks, &'a Counter> {
    let mut service = Service::new(files, tasks, mint);
    service.add(new("One")).unwrap();
    service.add(new("Two")).unwrap();
    service
}

#[test]
fn link_governs_refuses_a_task_the_project_does_not_hold() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = service_with_two_plans(&files, &tasks, &mint);
    assert_eq!(
        service.link("plan-1", Link::Governs("ghost".into())),
        Err(PlanError::NoSuchTask("ghost".into()))
    );
}

#[test]
fn link_to_or_from_a_missing_plan_is_refused() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = service_with_two_plans(&files, &tasks, &mint);
    assert_eq!(
        service.link("plan-1", Link::Supersedes("plan-9".into())),
        Err(PlanError::NoSuchPlan("plan-9".into()))
    );
    assert_eq!(
        service.link("plan-9", Link::Supersedes("plan-1".into())),
        Err(PlanError::NoSuchPlan("plan-9".into()))
    );
}

#[test]
fn governs_is_queryable_both_ways() {
    let tasks = FakeTasks::with(&[("task-a", &[]), ("task-b", &[])]);
    let (files, mint) = (Files::default(), Counter::default());
    let mut service = service_with_two_plans(&files, &tasks, &mint);
    service
        .link("plan-1", Link::Governs("task-a".into()))
        .unwrap();
    service
        .link("plan-2", Link::Governs("task-a".into()))
        .unwrap();
    service
        .link("plan-2", Link::Governs("task-b".into()))
        .unwrap();

    let governing = |task: &str| {
        service
            .list(&PlanQuery {
                governs: Some(task.into()),
                ..PlanQuery::default()
            })
            .into_iter()
            .map(|p| p.harp_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(governing("task-a"), vec!["plan-1", "plan-2"]);
    assert_eq!(governing("task-b"), vec!["plan-2"]);
    assert_eq!(
        service.show("plan-2").unwrap().plan.governs,
        vec!["task-a", "task-b"]
    );
}

#[test]
fn supersede_marks_the_target_superseded_and_unlink_reinstates_it() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = service_with_two_plans(&files, &tasks, &mint);
    service.set_status("plan-1", PlanStatus::Active).unwrap();
    let two = service
        .link("plan-2", Link::Supersedes("plan-1".into()))
        .unwrap();
    assert_eq!(two.supersedes, vec!["plan-1"]);
    let one = service.show("plan-1").unwrap().plan;
    assert_eq!(one.status, PlanStatus::Superseded);
    assert_eq!(one.superseded_by, "plan-2");

    service
        .unlink("plan-2", Link::Supersedes("plan-1".into()))
        .unwrap();
    let one = service.show("plan-1").unwrap().plan;
    assert_eq!(one.status, PlanStatus::Active);
    assert_eq!(one.superseded_by, "");
}

#[test]
fn a_supersede_closing_a_cycle_is_refused_directly_and_transitively() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = service_with_two_plans(&files, &tasks, &mint);
    service.add(new("Three")).unwrap();
    service
        .link("plan-2", Link::Supersedes("plan-1".into()))
        .unwrap();
    service
        .link("plan-3", Link::Supersedes("plan-2".into()))
        .unwrap();
    assert_eq!(
        service.link("plan-1", Link::Supersedes("plan-2".into())),
        Err(PlanError::SupersedeCycle {
            plan: "plan-1".into(),
            target: "plan-2".into()
        })
    );
    assert_eq!(
        service.link("plan-1", Link::Supersedes("plan-3".into())),
        Err(PlanError::SupersedeCycle {
            plan: "plan-1".into(),
            target: "plan-3".into()
        })
    );
}

#[test]
fn an_aggregate_refusal_comes_back_as_rejected() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = service_with_two_plans(&files, &tasks, &mint);
    assert_eq!(
        service.set_status("plan-1", PlanStatus::Done),
        Err(PlanError::Rejected(PlanRejection::Transition {
            from: PlanStatus::Draft,
            to: PlanStatus::Done
        }))
    );
}

// ---- list and show ----

#[test]
fn list_filters_by_status() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let mut service = service_with_two_plans(&files, &tasks, &mint);
    service.set_status("plan-2", PlanStatus::Active).unwrap();
    let active = service.list(&PlanQuery {
        statuses: vec![PlanStatus::Active],
        ..PlanQuery::default()
    });
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].harp_id, "plan-2");
    assert_eq!(service.list(&PlanQuery::default()).len(), 2);
}

#[test]
fn show_finds_a_plan_by_id_relative_path_or_absolute_path_and_returns_its_text() {
    let files = Files::with(&[("docs/design.plan.md", TEXT)]);
    let (tasks, mint) = (FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    service
        .add(NewPlan {
            path: Some("docs/design.plan.md".into()),
            ..new("Design")
        })
        .unwrap();
    for key in ["plan-1", "docs/design.plan.md", "/tree/docs/design.plan.md"] {
        let shown = service.show(key).unwrap();
        assert_eq!(shown.plan.harp_id, "plan-1", "{key}");
        assert_eq!(shown.content.as_deref(), Some(TEXT), "{key}");
    }
    assert_eq!(
        service.show("nope"),
        Err(PlanError::NoSuchPlan("nope".into()))
    );
}

#[test]
fn show_of_a_plan_whose_file_is_gone_has_no_content() {
    let files = Files::default();
    let (tasks, mint) = (FakeTasks::default(), Counter::default());
    let mut service = Service::new(&files, &tasks, &mint);
    let plan = service.add(new("T")).unwrap();
    files.remove(&plan.path);
    assert_eq!(service.show("plan-1").unwrap().content, None);
}

// ---- check: drift is a finding, never silent ----

#[test]
fn check_of_files_as_recorded_finds_nothing() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let service = service_with_two_plans(&files, &tasks, &mint);
    let report = service.check();
    assert_eq!(report.findings, vec![]);
    assert!(!report.failed());
}

#[test]
fn check_reports_a_missing_file_and_fails() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let service = service_with_two_plans(&files, &tasks, &mint);
    files.remove(".taskloom/plans/plan-1.plan.md");
    let report = service.check();
    assert_eq!(
        report.findings,
        vec![Finding::Missing {
            plan: "plan-1".into(),
            path: ".taskloom/plans/plan-1.plan.md".into()
        }]
    );
    assert!(report.failed());
}

/// Ruling (a), 2026-10-30: the hash tracks edits. `check` reads the file
/// and records its new hash, so an edit is no finding.
#[test]
fn check_records_an_edited_files_new_hash_and_finds_nothing() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let service = service_with_two_plans(&files, &tasks, &mint);
    let recorded = service.show("plan-2").unwrap().plan.content_hash;
    files.put(".taskloom/plans/plan-2.plan.md", "edited");
    let report = service.check();
    assert_eq!(report.findings, vec![]);
    assert!(!report.failed());
    let tracked = service.show("plan-2").unwrap().plan.content_hash;
    assert_eq!(tracked, content_hash(b"edited"));
    assert_ne!(tracked, recorded);
}

#[test]
fn check_reports_an_unregistered_plan_file_under_dot_taskloom_plans() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let service = service_with_two_plans(&files, &tasks, &mint);
    files.put(".taskloom/plans/stray.plan.md", "x");
    assert_eq!(
        service.check().findings,
        vec![Finding::Unregistered {
            path: ".taskloom/plans/stray.plan.md".into()
        }]
    );
}

/// Ruling (a): check's findings are only a missing file or an
/// unregistered one, so a governed task since pruned is none.
#[test]
fn check_does_not_report_a_governed_task_since_pruned() {
    let tasks = FakeTasks::with(&[("task-a", &[])]);
    let (files, mint) = (Files::default(), Counter::default());
    let mut service = service_with_two_plans(&files, &tasks, &mint);
    service
        .link("plan-1", Link::Governs("task-a".into()))
        .unwrap();
    tasks.tasks.borrow_mut().remove("task-a");
    assert_eq!(service.check().findings, vec![]);
}

/// Ruling (a): every finding fails check.
#[test]
fn any_finding_fails_check() {
    let report = CheckReport {
        findings: vec![Finding::Unregistered {
            path: ".taskloom/plans/x.plan.md".into(),
        }],
    };
    assert!(report.failed());
}

/// Ruling (a): no `--accept`: `list` and `show` read the file as it now is
/// and record its hash, as `check` does.
#[test]
fn list_and_show_record_the_files_hash_as_it_now_is() {
    let (files, tasks, mint) = (Files::default(), FakeTasks::default(), Counter::default());
    let service = service_with_two_plans(&files, &tasks, &mint);
    files.put(".taskloom/plans/plan-1.plan.md", "edited");
    let listed = service.list(&PlanQuery::default());
    assert_eq!(listed[0].content_hash, content_hash(b"edited"));
    files.put(".taskloom/plans/plan-1.plan.md", "edited again");
    let shown = service.show("plan-1").unwrap().plan;
    assert_eq!(shown.content_hash, content_hash(b"edited again"));
    assert_eq!(service.check().findings, vec![]);
}

// ---- import of ctxloom:plan tags ----

/// Tasks tagged with a work-tree plan (relative and absolute spellings of
/// one file), an external readable plan, a dead path and a non-plan file.
fn tagged_project() -> (Files, FakeTasks) {
    let files = Files::with(&[
        ("docs/design.plan.md", TEXT),
        ("/home/b/sessions/s1/v1-removal.plan.md", "# V1 removal\n"),
        ("docs/README.md", "not a plan"),
    ]);
    let tasks = FakeTasks::with(&[
        ("task-a", &["docs/design.plan.md"]),
        (
            "task-b",
            &[
                "/tree/docs/design.plan.md",
                "/home/b/sessions/s1/v1-removal.plan.md",
            ],
        ),
        ("task-c", &["/home/b/sessions/gone/old.plan.md"]),
        ("task-d", &["docs/README.md"]),
    ]);
    (files, tasks)
}

#[test]
fn import_turns_each_distinct_plan_path_into_one_plan_governing_its_tasks() {
    let (files, tasks) = tagged_project();
    let mint = Counter::default();
    let mut service = Service::new(&files, &tasks, &mint);
    let report = service.import(false);
    assert_eq!(
        report.plans,
        vec![
            ImportedPlan {
                harp_id: "plan-1".into(),
                path: "docs/design.plan.md".into(),
                external: false,
                new: true,
                governs: vec!["task-a".into(), "task-b".into()],
            },
            // Ruling (b): an external plan is copied in, and the copy
            // registered.
            ImportedPlan {
                harp_id: "plan-2".into(),
                path: ".taskloom/plans/plan-2.plan.md".into(),
                external: true,
                new: true,
                governs: vec!["task-b".into()],
            },
        ]
    );
    assert_eq!(
        files.text(".taskloom/plans/plan-2.plan.md").as_deref(),
        Some("# V1 removal\n"),
        "the copy holds the original's text"
    );
    assert_eq!(
        files
            .text("/home/b/sessions/s1/v1-removal.plan.md")
            .as_deref(),
        Some("# V1 removal\n"),
        "the original is left as it was"
    );
    let design = service.show("plan-1").unwrap().plan;
    assert_eq!(
        design.title, "Plans move into taskloom",
        "title from frontmatter"
    );
    assert_eq!(
        design.status,
        PlanStatus::Active,
        "an imported plan is in use"
    );
    assert_eq!(design.content_hash, content_hash(TEXT.as_bytes()));
    assert_eq!(
        service.show("plan-2").unwrap().plan.title,
        "V1 removal",
        "title from the heading"
    );
}

#[test]
fn import_reports_dead_paths_and_non_plan_files_and_keeps_their_tags() {
    let (files, tasks) = tagged_project();
    let mint = Counter::default();
    let mut service = Service::new(&files, &tasks, &mint);
    let report = service.import(false);
    assert_eq!(
        report.dead,
        vec![LeftTag {
            value: "/home/b/sessions/gone/old.plan.md".into(),
            tasks: vec!["task-c".into()]
        }]
    );
    assert_eq!(
        report.not_plans,
        vec![LeftTag {
            value: "docs/README.md".into(),
            tasks: vec!["task-d".into()]
        }]
    );
    assert_eq!(
        tasks.tags_of("task-c"),
        vec!["/home/b/sessions/gone/old.plan.md"]
    );
    assert_eq!(tasks.tags_of("task-d"), vec!["docs/README.md"]);
}

#[test]
fn import_retires_every_migrated_tag() {
    let (files, tasks) = tagged_project();
    let mint = Counter::default();
    let mut service = Service::new(&files, &tasks, &mint);
    let report = service.import(false);
    assert_eq!(
        report.retired,
        vec![
            ("task-a".into(), "docs/design.plan.md".into()),
            (
                "task-b".into(),
                "/home/b/sessions/s1/v1-removal.plan.md".into()
            ),
            ("task-b".into(), "/tree/docs/design.plan.md".into()),
        ]
    );
    assert!(tasks.tags_of("task-a").is_empty());
    assert!(tasks.tags_of("task-b").is_empty());
}

#[test]
fn import_dry_run_reports_the_same_and_changes_nothing() {
    let (files, tasks) = tagged_project();
    let mint = Counter::default();
    let mut service = Service::new(&files, &tasks, &mint);
    let report = service.import(true);
    assert_eq!(report.plans.len(), 2);
    assert_eq!(report.retired.len(), 3);
    assert!(service.list(&PlanQuery::default()).is_empty());
    assert_eq!(tasks.tags_of("task-a"), vec!["docs/design.plan.md"]);
}

#[test]
fn import_again_adds_no_plan_and_links_a_registered_path_as_not_new() {
    let (files, tasks) = tagged_project();
    let mint = Counter::default();
    let mut service = Service::new(&files, &tasks, &mint);
    service.import(false);
    // A task tagged later with a path already registered.
    tasks.tasks.borrow_mut().insert(
        "task-e".into(),
        [(RETIRED_TAG.to_string(), "docs/design.plan.md".to_string())].into(),
    );
    let again = service.import(false);
    assert_eq!(
        again.plans,
        vec![ImportedPlan {
            harp_id: "plan-1".into(),
            path: "docs/design.plan.md".into(),
            external: false,
            new: false,
            governs: vec!["task-a".into(), "task-b".into(), "task-e".into()],
        }]
    );
    assert_eq!(service.list(&PlanQuery::default()).len(), 2);
}

/// Ruling (b): an imported external plan is copied into the work tree,
/// so check finds nothing of it.
#[test]
fn an_imported_external_plan_is_copied_in_so_check_finds_nothing() {
    let (files, tasks) = tagged_project();
    let mint = Counter::default();
    let mut service = Service::new(&files, &tasks, &mint);
    service.import(false);
    assert_eq!(service.check().findings, vec![]);
    assert_eq!(
        service.show("plan-2").unwrap().plan.path,
        ".taskloom/plans/plan-2.plan.md"
    );
}
