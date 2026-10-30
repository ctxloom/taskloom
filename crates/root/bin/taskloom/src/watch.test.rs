//! Acceptance: `taskloom watch`, the real binary in a temporary home: its
//! first line at once, a line for a task another process adds, one JSON
//! document a line, and a clean exit at SIGINT or SIGTERM (on Windows, at
//! a Ctrl-Break sent to its process group). Each watch runs
//! with stdin from /dev/null, as the VS Code extension starts it, and a
//! closed stdin never stops it; a test that ends with it still running
//! kills it (its own child, by pid), and one whose stop a mutant broke is
//! killed by the mutation gate, which kills whatever its run leaves. The
//! test waits on
//! the stream's lines (each wait bounded, so a stream that never answers
//! fails rather than hangs), never on a sleep. Then the stream in
//! process, its ticks injected, over a prune's halves.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use serde_json::Value;

use super::{binary, disk, one_document, Env, Isolated};

/// How long a line may take before the stream is judged not to answer.
const PATIENCE: Duration = Duration::from_secs(30);

/// A running `taskloom watch`, killed if a test ends with it still running.
pub(super) struct Watching {
    child: Option<Child>,
    lines: Receiver<String>,
}

impl Watching {
    pub(super) fn start(env: &Env, args: &[&str]) -> Watching {
        let mut command = Command::new(binary());
        // A process group of its own on Windows, so a Ctrl-Break sent to
        // it (`ctrl_break`) reaches the watch alone.
        let mut child = os::own_group(&mut command)
            .arg("watch")
            .args(args)
            .current_dir(env.project.path())
            .isolated(env.home.path())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            // As the VS Code extension spawns it: stdin is /dev/null, closed
            // from the start.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("taskloom watch starts");
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Watching {
            child: Some(child),
            lines,
        }
    }

    /// The stream's next line.
    pub(super) fn line(&self) -> String {
        self.lines
            .recv_timeout(PATIENCE)
            .unwrap_or_else(|e| panic!("no line from watch: {e}"))
    }

    /// Send `signal` and wait for the process to end; whatever it wrote
    /// after the lines already read must be nothing. Its status, on unix.
    ///
    /// Windows has no signal to send one process: there the process is
    /// killed, its status says nothing of a clean end, and `None` says so.
    /// (Its clean stop at a Ctrl-Break sent to its group is
    /// `ctrl_break`'s.)
    pub(super) fn stop(mut self, signal: &str) -> Option<ExitStatus> {
        let mut child = self.child.take().unwrap();
        if os::host().flavor() != os::Flavor::Unix {
            child.kill().expect("watch is killed");
            let status = child.wait().expect("watch ends");
            self.finish(status);
            return None;
        }
        let sent = Command::new("kill")
            .args([&format!("-{signal}"), &child.id().to_string()])
            .status()
            .expect("kill runs");
        assert!(sent.success(), "kill -{signal}");
        let status = child.wait().expect("watch ends");
        Some(self.finish(status))
    }

    /// Send a Ctrl-Break to the process's group (Windows) and wait for it
    /// to end; whatever it wrote after the lines already read must be
    /// nothing. Its status.
    fn ctrl_break(mut self) -> ExitStatus {
        let mut child = self.child.take().unwrap();
        os::ctrl_break(&child).expect("a Ctrl-Break is sent");
        let status = child.wait().expect("watch ends");
        self.finish(status)
    }

    /// Whether the process is still running.
    fn alive(&mut self) -> bool {
        let child = self.child.as_mut().unwrap();
        child.try_wait().expect("the watch's status").is_none()
    }

    /// `status`, once nothing more was written.
    fn finish(&self, status: ExitStatus) -> ExitStatus {
        match self.lines.recv_timeout(PATIENCE) {
            Err(RecvTimeoutError::Disconnected) => {}
            other => panic!("watch wrote more, or its stdout stayed open: {other:?}"),
        }
        status
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The stream's line, for `project`.
fn event(project: &str) -> Value {
    serde_json::json!({"event": "changed", "kind": "tasks", "project": project})
}

#[test]
fn test_watch_writes_a_line_at_once_and_one_for_a_task_another_process_adds() {
    let env = Env::new("watch-harp");
    env.seed("Before");
    let shown = env.run(&["project", "show", "--json"]);
    let project = one_document(&shown.stdout)["project_id"]
        .as_str()
        .expect("the project")
        .to_string();

    let watching = Watching::start(&env, &[]);
    // A subscriber renders the current state from the first line, so it
    // comes before any change; one JSON document a line.
    assert_eq!(one_document(&watching.line()), event(&project));

    let added = env.run(&["add", "After"]);
    assert!(added.ok, "{}", added.stderr);
    assert_eq!(one_document(&watching.line()), event(&project));

    if let Some(status) = watching.stop("INT") {
        assert!(status.success(), "SIGINT is a clean end: {status}");
    }
}

#[test]
fn test_watch_ends_cleanly_at_sigterm_and_watches_a_pinned_project() {
    let env = Env::new("watch-term-harp");
    env.seed("Seed");
    let shown = env.run(&["project", "show", "--json"]);
    let project = one_document(&shown.stdout)["project_id"]
        .as_str()
        .unwrap()
        .to_string();
    let elsewhere = Env::new("watch-term-elsewhere");
    let watching = Watching::start(&elsewhere, &["--project", &project, "--json"]);
    assert_eq!(one_document(&watching.line()), event(&project));
    if let Some(status) = watching.stop("TERM") {
        assert!(status.success(), "SIGTERM is a clean end: {status}");
    }
}

/// Go's watch stops at os.Interrupt and SIGTERM, which on Windows are
/// Ctrl-C and Ctrl-Break, and a console's close, logoff and shutdown. A
/// Ctrl-Break is the one another process can send (to a process group):
/// the watch stops at it cleanly rather than being ended by the default
/// handler. Windows alone: unix has no console events.
#[test]
fn test_watch_ends_cleanly_at_a_ctrl_break_on_windows() {
    if os::host().flavor() != os::Flavor::Windows {
        return;
    }
    let env = Env::new("watch-break-harp");
    env.seed("Seed");
    let watching = Watching::start(&env, &[]);
    assert_eq!(one_document(&watching.line())["event"], "changed");
    let status = watching.ctrl_break();
    assert!(status.success(), "a Ctrl-Break is a clean end: {status}");
}

#[test]
fn test_watch_where_there_is_no_project_says_so_and_binds_none() {
    let env = Env::new("watch-none-harp");
    let ran = env.run(&["watch", "--format", "text"]);
    assert!(!ran.ok);
    assert_eq!(ran.stdout, "");
    assert_eq!(
        ran.stderr,
        "taskloom: no project here yet; add a task first (taskloom add \"<task text>\")\n"
    );
    assert!(
        !disk::exists(env.home.path().join(".ctxloom")),
        "a watch binds no project and creates nothing"
    );

    let ran = env.run(&["watch", "--json"]);
    assert!(!ran.ok);
    assert_eq!(ran.stdout, "", "an error leaves stdout empty");
    assert_eq!(one_document(&ran.stderr)["code"], "tasks-watch-no-project");
}

#[test]
fn test_watch_with_stdin_from_dev_null_stays_up_and_answers_each_change() {
    // The VS Code extension spawns `taskloom watch` with stdin ignored: a
    // watch that ended at a closed stdin wrote one line and exited, and
    // its tasks never updated live.
    let env = Env::new("watch-devnull-harp");
    env.seed("Seed");
    let mut watching = Watching::start(&env, &[]);
    assert_eq!(one_document(&watching.line())["event"], "changed");

    let added = env.run(&["add", "Another"]);
    assert!(added.ok, "{}", added.stderr);
    assert_eq!(one_document(&watching.line())["event"], "changed");
    assert!(watching.alive(), "a closed stdin does not end the watch");

    if let Some(status) = watching.stop("TERM") {
        assert!(status.success(), "SIGTERM is a clean end: {status}");
    }
}

// ------------------------------------------------- in process, tick by tick

/// The stream in process, over the binary's own store and use cases, its
/// ticks injected: each tick first does one step to the database through
/// a handle of its own (another process's), then the stream polls, so
/// what lands between which polls is chosen, not raced for. A burst's line
/// is written at its first quiet tick.
mod ticked {
    use std::collections::VecDeque;
    use std::future::poll_fn;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::task::Poll;

    use angzarr_embedded::next::v1::event_meta::Causation;
    use angzarr_embedded::next::v1::{
        command_page, Command, CommandBook, CommandPage, Cover, EventMeta, EventPage, Uuid,
    };
    use angzarr_embedded::next::{
        ids, Any, EventStore, Kernel, MemoryDeadLetters, Payload, Purge, Sampled, StreamRef,
        Timestamp, Write,
    };
    use angzarr_store_sqlite::SqliteStore;
    use harp_names::HarpNames;
    use ports::{Project, ProjectIdentity};
    use system::System;
    use task_store_sqlite::SqliteStores;
    use tasks::{AddTask, RemovalReason, RemoveTask, TaskAggregate, TaskCommand, TASK_DOMAIN};
    use tasks_application::{ListQuery, NewTask, Service, TaskService};
    use tasks_watch::{Settings, Ticks};

    /// The one project, wherever the call is from.
    struct Here(Project);

    impl ProjectIdentity for Here {
        fn for_write(
            &mut self,
            _: Option<&str>,
        ) -> Result<Project, angzarr_embedded_diagnostic::Report> {
            Ok(self.0.clone())
        }
        fn for_read(
            &mut self,
            _: Option<&str>,
        ) -> Result<Option<Project>, angzarr_embedded_diagnostic::Report> {
            Ok(Some(self.0.clone()))
        }
        fn every_project(&mut self) -> Result<Vec<Project>, angzarr_embedded_diagnostic::Report> {
            Ok(vec![self.0.clone()])
        }
        fn rootless_directory(
            &mut self,
        ) -> Result<Option<String>, angzarr_embedded_diagnostic::Report> {
            Ok(None)
        }
    }

    type Real = Service<Here, SqliteStores, HarpNames, System, System>;

    fn service(project: &Project) -> Real {
        Service::new(
            Here(project.clone()),
            SqliteStores::new(crate::tests::os()),
            HarpNames,
            System,
            System,
        )
    }

    /// What another process does before a tick's poll.
    #[derive(Clone, Debug)]
    enum Step {
        /// Nothing: a quiet tick.
        Quiet,
        /// A prune's removal of the task, without its purge.
        Prune(String),
        /// A prune's purge of the task's stream, after its removal.
        Purge(String),
        /// A prune whole: its removal, then its purge.
        Pruned(String),
        /// A prune whole, then a task minted on the freed harp with this
        /// text: the harp's stream started again.
        Reminted(String, String),
        /// A commit outside the task domain: the store's token moves, and
        /// no task changes.
        Elsewhere,
    }

    fn task_stream(harp: &str) -> Cover {
        Cover {
            domain: TASK_DOMAIN.to_string(),
            root: ids::compute_root(TASK_DOMAIN, harp).ok(),
            edition: None,
        }
    }

    /// Run `command` on `harp`'s stream in `store`, as command `id`.
    async fn command(store: &SqliteStore, harp: &str, command: TaskCommand, id: u8) {
        let book = CommandBook {
            cover: Some(task_stream(harp)),
            page: Some(CommandPage {
                body: Some(command_page::Body::Command(Command {
                    payload: Some(command.to_any()),
                })),
                ..CommandPage::default()
            }),
            handling: None,
        };
        let registry = tasks_application::registry();
        let dead_letters = MemoryDeadLetters::new();
        let sampled = Sampled {
            now: Timestamp::default(),
            command_id: Uuid::from_bytes([id; 16]),
        };
        Kernel::new(&registry, &dead_letters)
            .on_retryable(tasks_application::KERNEL_RETRY)
            .handle::<TaskAggregate, _, _>(store, book, &sampled, &mut ())
            .await
            .unwrap_or_else(|failure| panic!("{failure:?}"));
    }

    impl Step {
        async fn apply(self, store: &SqliteStore, elsewhere: &mut u64) {
            match self {
                Step::Quiet => {}
                Step::Prune(harp) => {
                    let removal = RemoveTask {
                        reason: RemovalReason::Pruned,
                    };
                    command(store, &harp, removal.into(), 0xee).await;
                }
                Step::Reminted(harp, text) => {
                    Box::pin(Step::Pruned(harp.clone()).apply(store, elsewhere)).await;
                    let add = AddTask {
                        harp_id: harp.clone(),
                        text,
                        status: None,
                        trigger: None,
                        tags: Vec::new(),
                        origin_session: None,
                        defers: tasks::DEFERRED.to_string(),
                        initial: tasks::TO_DO.to_string(),
                    };
                    command(store, &harp, add.into(), 0xdd).await;
                }
                Step::Pruned(harp) => {
                    Box::pin(Step::Prune(harp.clone()).apply(store, elsewhere)).await;
                    Box::pin(Step::Purge(harp).apply(store, elsewhere)).await;
                }
                Step::Purge(harp) => {
                    let stream = task_stream(&harp);
                    let stream = StreamRef::try_from(&stream).unwrap();
                    let head = store.head(stream).await.unwrap();
                    let deleted = store.purge(stream, head.next_sequence).await.unwrap();
                    assert!(deleted > 0, "the stream was there to purge");
                }
                Step::Elsewhere => {
                    let other = Cover {
                        domain: "elsewhere".to_string(),
                        root: Some(Uuid::from_bytes([7; 16])),
                        edition: None,
                    };
                    let id = Uuid::from_bytes([*elsewhere as u8 + 1; 16]);
                    let page = EventPage {
                        payload: Some(Any {
                            type_url: "/elsewhere.v1.Event".to_string(),
                            value: Vec::new(),
                        }),
                        meta: Some(EventMeta {
                            causation: Some(Causation::CommandId(id.clone())),
                            correlation_id: Some(id),
                            recorded_at: Some(Timestamp::default()),
                            rules_version: "v1".to_string(),
                            ..EventMeta::default()
                        }),
                        sequence: *elsewhere,
                    };
                    let stream = StreamRef::try_from(&other).unwrap();
                    store
                        .write(Write::append(stream, *elsewhere, &[page]))
                        .await
                        .unwrap();
                    *elsewhere += 1;
                }
            }
        }
    }

    /// One step before each tick; once they run out, the stream is told
    /// to stop.
    struct Scripted {
        steps: VecDeque<Step>,
        other: SqliteStore,
        elsewhere: u64,
        done: Arc<AtomicBool>,
    }

    impl Ticks for Scripted {
        async fn tick(&mut self) {
            match self.steps.pop_front() {
                Some(step) => step.apply(&self.other, &mut self.elsewhere).await,
                None => {
                    self.done.store(true, Ordering::SeqCst);
                    poll_fn(|cx| {
                        cx.waker().wake_by_ref();
                        Poll::<()>::Pending
                    })
                    .await;
                }
            }
        }
    }

    /// The lines the stream writes over `steps`, for a project holding
    /// one task (`harp`) at the start; and the project's tasks after.
    fn watched(steps: impl Fn(&str) -> Vec<Step>) -> (usize, Vec<String>) {
        let dir = crate::tests::Temp::new();
        let project = Project {
            id: "jolly-mute-magma".to_string(),
            root: Some(dir.path().display().to_string()),
            store: dir.path().join("state.db"),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let mut seeding = service(&project);
            let new = |text: &str| NewTask {
                text: text.to_string(),
                ..NewTask::default()
            };
            let harp = seeding.add(new("Pruned")).await.unwrap().task.harp_id;
            seeding.add(new("Kept")).await.unwrap();

            let done = Arc::new(AtomicBool::new(false));
            let mut ticks = Scripted {
                steps: steps(&harp).into(),
                other: SqliteStore::open(&project.store).await.unwrap(),
                elsewhere: 0,
                done: Arc::clone(&done),
            };
            let stop = poll_fn(|_| {
                if done.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            });
            let settings = Settings {
                format: presentation::Format::Json,
                project: None,
                prog: "taskloom",
            };
            let catalog = super::super::super::catalog().unwrap();
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let mut watching = service(&project);
            let code = tasks_watch::run(
                &mut watching,
                &settings,
                &mut ticks,
                stop,
                &catalog,
                &mut out,
                &mut err,
            )
            .await;
            assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
            let lines = String::from_utf8(out).unwrap().lines().count();

            let listing = service(&project)
                .list(ListQuery {
                    all: true,
                    ..ListQuery::default()
                })
                .await
                .unwrap();
            let texts = listing.rows.iter().map(|r| r.task.text.clone()).collect();
            (lines, texts)
        })
    }

    #[test]
    fn test_a_prune_between_two_polls_is_one_line() {
        let (lines, tasks) = watched(|harp| vec![Step::Pruned(harp.to_string()), Step::Quiet]);
        assert_eq!(lines, 2, "the first line, and the prune's");
        assert_eq!(tasks, ["Kept"]);
    }

    #[test]
    fn test_a_poll_between_a_prunes_removal_and_its_purge_is_one_line() {
        let (lines, tasks) = watched(|harp| {
            vec![
                Step::Prune(harp.to_string()),
                Step::Quiet,
                Step::Purge(harp.to_string()),
                Step::Quiet,
            ]
        });
        // Read once between, at the removal; the quiet tick after has
        // nothing to read, and the purge's read finds nothing new.
        assert_eq!(lines, 2, "the first line, and the removal's");
        assert_eq!(tasks, ["Kept"]);
    }

    #[test]
    fn test_a_second_read_between_a_prunes_removal_and_its_purge_is_no_line() {
        // The store read again after the removal, before the purge (its
        // token moved for a commit no task sees): the pruned stream is the
        // one already projected, so nothing is new.
        let (lines, tasks) = watched(|harp| {
            vec![
                Step::Prune(harp.to_string()),
                Step::Quiet,
                Step::Elsewhere,
                Step::Quiet,
                Step::Purge(harp.to_string()),
                Step::Quiet,
                Step::Quiet,
            ]
        });
        assert_eq!(lines, 2, "the first, and the removal's");
        assert_eq!(tasks, ["Kept"]);
    }

    #[test]
    fn test_a_commit_no_task_sees_is_no_line() {
        let (lines, tasks) = watched(|_| vec![Step::Elsewhere, Step::Quiet, Step::Elsewhere]);
        assert_eq!(lines, 1, "only the first");
        assert_eq!(tasks, ["Pruned", "Kept"]);
    }

    #[test]
    fn test_a_prune_and_a_mint_on_its_harp_between_two_polls_is_the_new_task() {
        // The harp freed and minted again unseen: the stream restarted,
        // shorter than the one the watch last read.
        let (lines, tasks) = watched(|harp| {
            vec![
                Step::Reminted(harp.to_string(), "Again".to_string()),
                Step::Quiet,
            ]
        });
        assert_eq!(lines, 2, "the first, and the new task's");
        let mut tasks = tasks;
        tasks.sort();
        assert_eq!(tasks, ["Again", "Kept"]);
    }
}
