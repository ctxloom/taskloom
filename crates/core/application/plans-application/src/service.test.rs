//! The plans use cases over in-memory plan files, tasks and rules.

use angzarr_diagnostic::{code, Diagnostic};

use super::*;
use crate::fakes::{ready, words, MemFiles, MemTasks, Roots, Words};

type Fake = PlanService<MemFiles, MemTasks, Words, Roots>;

fn text(id: &str, extra: &str) -> String {
    format!("---\nid: {id}\n{extra}---\n# {id}\n")
}

fn service(files: &[(&str, &str)]) -> Fake {
    PlanService::new(
        MemFiles::with(files),
        MemTasks {
            tasks: vec!["swift-amber-falcon", "calm-otter-dune"],
            ..MemTasks::default()
        },
        words(&["first-minted-id", "second-minted-id"]),
        Roots(None),
    )
}

fn names(plans: &[Plan]) -> Vec<&str> {
    plans.iter().map(|p| p.name.as_str()).collect()
}

fn listed(service: &mut Fake, query: &PlanQuery) -> Vec<Plan> {
    ready(service.list(query)).unwrap()
}

fn code(error: PlanError) -> String {
    error.code().to_string()
}

#[test]
fn the_default_roots_are_the_work_tree_and_taskloom_plans_each_file_once() {
    let mut s = service(&[
        ("/tree/docs/in-tree.plan.md", &text("aa-bb", "")),
        ("/tree/.taskloom/plans/in-plans.plan.md", &text("cc-dd", "")),
        ("/tree/docs/readme.md", "not a plan"),
        ("/elsewhere/outside.plan.md", &text("ee-ff", "")),
    ]);
    let all = listed(&mut s, &PlanQuery::default());
    assert_eq!(names(&all), ["in-plans", "in-tree"]);
}

#[test]
fn configured_roots_replace_the_defaults_and_a_relative_one_is_the_work_roots() {
    let files = [
        ("/tree/docs/a.plan.md", text("aa-bb", "")),
        ("/tree/other/b.plan.md", text("cc-dd", "")),
        ("/far/c.plan.md", text("ee-ff", "")),
    ];
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    for (roots, expected) in [
        (vec!["docs"], vec!["a"]),
        (vec!["./docs/../other"], vec!["b"]),
        (vec!["/far"], vec!["c"]),
        (vec!["docs", "/far"], vec!["c", "a"]), // by path
        (vec![], vec![]),
    ] {
        let mut s = service(&borrowed);
        s.config = Roots(Some(roots.iter().map(|r| r.to_string()).collect()));
        let found = listed(&mut s, &PlanQuery::default());
        assert_eq!(names(&found), expected, "{roots:?}");
    }
}

#[test]
fn list_filters_by_status_governs_and_tags_and_refuses_what_is_none() {
    let mut s = service(&[
        (
            "/tree/a.plan.md",
            &text(
                "aa-bb",
                "status: active\ntags: [area:cli]\ngoverns: [swift-amber-falcon]\n",
            ),
        ),
        (
            "/tree/b.plan.md",
            &text(
                "cc-dd",
                "tags: [human:decision]\ngoverns: [swift-amber-falcon, calm-otter-dune]\n",
            ),
        ),
        (
            "/tree/c.plan.md",
            &text(
                "ee-ff",
                "status: done\ntags: [area:cli]\nsupersedes: [cc-dd]\n",
            ),
        ),
    ]);
    let mut by = |statuses: &[&str], governs: Option<&str>, tag_query: Option<&str>| {
        let query = PlanQuery {
            statuses: statuses.iter().map(|s| s.to_string()).collect(),
            governs: governs.map(str::to_string),
            tag_query: tag_query.map(str::to_string),
        };
        names(&listed(&mut s, &query))
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(by(&["active"], None, None), ["a"]);
    assert_eq!(by(&["superseded"], None, None), ["b"], "derived");
    assert_eq!(by(&["active", "done"], None, None), ["a", "c"]);
    assert_eq!(by(&[], Some("swift-amber-falcon"), None), ["a", "b"]);
    assert_eq!(by(&[], Some("calm-otter-dune"), None), ["b"]);
    assert_eq!(by(&[], None, Some("area:cli")), ["a", "c"]);
    assert_eq!(
        by(&[], None, Some("area:cli/human:decision/or")),
        ["a", "b", "c"]
    );
    assert_eq!(by(&[], Some("swift-amber-falcon"), Some("area:cli")), ["a"]);
    assert_eq!(by(&[], None, Some("area:cli/not")), ["b"]);

    let bad = PlanQuery {
        statuses: vec!["nonsense".into()],
        ..PlanQuery::default()
    };
    assert_eq!(
        code(ready(s.list(&bad)).unwrap_err()),
        code!("unknown-status")
    );
    let malformed = PlanQuery {
        tag_query: Some("and".into()),
        ..PlanQuery::default()
    };
    let error = ready(s.list(&malformed)).unwrap_err();
    assert_eq!(code(error), code!("tag-query"));
}

#[test]
fn the_rules_are_asked_for_only_when_a_tag_query_needs_them() {
    let mut s = service(&[("/tree/a.plan.md", &text("aa-bb", ""))]);
    listed(&mut s, &PlanQuery::default());
    assert_eq!(s.tasks.asked, 0);
    listed(
        &mut s,
        &PlanQuery {
            tag_query: Some("area:cli".into()),
            ..PlanQuery::default()
        },
    );
    assert_eq!(s.tasks.asked, 1);
}

#[test]
fn show_takes_an_id_or_a_path_and_a_file_outside_the_roots_by_its_path() {
    let mut s = service(&[
        ("/tree/docs/a.plan.md", &text("aa-bb", "")),
        ("/tree/docs/b.plan.md", &text("aa-bb", "")),
        ("/tree/docs/c.plan.md", &text("cc-dd", "title: C\n")),
        ("/elsewhere/d.plan.md", &text("ee-ff", "")),
    ]);
    let by_id = ready(s.show("cc-dd")).unwrap();
    assert_eq!(by_id.plan.title, "C");
    assert_eq!(by_id.content, text("cc-dd", "title: C\n"));
    let by_path = ready(s.show("docs/c.plan.md")).unwrap();
    assert_eq!(by_path.plan.path, "/tree/docs/c.plan.md");
    let absolute = ready(s.show("/tree/docs/../docs/c.plan.md")).unwrap();
    assert_eq!(absolute.plan.id.as_deref(), Some("cc-dd"));
    let outside = ready(s.show("/elsewhere/d.plan.md")).unwrap();
    assert_eq!(outside.plan.id.as_deref(), Some("ee-ff"));

    let ambiguous = ready(s.show("aa-bb")).unwrap_err();
    assert_eq!(code(ambiguous.clone()), code!("ambiguous"));
    assert!(
        ambiguous.to_string().contains("/tree/docs/b.plan.md"),
        "{ambiguous}"
    );
    assert_eq!(
        code(ready(s.show("nope")).unwrap_err()),
        code!("no-such-plan")
    );
    assert_eq!(
        code(ready(s.show("docs/missing.plan.md")).unwrap_err()),
        code!("no-such-plan")
    );
}

#[test]
fn show_of_a_superseded_plan_derives_the_status() {
    let mut s = service(&[
        ("/tree/a.plan.md", &text("aa-bb", "status: active\n")),
        ("/tree/b.plan.md", &text("cc-dd", "supersedes: [aa-bb]\n")),
        (
            "/elsewhere/c.plan.md",
            &text("ee-ff", "supersedes: [cc-dd]\n"),
        ),
    ]);
    assert_eq!(ready(s.show("aa-bb")).unwrap().plan.status, "superseded");
    assert_eq!(ready(s.show("cc-dd")).unwrap().plan.status, "draft");
}

#[test]
fn set_writes_only_the_asked_scalars_and_mints_an_id_on_a_first_write() {
    let mut s = service(&[("/tree/a.plan.md", "---\ntitle: Old\n---\n# Body\n")]);
    let plan = ready(s.set(
        "a.plan.md",
        SetPlan {
            title: Some("New".into()),
            status: Some("active".into()),
        },
    ))
    .unwrap();
    assert_eq!(plan.id.as_deref(), Some("first-minted-id"));
    assert_eq!(plan.title, "New");
    assert_eq!(plan.status, "active");
    assert_eq!(
        s.files.text("/tree/a.plan.md"),
        "---\ntitle: New\nid: first-minted-id\nstatus: active\n---\n# Body\n"
    );
    let again = ready(s.set(
        "first-minted-id",
        SetPlan {
            status: Some("done".into()),
            ..SetPlan::default()
        },
    ))
    .unwrap();
    assert_eq!(
        again.id.as_deref(),
        Some("first-minted-id"),
        "the id is kept"
    );
}

#[test]
fn set_refuses_what_is_invalid_before_it_looks_at_any_file() {
    let mut s = service(&[]);
    let set = |title: Option<&str>, status: Option<&str>| SetPlan {
        title: title.map(str::to_string),
        status: status.map(str::to_string),
    };
    let refused = |s: &mut Fake, change| code(ready(s.set("no-such", change)).unwrap_err());
    assert_eq!(refused(&mut s, set(None, None)), code!("no-change"));
    assert_eq!(refused(&mut s, set(Some("  "), None)), code!("blank-title"));
    assert_eq!(
        refused(&mut s, set(None, Some("superseded"))),
        code!("hand-superseded")
    );
    assert_eq!(
        refused(&mut s, set(None, Some("nonsense"))),
        code!("unknown-status")
    );
    assert_eq!(refused(&mut s, set(Some("T"), None)), code!("no-such-plan"));
}

#[test]
fn a_file_that_cannot_be_edited_is_left_alone() {
    let broken = "---\ntitle: [unclosed\n---\n# B\n";
    let mut s = service(&[("/tree/b.plan.md", broken)]);
    let error = ready(s.set(
        "b.plan.md",
        SetPlan {
            status: Some("active".into()),
            ..SetPlan::default()
        },
    ))
    .unwrap_err();
    assert_eq!(code(error), code!("frontmatter"));
    assert_eq!(s.files.text("/tree/b.plan.md"), broken);
}

#[test]
fn a_file_that_cannot_be_written_is_the_files_report() {
    let mut s = service(&[("/tree/a.plan.md", &text("aa-bb", ""))]);
    s.files.deny.borrow_mut().push("/tree/a.plan.md".into());
    let error = ready(s.set(
        "aa-bb",
        SetPlan {
            title: Some("T".into()),
            ..SetPlan::default()
        },
    ))
    .unwrap_err();
    assert_eq!(code(error), "x-denied");
}

#[test]
fn add_writes_a_new_file_with_a_minted_id_under_the_first_root() {
    let mut s = service(&[]);
    let plan = ready(s.add(NewPlan {
        title: "Fresh: plan".into(),
        governs: vec!["swift-amber-falcon".into()],
        ..NewPlan::default()
    }))
    .unwrap();
    assert_eq!(plan.id.as_deref(), Some("first-minted-id"));
    assert_eq!(plan.path, "/tree/.taskloom/plans/first-minted-id.plan.md");
    assert_eq!(plan.status, "draft");
    assert_eq!(plan.governs, ["swift-amber-falcon"]);
    assert_eq!(
        s.files.text(&plan.path),
        "---\nid: first-minted-id\ntitle: \"Fresh: plan\"\nstatus: draft\ngoverns:\n  - swift-amber-falcon\n---\n\n# Fresh: plan\n"
    );

    s.config = Roots(Some(vec!["docs".into(), "/far".into()]));
    let there = ready(s.add(NewPlan {
        title: "T".into(),
        status: Some("active".into()),
        ..NewPlan::default()
    }))
    .unwrap();
    assert_eq!(there.path, "/tree/docs/second-minted-id.plan.md");
    assert_eq!(there.status, "active");
    let named = ready(s.add(NewPlan {
        title: "T".into(),
        path: Some("notes/mine.plan.md".into()),
        ..NewPlan::default()
    }))
    .unwrap();
    assert_eq!(named.path, "/tree/notes/mine.plan.md");
}

#[test]
fn add_refuses_a_bad_request_and_a_taken_path_and_never_mints_a_taken_id() {
    let mut s = service(&[("/tree/x.plan.md", &text("first-minted-id", ""))]);
    let add = |s: &mut Fake, new| code(ready(s.add(new)).unwrap_err());
    let titled = |title: &str| NewPlan {
        title: title.into(),
        ..NewPlan::default()
    };
    assert_eq!(add(&mut s, titled(" ")), code!("blank-title"));
    assert_eq!(
        add(
            &mut s,
            NewPlan {
                status: Some("superseded".into()),
                ..titled("T")
            }
        ),
        code!("hand-superseded")
    );
    assert_eq!(
        add(
            &mut s,
            NewPlan {
                path: Some("x.md".into()),
                ..titled("T")
            }
        ),
        code!("not-a-plan-path")
    );
    assert_eq!(
        add(
            &mut s,
            NewPlan {
                governs: vec!["ghost-task".into()],
                ..titled("T")
            }
        ),
        code!("no-such-task")
    );
    assert_eq!(
        add(
            &mut s,
            NewPlan {
                path: Some("x.plan.md".into()),
                ..titled("T")
            }
        ),
        code!("file-exists")
    );
    // The first minted id is in use, so the second is taken; the refused
    // adds above minted what they minted.
    let mut s = service(&[("/tree/x.plan.md", &text("first-minted-id", ""))]);
    let plan = ready(s.add(titled("T"))).unwrap();
    assert_eq!(plan.id.as_deref(), Some("second-minted-id"));
}

#[test]
fn ids_that_are_all_taken_exhaust_the_mint() {
    let mut s = service(&[("/tree/x.plan.md", &text("last-one-left", ""))]);
    s.mint = words(&[]);
    let error = ready(s.add(NewPlan {
        title: "T".into(),
        ..NewPlan::default()
    }))
    .unwrap_err();
    assert_eq!(code(error), code!("harps-exhausted"));
}

fn tags_of(s: &mut Fake, plan: &str) -> Vec<String> {
    ready(s.show(plan)).unwrap().plan.tags
}

#[test]
fn a_tag_added_is_held_to_the_stores_rules_and_a_tag_removed_is_not() {
    let mut s = service(&[("/tree/a.plan.md", &text("aa-bb", "tags: [area:cli]\n"))]);
    s.tasks.rules = Some(vec!["tagma.permit:\"*:*\"".to_string()]);
    let plan = ready(s.tag("aa-bb", "human:decision", true)).unwrap();
    assert_eq!(plan.tags, ["area:cli", "human:decision"]);
    assert_eq!(
        s.files.text("/tree/a.plan.md"),
        text("aa-bb", "tags: [area:cli, human:decision]\n")
    );

    // Closed to everything but the seeded vocabulary.
    s.tasks.rules = None;
    let refused = ready(s.tag("aa-bb", "area:other", true)).unwrap_err();
    assert!(refused.invalid(), "{refused}");
    assert_eq!(tags_of(&mut s, "aa-bb"), ["area:cli", "human:decision"]);
    // Taking one out asks nothing of the rules.
    let plan = ready(s.tag("aa-bb", "area:cli", false)).unwrap();
    assert_eq!(plan.tags, ["human:decision"]);
}

#[test]
fn a_tag_no_rule_could_admit_is_refused_before_a_file_is_read() {
    let mut s = service(&[]);
    for tag in [
        "ctxloom:plan=\"x\"",
        "and",
        "tagma.permit:x",
        " ",
        "not a tag (((",
    ] {
        let error = ready(s.tag("no-such", tag, true)).unwrap_err();
        assert!(error.invalid(), "{tag}: {error}");
    }
    assert_eq!(s.tasks.asked, 0);
}

#[test]
fn a_tag_already_there_and_a_tag_absent_write_nothing() {
    let mut s = service(&[("/tree/a.plan.md", &text("aa-bb", "tags: [area:cli]\n"))]);
    s.tasks.rules = Some(vec!["tagma.permit:\"*:*\"".to_string()]);
    s.files.deny.borrow_mut().push("/tree/a.plan.md".into());
    assert!(ready(s.tag("aa-bb", "area:cli", true)).is_ok());
    assert!(ready(s.tag("aa-bb", "area:none", false)).is_ok());
}

#[test]
fn a_tag_is_found_to_remove_by_its_canonical_form_and_written_as_the_file_had_it() {
    let mut s = service(&[(
        "/tree/a.plan.md",
        &text("aa-bb", "tags: [\"  area:cli\", priority=3]\n"),
    )]);
    let plan = ready(s.tag("aa-bb", "priority=3", false)).unwrap();
    assert_eq!(plan.tags, ["  area:cli"]);
}

#[test]
fn link_governs_needs_a_task_that_exists_and_is_idempotent() {
    let mut s = service(&[("/tree/a.plan.md", &text("aa-bb", ""))]);
    let governs = |t: &str| Link::Governs(t.to_string());
    let plan = ready(s.link("aa-bb", governs("swift-amber-falcon"), true)).unwrap();
    assert_eq!(plan.governs, ["swift-amber-falcon"]);
    let before = s.files.text("/tree/a.plan.md");
    ready(s.link("aa-bb", governs("swift-amber-falcon"), true)).unwrap();
    assert_eq!(s.files.text("/tree/a.plan.md"), before, "linked once");
    let error = ready(s.link("aa-bb", governs("ghost-task"), true)).unwrap_err();
    assert_eq!(code(error), code!("no-such-task"));
    assert_eq!(s.files.text("/tree/a.plan.md"), before);
    // A task that is gone can still be unlinked.
    s.tasks.tasks.clear();
    let plan = ready(s.link("aa-bb", governs("swift-amber-falcon"), false)).unwrap();
    assert_eq!(plan.governs, Vec::<String>::new());
    ready(s.link("aa-bb", governs("swift-amber-falcon"), false)).unwrap();
}

#[test]
fn link_supersedes_names_a_plan_by_id_or_path_and_refuses_self_and_cycles() {
    let mut s = service(&[
        ("/tree/a.plan.md", &text("aa-bb", "")),
        ("/tree/b.plan.md", &text("cc-dd", "")),
        ("/tree/c.plan.md", &text("ee-ff", "")),
        ("/tree/noid.plan.md", "# no id\n"),
    ]);
    let supersedes = |t: &str| Link::Supersedes(t.to_string());
    let a_before = s.files.text("/tree/a.plan.md");
    let plan = ready(s.link("cc-dd", supersedes("aa-bb"), true)).unwrap();
    assert_eq!(plan.supersedes, ["aa-bb"]);
    assert_eq!(
        s.files.text("/tree/a.plan.md"),
        a_before,
        "the superseded file is not written"
    );
    assert_eq!(ready(s.show("aa-bb")).unwrap().plan.status, "superseded");
    ready(s.link("ee-ff", supersedes("b.plan.md"), true)).unwrap();
    assert_eq!(ready(s.show("ee-ff")).unwrap().plan.supersedes, ["cc-dd"]);

    let refused = |s: &mut Fake, plan: &str, target: &str| {
        code(ready(s.link(plan, supersedes(target), true)).unwrap_err())
    };
    assert_eq!(refused(&mut s, "aa-bb", "aa-bb"), code!("supersede-self"));
    assert_eq!(
        refused(&mut s, "aa-bb", "a.plan.md"),
        code!("supersede-self")
    );
    assert_eq!(refused(&mut s, "aa-bb", "cc-dd"), code!("supersede-cycle"));
    assert_eq!(
        refused(&mut s, "aa-bb", "ee-ff"),
        code!("supersede-cycle"),
        "through another plan"
    );
    assert_eq!(
        refused(&mut s, "aa-bb", "ghost-plan"),
        code!("no-such-plan")
    );
    assert_eq!(refused(&mut s, "aa-bb", "noid.plan.md"), code!("no-id"));

    let plan = ready(s.link("cc-dd", supersedes("a.plan.md"), false)).unwrap();
    assert_eq!(
        plan.supersedes,
        Vec::<String>::new(),
        "unlinked by path too"
    );
    assert_eq!(ready(s.show("aa-bb")).unwrap().plan.status, "draft");
}

#[test]
fn check_finds_each_fault_and_names_the_file() {
    let mut s = service(&[
        (
            "/tree/good.plan.md",
            &text(
                "aa-bb",
                "tags: [human:decision]\ngoverns: [swift-amber-falcon]\n",
            ),
        ),
        ("/tree/yaml.plan.md", "---\nid: cc-dd\ntitle: [x\n---\n"),
        ("/tree/noid.plan.md", "---\ntitle: T\n---\n"),
        ("/tree/bare.plan.md", "# bare\n"),
        ("/tree/dup1.plan.md", &text("ee-ff", "")),
        ("/tree/dup2.plan.md", &text("ee-ff", "")),
        ("/tree/tag.plan.md", &text("gg-hh", "tags: [area:nope]\n")),
        (
            "/tree/gov.plan.md",
            &text("ii-jj", "governs: [ghost-task]\n"),
        ),
        (
            "/tree/sup.plan.md",
            &text("kk-ll", "supersedes: [ghost-plan, kk-ll, aa-bb]\n"),
        ),
        ("/tree/stat.plan.md", &text("mm-nn", "status: banana\n")),
        ("/tree/hand.plan.md", &text("oo-pp", "status: superseded\n")),
    ]);
    let report = ready(s.check()).unwrap();
    assert!(report.failed());
    let by_path = |path: &str| -> Vec<String> {
        report
            .findings
            .iter()
            .filter(|f| f.path() == format!("/tree/{path}.plan.md"))
            .map(|f| f.code().to_string())
            .collect()
    };
    assert_eq!(by_path("good"), Vec::<String>::new());
    assert_eq!(by_path("yaml"), [code!("finding-frontmatter")]);
    assert_eq!(by_path("noid"), [code!("finding-no-id")]);
    assert_eq!(by_path("bare"), [code!("finding-no-id")]);
    assert_eq!(by_path("dup1"), [code!("finding-duplicate-id")]);
    assert_eq!(by_path("dup2"), [code!("finding-duplicate-id")]);
    assert_eq!(by_path("tag"), [code!("finding-tag")]);
    assert_eq!(by_path("gov"), [code!("finding-governs")]);
    assert_eq!(
        by_path("sup"),
        [
            code!("finding-supersedes"),
            code!("finding-supersedes-self")
        ]
    );
    assert_eq!(by_path("stat"), [code!("finding-status")]);
    assert_eq!(by_path("hand"), [code!("finding-hand-superseded")]);
    let paths: Vec<&str> = report.findings.iter().map(Finding::path).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted, "in path order");
}

#[test]
fn check_of_sound_plans_finds_nothing_and_asks_the_rules_only_when_there_are_tags() {
    let mut s = service(&[
        (
            "/tree/a.plan.md",
            &text("aa-bb", "governs: [swift-amber-falcon]\n"),
        ),
        (
            "/tree/b.plan.md",
            &text("cc-dd", "supersedes: [aa-bb]\nstatus: done\n"),
        ),
    ]);
    let report = ready(s.check()).unwrap();
    assert!(!report.failed(), "{report:?}");
    assert_eq!(s.tasks.asked, 0);
}

#[test]
fn check_names_a_file_the_roots_found_but_could_not_read() {
    struct Unreadable(MemFiles);
    impl PlanFiles for Unreadable {
        fn work_root(&self) -> Result<String, angzarr_diagnostic::Report> {
            self.0.work_root()
        }
        fn read(&self, path: &str) -> Option<String> {
            (!path.contains("secret"))
                .then(|| self.0.read(path))
                .flatten()
        }
        fn replace(&self, path: &str, text: &str) -> Result<(), angzarr_diagnostic::Report> {
            self.0.replace(path, text)
        }
        fn create(&self, path: &str, text: &str) -> Result<bool, angzarr_diagnostic::Report> {
            self.0.create(path, text)
        }
        fn scan(&self, root: &str) -> Vec<String> {
            self.0.scan(root)
        }
    }
    let mut s = PlanService::new(
        Unreadable(MemFiles::with(&[("/tree/secret.plan.md", "x")])),
        MemTasks::default(),
        words(&[]),
        Roots(None),
    );
    let report = ready(s.check()).unwrap();
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].code(), code!("finding-unreadable"));
}

#[test]
fn stamp_appends_the_session_once_and_mints_an_id_for_a_file_without() {
    let mut s = service(&[
        ("/tree/a.plan.md", &text("aa-bb", "sessions:\n  - older\n")),
        ("/elsewhere/b.plan.md", "# Plan\n\nBody.\n"),
        ("/tree/notes.md", "---\ntitle: n\n---\n"),
    ]);
    assert_eq!(
        s.stamp("/tree/a.plan.md", "this-session"),
        Ok(Stamp::Stamped)
    );
    assert_eq!(
        s.files.text("/tree/a.plan.md"),
        text("aa-bb", "sessions:\n  - older\n  - this-session\n")
    );
    assert_eq!(
        s.stamp("/tree/a.plan.md", "this-session"),
        Ok(Stamp::Already)
    );
    assert_eq!(
        s.stamp("/elsewhere/b.plan.md", "this-session"),
        Ok(Stamp::Stamped)
    );
    assert_eq!(
        s.files.text("/elsewhere/b.plan.md"),
        "---\nid: first-minted-id\nsessions:\n  - this-session\n---\n# Plan\n\nBody.\n"
    );
    assert_eq!(
        s.stamp("/tree/notes.md", "this-session"),
        Ok(Stamp::NotAPlan)
    );
    assert_eq!(
        s.stamp("/tree/gone.plan.md", "this-session"),
        Ok(Stamp::NotAPlan)
    );
    assert_eq!(s.stamp("/tree/a.plan.md", ""), Ok(Stamp::NoSession));
    assert_eq!(s.files.text("/tree/notes.md"), "---\ntitle: n\n---\n");
}

#[test]
fn stamp_of_a_file_that_has_the_session_but_no_id_only_mints_the_id() {
    let mut s = service(&[(
        "/tree/a.plan.md",
        "---\nsessions: [this-session]\n---\n# A\n",
    )]);
    assert_eq!(s.stamp("a.plan.md", "this-session"), Ok(Stamp::Stamped));
    assert_eq!(
        s.files.text("/tree/a.plan.md"),
        "---\nsessions: [this-session]\nid: first-minted-id\n---\n# A\n"
    );
}

#[test]
fn stamp_of_a_file_it_cannot_edit_is_the_error_and_leaves_it() {
    let broken = "---\ntitle: [x\n---\n";
    let mut s = service(&[("/tree/a.plan.md", broken)]);
    let error = s.stamp("/tree/a.plan.md", "this-session").unwrap_err();
    assert_eq!(code(error), code!("frontmatter"));
    assert_eq!(s.files.text("/tree/a.plan.md"), broken);
}

#[test]
fn a_snapshot_holds_each_files_metadata_and_body() {
    let mut s = service(&[(
        "/tree/a.plan.md",
        "---\nid: aa-bb\ntitle: A\ntags: [area:cli]\n---\n# A\nbody\n",
    )]);
    let snapshot = s.snapshot().unwrap();
    let watched = &snapshot["/tree/a.plan.md"];
    assert_eq!(watched.id.as_deref(), Some("aa-bb"));
    assert_eq!(watched.meta.title.as_deref(), Some("A"));
    assert_eq!(watched.meta.tags, ["area:cli"]);
    assert_eq!(watched.body, "# A\nbody\n");
    assert_eq!(watched.invalid, None);
}

#[test]
fn paths_are_normalized_and_placed() {
    assert_eq!(normalized("/a/./b/../c//d"), "/a/c/d");
    assert_eq!(normalized("C:\\a\\..\\b"), "C:/b");
    assert_eq!(normalized("/.."), "/");
    assert!(is_absolute("/a") && is_absolute("C:/a") && is_absolute("C:\\a"));
    assert!(!is_absolute("a/b") && !is_absolute("./a") && !is_absolute(""));
    assert_eq!(placed(Some("/tree"), "docs/x"), Some("/tree/docs/x".into()));
    assert_eq!(placed(Some("/tree"), "/abs/x"), Some("/abs/x".into()));
    assert_eq!(placed(None, "docs/x"), None);
}

#[test]
fn a_missing_work_root_is_said_for_a_relative_path_not_an_absolute_one() {
    struct Rootless;
    impl PlanFiles for Rootless {
        fn work_root(&self) -> Result<String, angzarr_diagnostic::Report> {
            Err(angzarr_diagnostic::Report::new("x-no-root"))
        }
        fn read(&self, _: &str) -> Option<String> {
            Some("# P\n".into())
        }
        fn replace(&self, _: &str, _: &str) -> Result<(), angzarr_diagnostic::Report> {
            Ok(())
        }
        fn create(&self, _: &str, _: &str) -> Result<bool, angzarr_diagnostic::Report> {
            Ok(true)
        }
        fn scan(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }
    let mut s = PlanService::new(
        Rootless,
        MemTasks::default(),
        words(&[]),
        Roots(Some(vec!["/abs".into()])),
    );
    assert_eq!(ready(s.show("/abs/p.plan.md")).unwrap().content, "# P\n");
    let error = ready(s.show("p.plan.md")).unwrap_err();
    assert_eq!(code(error), "x-no-root");
    s.config = Roots(None);
    assert_eq!(
        code(ready(s.list(&PlanQuery::default())).unwrap_err()),
        "x-no-root"
    );
}
