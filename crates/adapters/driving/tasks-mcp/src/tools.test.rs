//! Each tool's arguments as the request they make, and each answer as
//! the result it gives.

use std::path::PathBuf;

use serde_json::json;
use tasks_application::{
    Hidden, ListQuery, Listing, PriorityNote, PriorityWarning, Project, Reach, Row, Scope, Shown,
    StatusChange, Summary, TagChange, Task, TextEdit, Timestamp, Written,
};

use super::{
    list_request, listed, new_task, offered, status_change, tag_change, text_edit, written,
    ListRequest,
};
use crate::{McpError, Settings};

fn settings() -> Settings {
    Settings {
        session: "calm-otter-fox".to_string(),
        ..Settings::default()
    }
}

fn project() -> Project {
    Project {
        id: "jolly-mute-magma".to_string(),
        root: Some("/work/tree".to_string()),
        store: PathBuf::from("/s/state.db"),
    }
}

fn task() -> Task {
    Task {
        harp_id: "swift-amber".to_string(),
        text: "Ship it".to_string(),
        status: "To Do".to_string(),
        checked: false,
        text_hash: "0123456789ab".to_string(),
        trigger: String::new(),
        origin_session: String::new(),
        tags: vec![tag("a")],
        created_at: Timestamp {
            seconds: 1_781_058_920,
            nanos: 0,
        },
    }
}

fn tag(spelled: &str) -> tasks_application::Tag {
    tasks_application::parse_tag(spelled).unwrap().unwrap()
}

/// What `hide` facts show.
fn hiding(facts: &[&str]) -> Shown {
    Shown::of(&tasks_application::TagSchema::parse(facts).unwrap())
}

/// A listing that read `reach`, of `tasks` in `project()`.
fn listing(reach: Reach, tasks: Vec<Task>, shown: Shown) -> Listing {
    let rows = tasks
        .into_iter()
        .map(|task| Row {
            project: project(),
            task,
        })
        .collect();
    Listing {
        reach,
        rows,
        shown,
        hidden: Hidden::default(),
        searched: false,
        omitted: 0,
        priorities: None,
        priority_notes: Vec::new(),
    }
}

fn catalog() -> presentation::Catalog {
    presentation::Catalog::en_us(&[crate::EN_US]).unwrap()
}

fn pinned() -> Settings {
    Settings {
        project: Some("pinned-proj-id".to_string()),
        ..settings()
    }
}

#[test]
fn test_the_tools_offered_are_gos_five_in_gos_order() {
    let names: Vec<String> = offered().into_iter().map(|t| t.name.into_owned()).collect();
    assert_eq!(
        names,
        [
            "task_add",
            "task_edit",
            "task_list",
            "task_set_status",
            "task_tag"
        ]
    );
}

#[test]
fn test_a_listing_asks_for_everything_its_arguments_name() {
    let request = list_request(
        json!({
            "statuses": ["To Do", "@open"], "include_completed": true, "term": "ship",
            "tag_query": "a/b/and", "include_summary": true, "global": true,
            "sort": "priority", "compact": true, "limit": 5,
        }),
        &pinned(),
    )
    .unwrap();
    assert_eq!(
        request,
        ListRequest {
            query: ListQuery {
                project: Some("pinned-proj-id".to_string()),
                statuses: vec!["To Do".to_string(), "@open".to_string()],
                all: true,
                tag_query: Some("a/b/and".to_string()),
                term: Some("ship".to_string()),
                global: true,
                limit: Some(5),
                sort: Some("priority".to_string()),
            },
            compact: true,
            summary: true,
        }
    );
    assert_eq!(
        list_request(json!({"statuses": null}), &settings()).unwrap(),
        ListRequest::default()
    );
}

// An argument left out, or null, is `None`; one given "" is given, passed
// on as given for the use case to refuse as blank, never read as left out.
#[test]
fn test_a_string_argument_given_empty_is_passed_on_as_given() {
    let given = Some(String::new());
    let query = list_request(
        json!({"term": "", "tag_query": "", "sort": ""}),
        &settings(),
    )
    .unwrap()
    .query;
    assert_eq!(
        (query.term, query.tag_query, query.sort),
        (given.clone(), given.clone(), given.clone())
    );
    let nulls = json!({"term": null, "tag_query": null, "sort": null});
    assert_eq!(
        list_request(nulls, &settings()).unwrap(),
        ListRequest::default()
    );
    let task = new_task(
        json!({"text": "x", "status": "", "trigger": ""}),
        &settings(),
    )
    .unwrap();
    assert_eq!((task.status, task.trigger), (given.clone(), given.clone()));
    let bare = new_task(json!({"text": "x", "status": null}), &settings()).unwrap();
    assert_eq!((bare.status, bare.trigger), (None, None));
    let change = json!({"harp_id": "h", "status": "Done", "trigger": ""});
    assert_eq!(status_change(change, &settings()).unwrap().trigger, given);
}

// CTXLOOM_SESSION_HARP set empty is unset: a task is added in no session.
#[test]
fn test_an_add_in_an_empty_session_is_added_in_none() {
    let settings = Settings {
        session: String::new(),
        ..settings()
    };
    let task = new_task(json!({"text": "x"}), &settings).unwrap();
    assert_eq!(task.origin_session, None);
}

// A limit left out (or null) is no cap; one given is passed on as given,
// 0 included, for the use case to refuse as blank. A negative limit is no
// count of tasks, and is refused here.
#[test]
fn test_a_limit_left_out_is_none_and_one_given_is_passed_on() {
    for arguments in [json!({}), json!({ "limit": null })] {
        let request = list_request(arguments.clone(), &settings()).unwrap();
        assert_eq!(request.query.limit, None, "{arguments}");
    }
    let request = list_request(json!({ "limit": 0 }), &settings()).unwrap();
    assert_eq!(request.query.limit, Some(0));
    assert!(matches!(
        list_request(json!({ "limit": -3 }), &settings()),
        Err(McpError::Arguments { tool, .. }) if tool == "task_list"
    ));
}

#[test]
fn test_a_summary_counts_the_projects_the_listing_reached() {
    let request = list_request(json!({"include_summary": true}), &pinned()).unwrap();
    let every = Reach::Every {
        projects: 2,
        fallback: Some("/tmp".to_string()),
    };
    assert_eq!(
        request.summary_scope(&every),
        Scope {
            project: Some("pinned-proj-id".to_string()),
            global: true,
        }
    );
    assert!(!request.summary_scope(&Reach::Project(project())).global);
}

#[test]
fn test_an_add_asks_for_its_task_in_the_servers_session() {
    let task = new_task(
        json!({"text": "Ship it", "status": "Deferred", "trigger": "later", "tags": ["a"]}),
        &settings(),
    )
    .unwrap();
    assert_eq!(task.text, "Ship it");
    assert_eq!(task.status.as_deref(), Some("Deferred"));
    assert_eq!(task.trigger.as_deref(), Some("later"));
    assert_eq!(task.tags, ["a"]);
    assert_eq!(task.origin_session.as_deref(), Some("calm-otter-fox"));
    let bare = new_task(json!({"text": "x", "tags": null}), &settings()).unwrap();
    assert!(bare.tags.is_empty());
    assert!(matches!(
        new_task(json!({"text": 3}), &settings()),
        Err(McpError::Arguments { tool, .. }) if tool == "task_add"
    ));
}

#[test]
fn test_each_change_asks_for_its_task_by_harp_in_the_pinned_project() {
    assert_eq!(
        status_change(
            json!({"harp_id": "swift-amber", "status": "Deferred", "trigger": "later"}),
            &pinned()
        )
        .unwrap(),
        StatusChange {
            project: Some("pinned-proj-id".to_string()),
            harp_id: "swift-amber".to_string(),
            status: "Deferred".to_string(),
            trigger: Some("later".to_string()),
        }
    );
    assert_eq!(
        status_change(json!({"harp_id": "h", "status": "Done"}), &settings())
            .unwrap()
            .trigger,
        None
    );
    assert_eq!(
        text_edit(json!({"harp_id": "swift-amber", "text": "New"}), &pinned()).unwrap(),
        TextEdit {
            project: Some("pinned-proj-id".to_string()),
            harp_id: "swift-amber".to_string(),
            text: "New".to_string(),
        }
    );
    assert_eq!(
        tag_change(
            json!({"harp_id": "swift-amber", "add": ["a"], "remove": ["b"]}),
            &pinned()
        )
        .unwrap(),
        TagChange {
            project: Some("pinned-proj-id".to_string()),
            harp_id: "swift-amber".to_string(),
            add: vec!["a".to_string()],
            remove: vec!["b".to_string()],
        }
    );
    let bare = tag_change(json!({"harp_id": "h", "add": null}), &settings()).unwrap();
    assert!(bare.add.is_empty() && bare.remove.is_empty());
}

#[test]
fn test_each_change_without_what_its_schema_requires_is_refused_by_name() {
    for (tool, arguments) in [
        (
            "task_set_status",
            status_change(json!({"harp_id": "h"}), &settings()).err(),
        ),
        (
            "task_edit",
            text_edit(json!({"text": "x"}), &settings()).err(),
        ),
        (
            "task_tag",
            tag_change(json!({"harp_id": "h", "colour": 1}), &settings()).err(),
        ),
    ] {
        assert!(
            matches!(&arguments, Some(McpError::Arguments { tool: t, .. }) if t == tool),
            "{tool}: {arguments:?}"
        );
    }
}

#[test]
fn test_a_listed_task_carries_its_project_and_no_priority_unsorted() {
    let listing = listing(Reach::Project(project()), vec![task()], Shown::default());
    let value = listed(&listing, false, None, &catalog());
    assert_eq!(value.get("path"), None);
    assert_eq!(value["project_id"], "jolly-mute-magma");
    assert_eq!(value.get("warning"), None);
    assert_eq!(value.get("global"), None);
    assert_eq!(value.get("compact_tasks"), None);
    assert_eq!(value.get("summary"), None);
    let row = &value["tasks"][0];
    assert_eq!(row["harp_id"], "swift-amber");
    assert_eq!(row["project_id"], "jolly-mute-magma");
    assert_eq!(row["project_dir"], "/work/tree");
    assert_eq!(row["derived_priority"], json!(null));
    assert_eq!(row["created_at"], "2026-06-10T02:35:20Z");
}

// A listing sorted by priority: each task's derived priority, and the
// warnings, one line each, in `priority_warning`.
#[test]
fn test_a_listing_sorted_by_priority_carries_each_priority_and_its_warnings() {
    let mut listing = listing_of(Reach::Project(project()), vec![task(), task()]);
    listing.priorities = Some(vec![5.0, 2.5]);
    listing.priority_notes = vec![
        PriorityNote {
            project: None,
            warning: PriorityWarning::AllTied { scored: 0, live: 2 },
        },
        PriorityNote {
            project: Some("p".to_string()),
            warning: PriorityWarning::Inert {
                target: "a:b".to_string(),
                live: 2,
            },
        },
    ];
    let mut sources = vec![crate::EN_US];
    sources.extend(tasks_application::CATALOGS);
    let catalog = presentation::Catalog::en_us(&sources).unwrap();
    let value = listed(&listing, false, None, &catalog);
    assert_eq!(value["tasks"][0]["derived_priority"], json!(5));
    assert_eq!(value["tasks"][1]["derived_priority"], json!(2.5));
    let warning = value["priority_warning"].as_str().unwrap();
    let lines: Vec<&str> = warning.lines().collect();
    assert_eq!(lines.len(), 2, "{warning}");
    assert!(lines[0].starts_with("--sort priority is meaningless here"));
    assert!(lines[1].starts_with("project p: priority_fn/decay_fn reads a:b"));
    let unsorted = listed(
        &listing_of(Reach::Project(project()), vec![task()]),
        false,
        None,
        &catalog,
    );
    assert_eq!(unsorted.get("priority_warning"), None);
}

// A compact listing: the compact rows, and `tasks` null.
#[test]
fn test_a_compact_listing_is_each_tasks_headline_and_tasks_null() {
    let mut long = task();
    long.text = "First line\nthe rest".to_string();
    let listing = listing(Reach::Project(project()), vec![long], Shown::default());
    let value = listed(&listing, true, None, &catalog());
    assert_eq!(value["tasks"], json!(null));
    assert_eq!(
        value["compact_tasks"],
        json!([{
            "harp_id": "swift-amber", "status": "To Do", "checked": false, "tags": ["a"],
            "headline": "First line", "project_id": "jolly-mute-magma", "project_dir": "/work/tree",
        }])
    );
    let empty = listing_of(Reach::Project(project()), Vec::new());
    let value = listed(&empty, true, None, &catalog());
    assert_eq!(
        value.get("compact_tasks"),
        None,
        "omitted when empty, as Go's"
    );
}

fn listing_of(reach: Reach, tasks: Vec<Task>) -> Listing {
    listing(reach, tasks, Shown::default())
}

#[test]
fn test_a_listing_says_what_it_hid_and_what_its_limit_left_out() {
    let mut listing = listing_of(Reach::Project(project()), vec![task()]);
    listing.hidden = Hidden {
        completed: 2,
        deferred: 1,
    };
    listing.omitted = 4;
    let value = listed(&listing, false, None, &catalog());
    assert_eq!(value["hidden_completed"], 2);
    assert_eq!(value["hidden_deferred"], 1);
    assert_eq!(value["omitted_by_limit"], 4);
}

#[test]
fn test_a_summary_comes_with_the_listing_when_asked_for() {
    let listing = listing_of(Reach::Project(project()), vec![task()]);
    let summary = Summary {
        reach: Reach::Project(project()),
        counts: [("To Do".to_string(), 1)].into(),
        in_progress: Vec::new(),
    };
    let value = listed(&listing, false, Some(&summary), &catalog());
    assert_eq!(
        value["summary"],
        json!({"counts": {"To Do": 1}, "in_progress": []})
    );
}

#[test]
fn test_an_added_task_is_answered_with_its_project_and_no_store() {
    let written = Written {
        dead_letters: Vec::new(),
        project: project(),
        task: task(),
        shown: Shown::default(),
    };
    let value = super::written(&written);
    assert_eq!(value.get("path"), None);
    assert_eq!(value["project_id"], "jolly-mute-magma");
    assert_eq!(value["project_dir"], "/work/tree");
    assert_eq!(value["task"]["tags"], json!(["a"]));
    assert_eq!(value["task"]["derived_priority"], json!(null));
}

// A project with no root (a pinned one the registry holds no path for)
// still writes `project_dir`, as "": the JSON a program reads keeps its
// shape, as it always has.
#[test]
fn test_a_project_with_no_root_is_answered_with_an_empty_project_dir() {
    let rootless = Project {
        root: None,
        ..project()
    };
    let written = Written {
        dead_letters: Vec::new(),
        project: rootless.clone(),
        task: task(),
        shown: Shown::default(),
    };
    assert_eq!(super::written(&written)["project_dir"], json!(""));
    let mut listing = listing(
        Reach::Project(rootless.clone()),
        vec![task()],
        Shown::default(),
    );
    listing.rows[0].project = rootless;
    let value = listed(&listing, false, None, &catalog());
    assert_eq!(value["project_dir"], json!(""));
    assert_eq!(value["tasks"][0]["project_dir"], json!(""));
}

// A tool's result is read by a program, as the CLI's JSON is: each tag in
// its canonical spelling, and the tags a person is not shown kept.
#[test]
fn test_a_result_spells_every_tag_canonically_hidden_ones_too() {
    let mut task = task();
    task.tags = vec![tag("triage:cwe=79"), tag("x=\"y z\""), tag("a")];
    let shown = hiding(&[r#"tagma.hide:"triage:cwe"=true"#]);
    let spelled = json!(["triage:cwe=79", "x=\"y z\"", "a"]);
    let wrote = Written {
        dead_letters: Vec::new(),
        project: project(),
        task: task.clone(),
        shown: shown.clone(),
    };
    assert_eq!(written(&wrote)["task"]["tags"], spelled);
    let listing = listing(Reach::Project(project()), vec![task], shown);
    assert_eq!(
        listed(&listing, false, None, &catalog())["tasks"][0]["tags"],
        spelled
    );
}

#[test]
fn test_a_listing_of_no_project_says_so_and_lists_nothing() {
    let value = listed(
        &listing_of(Reach::NoProject, Vec::new()),
        false,
        None,
        &catalog(),
    );
    assert_eq!(
        value,
        json!({"tasks": [], "warning": "no project here yet: nothing to list"})
    );
}

// The no-project fallback: outside every work root a listing reads every
// project, says so in `global` and `project_count`, and says why in
// `notice`; each row carries its own project.
#[test]
fn test_a_listing_of_every_project_says_so_and_why_when_it_fell_back() {
    let fell_back = Reach::Every {
        projects: 3,
        fallback: Some("/tmp/somewhere".to_string()),
    };
    let value = listed(
        &listing_of(fell_back, vec![task()]),
        false,
        None,
        &catalog(),
    );
    assert_eq!(value.get("project_id"), None);
    assert_eq!(value.get("warning"), None);
    assert_eq!(value["global"], true);
    assert_eq!(value["project_count"], 3);
    assert!(
        value["notice"]
            .as_str()
            .is_some_and(|n| n.starts_with("no project here: /tmp/somewhere is in no git")),
        "{value}"
    );
    assert_eq!(value["tasks"][0]["project_id"], "jolly-mute-magma");
    let asked = Reach::Every {
        projects: 3,
        fallback: None,
    };
    let value = listed(&listing_of(asked, Vec::new()), false, None, &catalog());
    assert_eq!(value["global"], true);
    assert_eq!(
        value.get("notice"),
        None,
        "an explicit global needs no notice"
    );
}
