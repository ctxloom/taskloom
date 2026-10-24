//! The wiring's contract: how the binary (one service per command), the
//! MCP server and a gRPC server (one per call, on threads and spawned
//! futures) and ctxloom build the use cases from it, over a temporary home
//! and project, never a real one.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;

use annals_diagnostic::Diagnostic;
use tasks_application::{
    Changed, ImportRequest, ImportService, ListQuery, NewTask, PruneService, Reach, TaskService,
    WatchRequest, WatchService,
};
use tempfile::TempDir;

use super::*;

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(future)
}

/// An isolated home and project directory.
struct Dirs {
    home: TempDir,
    project: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            home: tempfile::tempdir().unwrap(),
            project: tempfile::tempdir().unwrap(),
        }
    }

    /// The wiring in `dir`, under the home, and nothing else.
    fn builder_in(&self, dir: &Path) -> Builder {
        Builder::new(dir).home(self.home.path())
    }

    /// The wiring in the project directory.
    fn taskloom(&self) -> Taskloom {
        self.builder_in(self.project.path()).build().unwrap()
    }
}

fn new_task(text: &str) -> NewTask {
    NewTask {
        text: text.to_string(),
        ..NewTask::default()
    }
}

fn texts(listing: &tasks_application::Listing) -> Vec<String> {
    let mut texts: Vec<String> = listing.tasks().map(|t| t.text.clone()).collect();
    texts.sort();
    texts
}

/// What a server's factory must be: a maker of services that crosses
/// threads and outlives the wiring, each service every use case a frontend
/// calls, its futures `Send`. MCP's `TaskTools::new` and a gRPC server ask
/// no more.
fn served<F, S>(factory: F) -> F
where
    F: Fn() -> S + Send + Sync + 'static,
    S: TaskService + PruneService + WatchService + Send + 'static,
{
    factory
}

// The binary's path: one service per command, each resolving the project
// the working directory is, so two commands share a store.
#[test]
fn test_a_task_added_by_one_service_is_listed_by_the_next() {
    let dirs = Dirs::new();
    let taskloom = dirs.taskloom();
    let written = block_on(taskloom.tasks().add(new_task("Ship it"))).unwrap();
    let listing = block_on(taskloom.tasks().list(ListQuery::default())).unwrap();
    assert_eq!(texts(&listing), ["Ship it"]);
    assert_eq!(listing.reach, Reach::Project(written.project));
}

// MCP's path: each call on a service the factory makes for it, on a
// blocking thread of its own; the factory outlives the wiring it came from.
#[test]
fn test_the_factory_makes_a_service_per_call_on_threads_of_their_own() {
    let dirs = Dirs::new();
    let factory = Arc::new(served(dirs.taskloom().services()));
    let calls: Vec<_> = (0..4)
        .map(|n| {
            let factory = Arc::clone(&factory);
            std::thread::spawn(move || block_on(factory().add(new_task(&format!("call {n}")))))
        })
        .collect();
    for call in calls {
        call.join().unwrap().unwrap();
    }
    let listing = block_on(factory().list(ListQuery::default())).unwrap();
    assert_eq!(texts(&listing), ["call 0", "call 1", "call 2", "call 3"]);
}

// A gRPC server's path: a call's future is spawned on the server's
// runtime, so it must be `Send` and own its service.
#[test]
fn test_a_calls_future_can_be_spawned_on_a_runtime() {
    let dirs = Dirs::new();
    let factory = served(dirs.taskloom().services());
    let added = block_on(async move {
        tokio::spawn(async move { factory().add(new_task("spawned")).await })
            .await
            .expect("the call ran")
    });
    assert_eq!(added.unwrap().task.text, "spawned");
}

// A change event is a typed value: a watch on one service sees what
// another service wrote.
#[test]
fn test_a_watch_sees_what_another_service_writes() {
    let dirs = Dirs::new();
    let taskloom = dirs.taskloom();
    let mut watcher = taskloom.tasks();
    assert!(
        block_on(watcher.watch(WatchRequest::default()))
            .unwrap()
            .is_none(),
        "no project yet: a watch binds none"
    );
    block_on(taskloom.tasks().add(new_task("first"))).unwrap();
    let mut watch = block_on(watcher.watch(WatchRequest::default()))
        .unwrap()
        .expect("the project the add minted");
    block_on(taskloom.tasks().add(new_task("second"))).unwrap();
    let changes = block_on(watcher.poll(&mut watch)).unwrap();
    let texts: Vec<_> = changes
        .iter()
        .map(|c| match c {
            Changed::Task(task) => task.text.clone(),
            Changed::Removed { harp_id } => format!("removed {harp_id}"),
        })
        .collect();
    assert_eq!(texts, ["second"]);
}

// CTXLOOM_PROJECT_ID, given to the builder, beats the working directory's
// project, as it does in the binary and the MCP server.
#[test]
fn test_a_pinned_project_beats_the_working_directorys() {
    let dirs = Dirs::new();
    let there = block_on(dirs.taskloom().tasks().add(new_task("there")))
        .unwrap()
        .project;
    let elsewhere = tempfile::tempdir().unwrap();
    let pinned = dirs
        .builder_in(elsewhere.path())
        .project_id(there.id.clone())
        .build()
        .unwrap();
    let written = block_on(pinned.tasks().add(new_task("pinned"))).unwrap();
    assert_eq!(written.project.id, there.id);
}

// The variables given are the configuration's environment layer.
#[test]
fn test_the_configuration_reads_the_variables_given() {
    let dirs = Dirs::new();
    let tagged = |taskloom: &Taskloom| {
        block_on(taskloom.tasks().add(NewTask {
            tags: vec!["triage:level=7".to_string()],
            ..new_task("Ship it")
        }))
    };
    assert!(tagged(&dirs.taskloom()).is_err(), "the default range, 1-5");
    let wide = dirs
        .builder_in(dirs.project.path())
        .vars(vec![(
            "TASKLOOM_CONFIG_TAG_SCHEMA".to_string(),
            r#"tagma.range:"triage:level"="1,9""#.to_string(),
        )])
        .build()
        .unwrap();
    assert!(tagged(&wide).is_ok());
}

// Import only on request, over the home's JSONL task logs, into the stores
// the tasks use cases read.
#[test]
fn test_the_importer_reads_the_homes_logs_into_the_stores_tasks_reads() {
    let dirs = Dirs::new();
    let logs = dirs.home.path().join(".ctxloom/tasks");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(
        logs.join("jolly-mute-magma.jsonl"),
        concat!(
            r#"{"op":"add","task":"brisk-heron","text":"Write the tour","status":"To Do","#,
            r#""session":"calm-otter","ts":"2026-03-01T10:00:00.123456789Z"}"#,
            "\n"
        ),
    )
    .unwrap();
    let taskloom = dirs.taskloom();
    let report = block_on(taskloom.importer().import(ImportRequest::default())).unwrap();
    assert!(report.complete(), "{report:?}");
    let listing = block_on(taskloom.tasks().list(ListQuery {
        project: Some("jolly-mute-magma".to_string()),
        ..ListQuery::default()
    }))
    .unwrap();
    assert_eq!(texts(&listing), ["Write the tour"]);
}

// The project use cases run over the same registry and stores.
#[test]
fn test_projects_shows_the_project_an_add_minted() {
    let dirs = Dirs::new();
    let taskloom = dirs.taskloom();
    let written = block_on(taskloom.tasks().add(new_task("Ship it"))).unwrap();
    let shown = taskloom.projects().show(None).unwrap();
    assert_eq!(shown.store, Some(written.project.store));
}

// A registration's command is resolved on the PATH given, and on no other.
#[test]
fn test_registrations_resolve_the_server_on_the_path_given() {
    let dirs = Dirs::new();
    let bin = tempfile::tempdir().unwrap();
    let server = bin.path().join("taskloom");
    std::fs::write(&server, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let on_path = dirs
        .builder_in(dirs.project.path())
        .path(bin.path())
        .build()
        .unwrap();
    assert!(
        on_path
            .registrations()
            .check(dirs.project.path())
            .resolvable
    );
    assert!(
        !dirs
            .taskloom()
            .registrations()
            .check(dirs.project.path())
            .resolvable,
        "no PATH given: the process's is not read"
    );
}

// The filesystem given is the one the wiring holds.
#[test]
fn test_the_filesystem_given_is_the_one_the_wiring_holds() {
    let dirs = Dirs::new();
    let given: Arc<dyn Fs> = Arc::new(file_system::Rooted::temp().unwrap());
    let taskloom = dirs
        .builder_in(dirs.project.path())
        .fs(Arc::clone(&given))
        .build()
        .unwrap();
    assert!(Arc::ptr_eq(taskloom.fs(), &given));
    assert!(
        Arc::ptr_eq(taskloom.clone().fs(), &given),
        "a clone shares it"
    );
}

// With no filesystem given, the real one, which refuses a working
// directory that is not absolute: a diagnostic of this crate's, which the
// catalog renders.
#[test]
fn test_a_relative_working_directory_is_refused_as_a_catalogued_diagnostic() {
    let refused = Builder::new("relative").build().unwrap_err();
    assert_eq!(
        refused,
        BuildError::RelativeCwd {
            cwd: "relative".to_string()
        }
    );
    assert_eq!(refused.code(), "tasks-lib-relative-cwd");
    assert_eq!(
        catalog(&[]).unwrap().render(&refused).unwrap(),
        "the working directory is not an absolute path: relative"
    );
}

/// Every diagnostic the wiring can raise: its own, annals', the use
/// cases', and every driven and platform adapter's.
fn wired_samples() -> Vec<Box<dyn Diagnostic>> {
    [
        crate::diagnostic_samples(),
        annals::diagnostic_samples(),
        annals_store_sql::diagnostic_samples(),
        tasks_application::diagnostic_samples(),
        project_application::diagnostic_samples(),
        registration_application::diagnostic_samples(),
        project_registry::diagnostic_samples(),
        task_store_sqlite::diagnostic_samples(),
        jsonl_task_log::diagnostic_samples(),
        tasks_config::diagnostic_samples(),
        agent_configs::diagnostic_samples(),
        file_lock::diagnostic_samples(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[test]
fn test_the_catalog_renders_every_diagnostic_the_wiring_can_raise() {
    let catalog = catalog(&[]).unwrap();
    for sample in wired_samples() {
        assert!(
            catalog.render(&*sample).is_ok(),
            "{} has no message",
            sample.code()
        );
    }
}

// A frontend's own messages are its to add; the wiring knows none.
#[test]
fn test_a_frontend_adds_its_own_messages_to_the_catalog() {
    let samples = project_cli::diagnostic_samples();
    let bare = catalog(&[]).unwrap();
    assert!(samples.iter().all(|s| bare.render(&**s).is_err()));
    let with_cli = catalog(&[project_cli::EN_US]).unwrap();
    for sample in &samples {
        assert!(with_cli.render(&**sample).is_ok(), "{}", sample.code());
    }
}
