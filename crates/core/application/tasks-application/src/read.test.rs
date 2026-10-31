//! The reads over fake ports: what a read reaches, and the listing's
//! filters, hiding and limit.

use crate::english::English;
use std::collections::BTreeSet;
use std::path::PathBuf;

use angzarr_embedded_diagnostic::Diagnostic;

use crate::fakes::{ready, FakeClock, FakeDelay, FakeIdentity, FakeNames, FakeStores, Flaky};
use crate::{
    AppError, Hidden, ListQuery, NewTask, PriorityWarning, Project, Reach, Scope, Service,
    ShowQuery, TaskService,
};
use task_priority::MAX;

pub(crate) type Fake = Service<FakeIdentity, FakeStores, FakeNames, FakeClock, FakeDelay>;

pub(crate) fn service(names: &[&str]) -> Fake {
    Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        FakeNames::of(names),
        FakeClock::default(),
        FakeDelay::default(),
    )
}

/// The project `id`, rooted at `/work/<id>`.
pub(crate) fn named(id: &str) -> Project {
    Project {
        id: id.to_string(),
        root: Some(format!("/work/{id}")),
        store: PathBuf::from(format!("/home/.ctxloom/projects/{id}/state.db")),
    }
}

/// Add a task of `text` in `status` (a trigger given for Deferred).
pub(crate) fn add(service: &mut Fake, text: &str, status: &str) {
    let trigger = (status == "Deferred").then(|| "later".to_string());
    let task = NewTask {
        text: text.to_string(),
        status: Some(status.to_string()),
        trigger,
        ..NewTask::default()
    };
    ready(service.add(task)).unwrap();
}

/// A service holding a task in each default status and one in
/// `Blocked`, each task's text its status.
pub(crate) fn with_every_status() -> Fake {
    let mut service = service(&["to-do", "doing", "deferred", "done", "archived", "blocked"]);
    for status in [
        "To Do",
        "In Progress",
        "Deferred",
        "Done",
        "Archived",
        "Blocked",
    ] {
        add(&mut service, status, status);
    }
    service
}

fn ids(service: &mut Fake, query: ListQuery) -> Vec<String> {
    ready(service.list(query))
        .unwrap()
        .tasks()
        .map(|t| t.harp_id.clone())
        .collect()
}

fn statuses(statuses: &[&str]) -> ListQuery {
    ListQuery {
        statuses: statuses.iter().map(|s| s.to_string()).collect(),
        ..ListQuery::default()
    }
}

// ------------------------------------------------------------ status classes

#[test]
fn test_open_is_every_status_not_terminal_whatever_its_name() {
    let mut service = with_every_status();
    assert_eq!(
        ids(&mut service, statuses(&["@open"])),
        ["to-do", "doing", "deferred", "blocked"]
    );
}

#[test]
fn test_terminal_is_done_and_archived_and_mixes_with_a_status() {
    let mut service = with_every_status();
    assert_eq!(
        ids(&mut service, statuses(&["@terminal"])),
        ["done", "archived"]
    );
    assert_eq!(
        ids(&mut service, statuses(&[" In Progress ", "@terminal"])),
        ["doing", "done", "archived"]
    );
}

#[test]
fn test_a_class_no_one_knows_is_refused_before_anything_is_read() {
    let mut service = with_every_status();
    service.identity.pins.clear();
    let error = ready(service.list(statuses(&["To Do", "@done"]))).unwrap_err();
    assert_eq!(
        error,
        AppError::UnknownStatusClass {
            status: "@done".to_string()
        }
    );
    assert!(error.to_string().contains("@open, @terminal"), "{error}");
    assert!(service.identity.pins.is_empty());
}

// ------------------------------------------------------------ term

#[test]
fn test_a_term_keeps_the_tasks_whose_text_mentions_it_in_any_case() {
    let mut service = service(&["first", "second", "third"]);
    for text in ["Ship the DOCS", "fix docs", "other"] {
        add(&mut service, text, "To Do");
    }
    let query = ListQuery {
        term: Some(" Docs ".to_string()),
        ..ListQuery::default()
    };
    assert_eq!(ids(&mut service, query), ["first", "second"]);
}

// ------------------------------------------------------------ hidden

#[test]
fn test_a_search_says_what_the_default_view_hid_of_its_matches() {
    let mut service = with_every_status();
    let query = ListQuery {
        term: Some("e".to_string()),
        ..ListQuery::default()
    };
    let listing = ready(service.list(query)).unwrap();
    let shown: Vec<&str> = listing.tasks().map(|t| t.harp_id.as_str()).collect();
    assert_eq!(shown, ["doing", "blocked"], "In Progress, Blocked");
    assert_eq!(
        listing.hidden,
        Hidden {
            completed: 2,
            deferred: 1
        },
        "Done, Archived; Deferred"
    );
    assert_eq!(listing.hidden.total(), 3);
    assert!(listing.searched);
    let plain = ready(service.list(ListQuery::default())).unwrap();
    assert!(!plain.searched);
    let tagged = ListQuery {
        tag_query: Some("x/not".to_string()),
        ..ListQuery::default()
    };
    assert!(ready(service.list(tagged)).unwrap().searched);
}

#[test]
fn test_all_or_a_status_hides_nothing() {
    let mut service = with_every_status();
    for query in [
        ListQuery {
            all: true,
            ..ListQuery::default()
        },
        statuses(&["Done"]),
    ] {
        let listing = ready(service.list(query)).unwrap();
        assert_eq!(listing.hidden, Hidden::default());
    }
}

// ------------------------------------------------------------ limit

#[test]
fn test_a_limit_keeps_the_first_tasks_and_counts_the_rest() {
    let mut service = with_every_status();
    let query = ListQuery {
        all: true,
        limit: Some(4),
        ..ListQuery::default()
    };
    let listing = ready(service.list(query)).unwrap();
    let shown: Vec<&str> = listing.tasks().map(|t| t.harp_id.as_str()).collect();
    assert_eq!(shown, ["to-do", "doing", "deferred", "done"]);
    assert_eq!(listing.omitted, 2);
    let roomy = ListQuery {
        limit: Some(9),
        ..ListQuery::default()
    };
    assert_eq!(ready(service.list(roomy)).unwrap().omitted, 0);
    let none = ready(service.list(ListQuery::default())).unwrap();
    assert_eq!((none.rows.len(), none.omitted), (3, 0), "none is no limit");
}

// A limit of 0 is given blank: refused, never read as no limit.
#[test]
fn test_a_limit_of_zero_is_refused_as_blank() {
    let mut service = with_every_status();
    let query = ListQuery {
        limit: Some(0),
        ..ListQuery::default()
    };
    assert_eq!(
        ready(service.list(query)).unwrap_err(),
        AppError::Blank { field: "limit" }
    );
}

// ------------------------------------------------------------ sort

fn by_priority() -> ListQuery {
    ListQuery {
        sort: Some(" priority ".to_string()),
        ..ListQuery::default()
    }
}

/// Tasks of triage levels 5 and 1, and one untriaged, added in that order.
fn with_levels() -> Fake {
    let mut service = service(&["low", "high", "none", "finished"]);
    tagged(&mut service, "low", "To Do", &["triage:level=5"]);
    tagged(&mut service, "high", "To Do", &["triage:level=1"]);
    tagged(&mut service, "none", "In Progress", &[]);
    tagged(&mut service, "finished", "Done", &["triage:level=1"]);
    service
}

#[test]
fn test_a_sort_by_priority_lists_the_highest_first_with_each_ones_priority() {
    let mut service = with_levels();
    let listing = ready(service.list(by_priority())).unwrap();
    let ranked: Vec<(&str, Option<f64>)> = listing
        .ranked()
        .map(|(row, priority)| (row.task.harp_id.as_str(), priority))
        .collect();
    assert_eq!(
        ranked,
        [
            ("high", Some(MAX)),
            ("low", Some(MAX * 2.0 / 3.0)),
            ("none", Some(MAX / 3.0)),
        ],
        "the Done task is hidden, and ranks no live task down"
    );
}

#[test]
fn test_an_unsorted_listing_has_no_priority() {
    let mut service = with_levels();
    let listing = ready(service.list(ListQuery::default())).unwrap();
    assert!(listing.ranked().all(|(_, priority)| priority.is_none()));
    assert!(listing.priority_notes.is_empty());
    assert_eq!(
        ids(&mut service, ListQuery::default()),
        ["low", "high", "none"]
    );
}

#[test]
fn test_priority_ranks_a_task_against_its_whole_project_not_the_page() {
    let mut service = with_levels();
    let query = ListQuery {
        term: Some("low".to_string()),
        ..by_priority()
    };
    let listing = ready(service.list(query)).unwrap();
    let ranked: Vec<Option<f64>> = listing.ranked().map(|(_, p)| p).collect();
    assert_eq!(ranked, [Some(MAX * 2.0 / 3.0)]);
}

#[test]
fn test_the_limit_takes_the_highest_priorities() {
    let mut service = with_levels();
    let query = ListQuery {
        limit: Some(1),
        ..by_priority()
    };
    let listing = ready(service.list(query)).unwrap();
    let shown: Vec<&str> = listing.tasks().map(|t| t.harp_id.as_str()).collect();
    assert_eq!((shown, listing.omitted), (vec!["high"], 2));
}

#[test]
fn test_a_ranking_that_means_little_says_so() {
    let mut service = service(&["a", "b"]);
    add(&mut service, "a", "To Do");
    add(&mut service, "b", "To Do");
    let listing = ready(service.list(by_priority())).unwrap();
    let warned: Vec<&PriorityWarning> = listing.priority_notes.iter().map(|n| &n.warning).collect();
    assert_eq!(
        warned.first(),
        Some(&&PriorityWarning::AllTied { scored: 0, live: 2 })
    );
    assert!(listing.priority_notes.iter().all(|n| n.project.is_none()));
}

#[test]
fn test_every_project_is_ranked_in_its_own_section() {
    let mut service = with_two_projects();
    service.identity.bound = Some(named("zulu-proj"));
    let query = ListQuery {
        global: true,
        ..by_priority()
    };
    let listing = ready(service.list(query)).unwrap();
    let ranked: Vec<(&str, Option<f64>)> = listing
        .ranked()
        .map(|(row, priority)| (row.project.id.as_str(), priority))
        .collect();
    assert_eq!(
        ranked,
        [
            ("alpha-proj", Some(MAX)),
            ("alpha-proj", Some(MAX)),
            ("zulu-proj", Some(MAX)),
        ]
    );
    let projects: BTreeSet<Option<&str>> = listing
        .priority_notes
        .iter()
        .map(|n| n.project.as_deref())
        .collect();
    assert_eq!(
        projects,
        BTreeSet::from([Some("alpha-proj"), Some("zulu-proj")])
    );
}

/// `add` and `remove` declared on the store's schema stream.
fn declare(service: &mut Fake, add: &[&str], remove: &[&str]) {
    let strings = |s: &[&str]| s.iter().map(|t| t.to_string()).collect();
    ready(crate::PermitService::declare(
        service,
        crate::Declaration {
            add: strings(add),
            remove: strings(remove),
        },
    ))
    .unwrap();
}

/// The seeded `taskloom.priority_fn`.
fn seeded_priority_fn() -> &'static str {
    crate::SEEDED_SCHEMA
        .iter()
        .find(|t| t.starts_with("taskloom.priority_fn:"))
        .expect("seeded")
}

#[test]
fn test_a_formula_that_does_not_compile_refuses_the_sort() {
    let mut service = with_levels();
    declare(
        &mut service,
        &[r#"taskloom.priority_fn:"triage:kind"="1 +""#],
        &[seeded_priority_fn()],
    );
    let error = ready(service.list(by_priority())).unwrap_err();
    assert_eq!(error.code(), "task-priority-malformed");
    assert!(
        ready(service.list(ListQuery::default())).is_ok(),
        "only the sort reads it"
    );
}

// The owner's rule (2026-10-30): the formulas are taskloom's, stored as
// `taskloom.*` tags on the schema stream and evaluated by taskloom's own
// evaluator (task-priority), never by tagma. A formula declared in the
// store is the one a sort ranks by: here effort alone, so the least
// effort ranks highest whatever the level.
#[test]
fn test_a_sort_ranks_by_the_stores_taskloom_formula_through_taskloom_evaluator() {
    let mut service = service(&["costly", "cheap"]);
    tagged(
        &mut service,
        "costly",
        "To Do",
        &["triage:level=1", "triage:effort=4"],
    );
    tagged(
        &mut service,
        "cheap",
        "To Do",
        &["triage:level=5", "triage:effort=1"],
    );
    let ranked = |service: &mut Fake| -> Vec<String> {
        ready(service.list(by_priority()))
            .unwrap()
            .ranked()
            .map(|(row, _)| row.task.harp_id.clone())
            .collect()
    };
    assert_eq!(
        ranked(&mut service),
        ["costly", "cheap"],
        "the seeded formula"
    );
    declare(
        &mut service,
        &[r#"taskloom.priority_fn:"triage:kind"="10 - {{triage:effort}}""#],
        &[seeded_priority_fn()],
    );
    assert_eq!(ranked(&mut service), ["cheap", "costly"], "the store's own");
    let checked = ready(crate::SchemaService::schema_check(&mut service, None)).unwrap();
    assert_eq!(checked, [], "tagma reads no taskloom.* tag as its own");
}

#[test]
fn test_an_unknown_sort_is_refused_before_anything_is_read() {
    let mut service = service(&[]);
    let query = ListQuery {
        sort: Some("age".to_string()),
        ..ListQuery::default()
    };
    assert_eq!(
        ready(service.list(query)).unwrap_err(),
        AppError::UnknownSort {
            sort: "age".to_string(),
        }
    );
    assert!(service.identity.pins.is_empty());
    assert!(ready(service.list(ListQuery::default())).is_ok(), "no sort");
}

// A term, tag query or sort given blank is given: refused, never read as
// none, before anything is read; a listing and a count of tags alike.
#[test]
fn test_a_blank_term_tag_query_or_sort_given_is_refused_before_anything_is_read() {
    let mut service = service(&[]);
    for blank in ["", "  "] {
        let given = || Some(blank.to_string());
        for (query, field) in [
            (
                ListQuery {
                    term: given(),
                    ..ListQuery::default()
                },
                "term",
            ),
            (
                ListQuery {
                    tag_query: given(),
                    ..ListQuery::default()
                },
                "tag_query",
            ),
            (
                ListQuery {
                    sort: given(),
                    ..ListQuery::default()
                },
                "sort",
            ),
        ] {
            let error = ready(service.list(query)).unwrap_err();
            assert_eq!(error, AppError::Blank { field });
        }
        for (query, field) in [
            (
                ListQuery {
                    term: given(),
                    ..ListQuery::default()
                },
                "term",
            ),
            (
                ListQuery {
                    tag_query: given(),
                    ..ListQuery::default()
                },
                "tag_query",
            ),
        ] {
            let error = ready(service.tags(query)).unwrap_err();
            assert_eq!(error, AppError::Blank { field });
        }
    }
    assert!(service.identity.pins.is_empty(), "no project resolved");
}

// A status given blank is given: refused, never read as no status filter
// (which lists the default statuses), before anything is read. A trailing
// comma (`--status Done,`) gives one blank too.
#[test]
fn test_a_blank_status_given_is_refused_before_anything_is_read() {
    let mut service = service(&[]);
    for blank in ["", "  "] {
        let query = || ListQuery {
            statuses: vec!["Done".to_string(), blank.to_string()],
            ..ListQuery::default()
        };
        let error = ready(service.list(query())).unwrap_err();
        assert_eq!(error, AppError::Blank { field: "status" });
        let error = ready(service.tags(query())).unwrap_err();
        assert_eq!(error, AppError::Blank { field: "status" });
    }
    assert!(service.identity.pins.is_empty(), "no project resolved");
}

// ------------------------------------------------------------ reach

/// Two projects with tasks, `b` added before `a`, and a third with no
/// store.
fn with_two_projects() -> Fake {
    let mut service = service(&["zesty-one", "alpha-two", "brisk-three"]);
    for id in ["alpha-proj", "zulu-proj"] {
        service
            .stores
            .others
            .insert(id.to_string(), Flaky::default());
    }
    service.identity.bound = Some(named("zulu-proj"));
    add(&mut service, "in zulu", "To Do");
    service.identity.bound = Some(named("alpha-proj"));
    add(&mut service, "in alpha", "To Do");
    add(&mut service, "in alpha too", "To Do");
    service.identity.bound = None;
    service.stores.absent = true;
    service.identity.every = vec![named("alpha-proj"), named("empty-proj"), named("zulu-proj")];
    service
}

#[test]
fn test_global_reads_every_project_by_project_then_harp() {
    let mut service = with_two_projects();
    service.identity.bound = Some(named("zulu-proj"));
    let query = ListQuery {
        global: true,
        ..ListQuery::default()
    };
    let listing = ready(service.list(query)).unwrap();
    let rows: Vec<(&str, &str)> = listing
        .rows
        .iter()
        .map(|r| (r.project.id.as_str(), r.task.harp_id.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("alpha-proj", "alpha-two"),
            ("alpha-proj", "brisk-three"),
            ("zulu-proj", "zesty-one")
        ]
    );
    assert_eq!(
        listing.reach,
        Reach::Every {
            projects: 2,
            fallback: None
        },
        "the project with no store is not counted"
    );
    assert_eq!(listing.project(), None);
    assert!(listing.reach.is_every());
}

#[test]
fn test_no_project_outside_a_work_root_reads_every_project_saying_where() {
    let mut service = with_two_projects();
    service.identity.rootless = Some("/tmp/somewhere".to_string());
    let listing = ready(service.list(ListQuery::default())).unwrap();
    assert_eq!(
        listing.reach,
        Reach::Every {
            projects: 2,
            fallback: Some("/tmp/somewhere".to_string())
        }
    );
    assert_eq!(listing.rows.len(), 3);
}

#[test]
fn test_a_project_found_is_read_alone_rootless_or_not() {
    let mut service = with_two_projects();
    service.identity.bound = Some(named("zulu-proj"));
    service.identity.rootless = Some("/tmp/somewhere".to_string());
    let listing = ready(service.list(ListQuery::default())).unwrap();
    assert_eq!(listing.reach, Reach::Project(named("zulu-proj")));
    assert_eq!(listing.rows.len(), 1);
    assert!(!listing.reach.is_every());
}

#[test]
fn test_a_global_read_whose_projects_cannot_be_named_fails_with_why() {
    let mut service = service(&[]);
    service.identity.fails = Some(angzarr_embedded_diagnostic::Report::new("x-registry"));
    let query = ListQuery {
        global: true,
        ..ListQuery::default()
    };
    assert_eq!(ready(service.list(query)).unwrap_err().code(), "x-registry");
}

#[test]
fn test_a_tag_query_over_every_project_tells_their_tasks_apart() {
    let mut service = with_two_projects();
    let query = ListQuery {
        global: true,
        tag_query: Some("x/not".to_string()),
        ..ListQuery::default()
    };
    assert_eq!(ready(service.list(query)).unwrap().rows.len(), 3);
}

// ------------------------------------------------------------ show

fn asked(ids: &[&str]) -> ShowQuery {
    ShowQuery {
        harp_ids: ids.iter().map(|s| s.to_string()).collect(),
        ..ShowQuery::default()
    }
}

#[test]
fn test_show_answers_with_each_task_asked_for_in_the_order_asked_whatever_its_status() {
    let mut service = with_every_status();
    let details = ready(service.show(asked(&["done", "to-do", "done"]))).unwrap();
    let ids: Vec<&str> = details
        .rows
        .iter()
        .map(|r| r.task.harp_id.as_str())
        .collect();
    assert_eq!(ids, ["done", "to-do", "done"], "a repeated id twice");
    assert_eq!(details.rows[0].task.text, "Done");
    assert_eq!(details.reach, Reach::Project(crate::fakes::project()));
}

#[test]
fn test_show_fails_naming_every_id_no_task_holds() {
    let mut service = with_every_status();
    let error = ready(service.show(asked(&["nope-a", "done", "nope-b"]))).unwrap_err();
    assert_eq!(
        error,
        AppError::NotFound {
            harp_ids: vec!["nope-a".to_string(), "nope-b".to_string()]
        }
    );
    assert!(
        error.english().contains("\"nope-a\", \"nope-b\""),
        "{error}"
    );
}

#[test]
fn test_show_across_projects_fails_on_an_id_two_hold_naming_them() {
    let mut service = service(&["swift-amber", "swift-amber", "calm-otter", "swift-amber"]);
    for id in ["alpha-proj", "zulu-proj"] {
        service
            .stores
            .others
            .insert(id.to_string(), Flaky::default());
    }
    service.identity.bound = Some(named("alpha-proj"));
    add(&mut service, "in alpha", "To Do");
    add(&mut service, "in alpha too", "To Do");
    service.identity.bound = Some(named("zulu-proj"));
    add(&mut service, "in zulu", "To Do");
    service.identity.every = vec![named("alpha-proj"), named("zulu-proj")];
    let global = ShowQuery {
        global: true,
        ..asked(&["swift-amber", "calm-otter"])
    };
    let error = ready(service.show(global.clone())).unwrap_err();
    assert_eq!(
        error,
        AppError::Ambiguous {
            held: vec![(
                "swift-amber".to_string(),
                vec!["alpha-proj".to_string(), "zulu-proj".to_string()]
            )]
        }
    );
    assert!(
        error
            .english()
            .contains("\"swift-amber\" in alpha-proj, zulu-proj"),
        "{error}"
    );
    let one = ShowQuery {
        harp_ids: vec!["calm-otter".to_string()],
        ..global
    };
    let details = ready(service.show(one)).unwrap();
    assert_eq!(details.rows[0].project.id, "alpha-proj");
    let here = ready(service.show(asked(&["swift-amber"]))).unwrap();
    assert_eq!(here.rows[0].task.text, "in zulu", "this project's alone");
}

#[test]
fn test_show_where_there_is_no_project_finds_nothing() {
    let mut service = service(&[]);
    let error = ready(service.show(asked(&["swift-amber"]))).unwrap_err();
    assert_eq!(error.code(), "tasks-application-not-found");
}

// ------------------------------------------------------------ summary

#[test]
fn test_a_summary_counts_every_task_by_status_and_names_those_in_progress() {
    let mut service = with_every_status();
    service.names = crate::fakes::FakeNames::of(&["also-doing"]);
    add(&mut service, "Second", "In Progress");
    let summary = ready(service.summary(Scope::default())).unwrap();
    let counts: Vec<(&str, usize)> = summary
        .counts
        .iter()
        .map(|(s, n)| (s.as_str(), *n))
        .collect();
    assert_eq!(
        counts,
        [
            ("Archived", 1),
            ("Blocked", 1),
            ("Deferred", 1),
            ("Done", 1),
            ("In Progress", 2),
            ("To Do", 1)
        ]
    );
    let doing: Vec<&str> = summary
        .in_progress
        .iter()
        .map(|r| r.task.harp_id.as_str())
        .collect();
    assert_eq!(doing, ["doing", "also-doing"], "in the order added");
    assert_eq!(summary.reach, Reach::Project(crate::fakes::project()));
}

#[test]
fn test_a_summary_across_projects_sums_them() {
    let mut service = with_two_projects();
    let global = Scope {
        global: true,
        ..Scope::default()
    };
    let summary = ready(service.summary(global)).unwrap();
    assert_eq!(summary.counts.get("To Do"), Some(&3));
    assert!(summary.reach.is_every());
}

// ------------------------------------------------------------ statuses

fn taxonomy(service: &mut Fake, scope: Scope) -> Vec<(String, usize, bool, bool)> {
    ready(service.statuses(scope))
        .unwrap()
        .statuses
        .into_iter()
        .map(|s| (s.name, s.order, s.terminal, s.requires_trigger))
        .collect()
}

fn info(name: &str, order: usize, terminal: bool, trigger: bool) -> (String, usize, bool, bool) {
    (name.to_string(), order, terminal, trigger)
}

#[test]
fn test_the_statuses_are_the_defaults_in_order_where_there_is_no_task() {
    let mut service = service(&[]);
    assert_eq!(
        taxonomy(&mut service, Scope::default()),
        [
            info("In Progress", 0, false, false),
            info("To Do", 1, false, false),
            info("Deferred", 2, false, true),
            info("Done", 3, true, false),
            info("Archived", 4, true, false),
        ]
    );
}

#[test]
fn test_the_statuses_in_use_follow_the_defaults_by_name_each_once() {
    let mut service = service(&["first", "second", "third", "fourth"]);
    for status in ["Waiting", "Blocked", "Waiting", "Done"] {
        add(&mut service, status, status);
    }
    let statuses = taxonomy(&mut service, Scope::default());
    assert_eq!(
        statuses[5..],
        [
            info("Blocked", 5, false, false),
            info("Waiting", 6, false, false)
        ]
    );
    assert_eq!(statuses.len(), 7);
}

// ------------------------------------------------------------ tags

fn tagged(service: &mut Fake, text: &str, status: &str, tags: &[&str]) {
    let trigger = (status == "Deferred").then(|| "later".to_string());
    let task = NewTask {
        text: text.to_string(),
        status: Some(status.to_string()),
        trigger,
        tags: tags.iter().map(|t| t.to_string()).collect(),
        ..NewTask::default()
    };
    ready(service.add(task)).unwrap();
}

fn counted(service: &mut Fake, query: ListQuery) -> Vec<(String, usize, usize)> {
    ready(service.tags(query))
        .unwrap()
        .tags
        .into_iter()
        .map(|c| (c.tag.to_string(), c.active, c.total))
        .collect()
}

fn count(tag: &str, active: usize, total: usize) -> (String, usize, usize) {
    (tag.to_string(), active, total)
}

#[test]
fn test_the_tags_in_use_are_counted_active_and_in_all_by_spelling() {
    let mut service = service(&["first", "second", "third", "fourth"]);
    tagged(&mut service, "a", "To Do", &["release", "b:x=1"]);
    tagged(&mut service, "b", "Done", &["release"]);
    tagged(&mut service, "c", "Deferred", &["release", "docs"]);
    tagged(&mut service, "d", "Blocked", &["docs"]);
    assert_eq!(
        counted(&mut service, ListQuery::default()),
        [
            count("b:x=1", 1, 1),
            count("docs", 1, 2),
            count("release", 1, 3)
        ]
    );
}

#[test]
fn test_the_tags_counted_are_those_of_the_tasks_selected() {
    let mut service = service(&["first", "second", "third"]);
    tagged(&mut service, "ship docs", "To Do", &["release"]);
    tagged(&mut service, "other", "Done", &["release", "x"]);
    tagged(&mut service, "ship code", "Done", &["y"]);
    let by_term = ListQuery {
        term: Some("SHIP".to_string()),
        ..ListQuery::default()
    };
    assert_eq!(
        counted(&mut service, by_term),
        [count("release", 1, 1), count("y", 0, 1)]
    );
    let by_status = ListQuery {
        statuses: vec!["@terminal".to_string()],
        ..ListQuery::default()
    };
    assert_eq!(
        counted(&mut service, by_status),
        [count("release", 0, 1), count("x", 0, 1), count("y", 0, 1)]
    );
    let by_query = ListQuery {
        tag_query: Some("x".to_string()),
        ..ListQuery::default()
    };
    assert_eq!(
        counted(&mut service, by_query),
        [count("release", 0, 1), count("x", 0, 1)]
    );
}

#[test]
fn test_a_tag_the_schema_hides_is_not_counted() {
    let mut service = service(&["first"]);
    tagged(&mut service, "a", "To Do", &["triage:kind=chore", "x"]);
    let query = ListQuery {
        ..ListQuery::default()
    };
    declare(&mut service, &[r#"tagma.hide:"triage:*"=true"#], &[]);
    let tags: Vec<String> = ready(service.tags(query))
        .unwrap()
        .tags
        .iter()
        .map(|c| c.tag.to_string())
        .collect();
    assert_eq!(tags, ["x"]);
}

#[test]
fn test_a_tags_read_refuses_a_malformed_query_or_class_before_reading() {
    let mut service = service(&[]);
    for query in [
        ListQuery {
            tag_query: Some("x/and".to_string()),
            ..ListQuery::default()
        },
        statuses(&["@nope"]),
    ] {
        assert!(ready(service.tags(query)).is_err());
    }
    assert!(service.identity.pins.is_empty());
}

// ------------------------------------------------------------ lint

#[test]
fn test_lint_sweeps_every_task_the_read_reaches_under_the_schema_now() {
    let mut service = service(&["first", "second"]);
    tagged(&mut service, "a", "Done", &["area:x=c"]);
    tagged(&mut service, "b", "To Do", &["area:x=a"]);
    declare(
        &mut service,
        &[r#"tagma.enum:"area:x"=a"#, r#"tagma.enum:"area:x"=b"#],
        &[],
    );
    let report = ready(service.lint(Scope::default())).unwrap();
    let found: Vec<(&str, &str)> = report
        .violations
        .iter()
        .map(|v| (v.harp_id.as_str(), v.project.as_deref().unwrap_or("")))
        .collect();
    assert_eq!(found, [("first", "jolly-mute-magma")]);
    assert!(
        report.checked_targets > 1,
        "the seeded schema's, and area:x"
    );
    assert_eq!(report.reach, Reach::Project(crate::fakes::project()));
}
