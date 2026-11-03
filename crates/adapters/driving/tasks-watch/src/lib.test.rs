//! The stream over a scripted watch: each tick is driven by the test and
//! each poll answers what the script says was committed before it, so the
//! interleaving of commits and ticks is the test's, never the clock's.

use std::cell::Cell;
use std::collections::VecDeque;
use std::future::{poll_fn, Future};
use std::path::PathBuf;
use std::rc::Rc;
use std::task::Poll;
use std::time::{Duration, Instant};

use angzarr_diagnostic::{code, Diagnostic, Report};
use angzarr_presentation::{Catalog, Format};
use serde_json::{json, Value};
use tasks_application::{
    AppError, Found, Positions, Project, Seen, StreamKey, Watch, WatchRequest, WatchService,
};
use uuid::Uuid;

use super::cursor::{encode, name};
use super::{
    after_help, heartbeat_ticks, long_about, run, Args, Group, Interval, Settings, Ticks, POLL,
};

/// What the long help says on each host, with the departures the human
/// reviewed.
#[path = "../fixtures/reviewed.rs"]
mod reviewed;

/// Run a future whose every await is ready at once: the test support's.
use angzarr_embedded::testing::block_on_ready as ready;

fn project() -> Project {
    Project {
        id: "jolly-mute-magma".to_string(),
        root: Some("/work/tree".to_string()),
        store: PathBuf::from("/home/.ctxloom/projects/jolly-mute-magma/state.db"),
    }
}

const A: Uuid = Uuid::from_u128(1);
const B: Uuid = Uuid::from_u128(2);

fn stream_of(root: Uuid) -> StreamKey {
    StreamKey {
        edition: None,
        root,
    }
}

/// An event of the task `harp` on the stream rooted at `root`.
fn event(root: Uuid, sequence: u64, harp: &str, text: &str, tags: &[&str]) -> Found {
    Found::Event {
        stream: stream_of(root),
        sequence,
        task: Seen {
            harp_id: harp.to_string(),
            text: text.to_string(),
            status: "To Do".to_string(),
            tags: tags.iter().map(ToString::to_string).collect(),
            removed: false,
        },
    }
}

/// A watch whose polls answer, in turn, what the script holds; then
/// nothing. What a scan finds is the script's too, in turn.
#[derive(Default)]
struct Scripted {
    project: Option<Project>,
    fails: Option<AppError>,
    /// Whether each poll, in turn, finds the store changed.
    polls: VecDeque<Result<bool, AppError>>,
    /// What each scan, in turn, finds.
    scans: VecDeque<Result<Vec<Found>, AppError>>,
    /// Where the store's streams end when the watch begins.
    heads: Positions,
    /// Whether the first poll finds no store.
    no_store: bool,
    requests: Vec<WatchRequest>,
    polled: u32,
    headed: u32,
    /// The positions each scan was handed.
    scanned: Vec<Positions>,
}

impl Scripted {
    fn of(polls: Vec<bool>) -> Scripted {
        Scripted {
            project: Some(project()),
            polls: polls.into_iter().map(Ok).collect(),
            ..Scripted::default()
        }
    }

    fn scanning(mut self, scans: Vec<Vec<Found>>) -> Scripted {
        self.scans = scans.into_iter().map(Ok).collect();
        self
    }
}

impl WatchService for Scripted {
    async fn watch(&mut self, request: WatchRequest) -> Result<Option<Watch>, AppError> {
        self.requests.push(request);
        if let Some(error) = self.fails.clone() {
            return Err(error);
        }
        let Some(project) = self.project.clone() else {
            return Ok(None);
        };
        if self.no_store {
            return Ok(Some(Watch::without_store(project)));
        }
        Ok(Some(Watch::of(project)))
    }

    async fn poll(&mut self, _: &mut Watch) -> Result<bool, AppError> {
        self.polled += 1;
        self.polls.pop_front().unwrap_or(Ok(false))
    }

    async fn heads(&mut self, _: &Watch) -> Result<Positions, AppError> {
        self.headed += 1;
        Ok(self.heads.clone())
    }

    async fn after(&mut self, _: &Watch, positions: &Positions) -> Result<Vec<Found>, AppError> {
        self.scanned.push(positions.clone());
        self.scans.pop_front().unwrap_or(Ok(Vec::new()))
    }
}

/// `n` ticks, each ready at once; after the last, the stop is asked for.
struct Steps {
    left: u32,
    stop: Rc<Cell<bool>>,
}

impl Ticks for Steps {
    async fn tick(&mut self) {
        assert!(self.left > 0, "the stream ticks only until stopped");
        self.left -= 1;
        if self.left == 0 {
            self.stop.set(true);
        }
    }
}

/// `n` ticks and the stop after them.
fn steps(n: u32) -> (Steps, impl Future<Output = ()>) {
    let stop = Rc::new(Cell::new(n == 0));
    let asked = Rc::clone(&stop);
    let future = poll_fn(move |_| {
        if asked.get() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    });
    (Steps { left: n, stop }, future)
}

fn settings(format: Format) -> Settings {
    Settings {
        format,
        project: None,
        since: None,
        heartbeat: None,
        prog: "taskloom",
    }
}

fn catalog() -> Catalog {
    let mut sources = vec![crate::EN_US];
    sources.extend(tasks_application::CATALOGS);
    Catalog::en_us(&sources).expect("the catalog loads")
}

/// What a run wrote: its exit code, stdout's lines, stderr.
struct Streamed {
    code: u8,
    lines: Vec<String>,
    err: String,
}

impl Streamed {
    /// The lines as JSON documents.
    fn json(&self) -> Vec<Value> {
        self.lines
            .iter()
            .map(|l| serde_json::from_str(l).expect("one JSON document a line"))
            .collect()
    }
}

fn stream(service: &mut Scripted, settings: &Settings, ticks: u32) -> Streamed {
    let (mut ticks, stop) = steps(ticks);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = ready(run(
        service,
        settings,
        &mut ticks,
        stop,
        &catalog(),
        &mut out,
        &mut err,
    ));
    Streamed {
        code,
        lines: String::from_utf8(out)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect(),
        err: String::from_utf8(err).unwrap(),
    }
}

fn json_settings() -> Settings {
    settings(Format::Json)
}

/// The cursor of `positions` for the test project.
fn cursor(positions: &[(Uuid, u64)]) -> String {
    encode(
        "jolly-mute-magma",
        &positions.iter().map(|(r, n)| (stream_of(*r), *n)).collect(),
    )
}

// ------------------------------------------------------------ the priming line

#[test]
fn test_the_priming_line_is_written_at_once_with_the_heads_as_the_cursor() {
    let mut service = Scripted::of(vec![]);
    service.heads = [(stream_of(A), 3), (stream_of(B), 1)].into();
    let streamed = stream(&mut service, &json_settings(), 0);
    assert_eq!(streamed.code, 0, "{}", streamed.err);
    assert_eq!(
        streamed.lines,
        [format!(
            r#"{{"v":1,"event":"primed","kind":"tasks","project":"jolly-mute-magma","cursor":"{}"}}"#,
            cursor(&[(A, 3), (B, 1)])
        )]
    );
    assert_eq!(service.polled, 0, "stopped before any tick");
    assert!(service.scanned.is_empty(), "no replay without a cursor");
}

#[test]
fn test_the_priming_line_in_text_is_one_line() {
    let mut service = Scripted::of(vec![]);
    let streamed = stream(&mut service, &settings(Format::Text), 0);
    assert_eq!(
        streamed.lines,
        [format!("watching jolly-mute-magma from {}", cursor(&[]))]
    );
}

#[test]
fn test_ticks_that_find_nothing_write_nothing_and_poll_each_time() {
    let mut service = Scripted::of(vec![]);
    let streamed = stream(&mut service, &json_settings(), 5);
    assert_eq!(streamed.code, 0);
    assert_eq!(streamed.lines.len(), 1, "only the priming line");
    assert_eq!(service.polled, 5, "a poll each tick");
    assert!(service.scanned.is_empty(), "no scan where the token held");
}

// ------------------------------------------------------------ the event lines

#[test]
fn test_each_event_is_a_line_with_its_stream_sequence_task_and_the_cursor_after_it() {
    let mut service = Scripted::of(vec![true]).scanning(vec![vec![
        event(A, 0, "swift-amber", "Alpha", &[]),
        event(A, 1, "swift-amber", "Beta", &["area:cli"]),
        event(B, 0, "quiet-heron", "Gamma", &[]),
    ]]);
    let streamed = stream(&mut service, &json_settings(), 1);
    assert_eq!(streamed.code, 0, "{}", streamed.err);
    let lines = streamed.json();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_eq!(
        streamed.lines[1],
        format!(
            r#"{{"v":1,"event":"changed","project":"jolly-mute-magma","stream":"{}","sequence":0,"harp_id":"swift-amber","task":{{"text":"Alpha","status":"To Do","tags":[],"removed":false}},"cursor":"{}"}}"#,
            name(&stream_of(A)),
            cursor(&[(A, 1)])
        ),
        "field order is fixed"
    );
    assert_eq!(lines[2]["task"]["tags"], json!(["area:cli"]));
    assert_eq!(lines[2]["sequence"], 1);
    // Each cursor covers every line before it on the wire.
    assert_eq!(lines[2]["cursor"], cursor(&[(A, 2)]));
    assert_eq!(lines[3]["cursor"], cursor(&[(A, 2), (B, 1)]));
    assert_eq!(
        service.scanned,
        [Positions::new()],
        "the scan began at the heads"
    );
}

#[test]
fn test_a_scan_is_handed_the_positions_the_lines_so_far_left() {
    let mut service = Scripted::of(vec![true, true]).scanning(vec![
        vec![event(A, 0, "swift-amber", "Alpha", &[])],
        vec![event(A, 1, "swift-amber", "Beta", &[])],
    ]);
    stream(&mut service, &json_settings(), 2);
    assert_eq!(
        service.scanned,
        [Positions::new(), [(stream_of(A), 1)].into(),]
    );
}

#[test]
fn test_an_event_line_in_text_names_the_task_status_text_and_position() {
    let mut service = Scripted::of(vec![true]).scanning(vec![vec![Found::Event {
        stream: stream_of(A),
        sequence: 4,
        task: Seen {
            harp_id: "swift-amber".to_string(),
            text: "First line\nsecond line".to_string(),
            status: "Done".to_string(),
            tags: Vec::new(),
            removed: true,
        },
    }]]);
    let streamed = stream(&mut service, &settings(Format::Text), 1);
    assert_eq!(
        streamed.lines.len(),
        2,
        "one line for the event, however long its text"
    );
    assert_eq!(
        streamed.lines[1],
        format!(
            "changed swift-amber [Done] removed First line ({} #4)",
            name(&stream_of(A))
        )
    );
}

#[test]
fn test_an_expired_stream_is_a_line_and_leaves_the_cursor() {
    let mut service = Scripted::of(vec![true]).scanning(vec![vec![
        Found::Expired {
            stream: stream_of(A),
        },
        event(A, 0, "swift-amber", "Again", &[]),
    ]]);
    service.heads = [(stream_of(A), 5), (stream_of(B), 2)].into();
    let streamed = stream(&mut service, &json_settings(), 1);
    let lines = streamed.json();
    assert_eq!(
        streamed.lines[1],
        format!(
            r#"{{"v":1,"event":"expired","project":"jolly-mute-magma","stream":"{}","cursor":"{}"}}"#,
            name(&stream_of(A)),
            cursor(&[(B, 2)])
        )
    );
    // Started again from 0: the cursor holds it afresh.
    assert_eq!(lines[2]["cursor"], cursor(&[(A, 1), (B, 2)]));

    let mut service = Scripted::of(vec![true]).scanning(vec![vec![Found::Expired {
        stream: stream_of(A),
    }]]);
    let streamed = stream(&mut service, &settings(Format::Text), 1);
    assert_eq!(
        streamed.lines[1],
        format!("expired {}", name(&stream_of(A)))
    );
}

// ------------------------------------------------------------ resuming

#[test]
fn test_since_primes_with_the_cursor_asked_for_and_replays_after_it_at_once() {
    let asked = cursor(&[(A, 1)]);
    let mut service =
        Scripted::of(vec![]).scanning(vec![vec![event(A, 1, "swift-amber", "Missed", &[])]]);
    let resumed = Settings {
        since: Some(asked.clone()),
        ..json_settings()
    };
    let streamed = stream(&mut service, &resumed, 0);
    assert_eq!(streamed.code, 0, "{}", streamed.err);
    let lines = streamed.json();
    assert_eq!(lines[0]["event"], "primed");
    assert_eq!(lines[0]["cursor"], asked.as_str(), "the one asked for");
    assert_eq!(lines[1]["event"], "changed");
    assert_eq!(lines[1]["sequence"], 1);
    assert_eq!(service.scanned, [[(stream_of(A), 1)].into()]);
    assert_eq!(service.headed, 0, "the heads are not read: the cursor is");
    assert_eq!(service.polled, 0);
}

#[test]
fn test_since_is_kept_verbatim_even_where_another_spelling_decodes_the_same() {
    let canonical = cursor(&[(A, 1)]);
    let padded = format!("{canonical}{}", "=".repeat((4 - canonical.len() % 4) % 4));
    let resumed = Settings {
        since: Some(padded.clone()),
        ..json_settings()
    };
    let streamed = stream(&mut Scripted::of(vec![]), &resumed, 0);
    assert_eq!(streamed.json()[0]["cursor"], padded.as_str());
}

// ------------------------------------------------------------ refusals

#[test]
fn test_a_cursor_invalid_on_its_own_content_is_exit_2_before_the_store_is_asked() {
    for bad in ["", "not a cursor"] {
        let mut service = Scripted::of(vec![]);
        let resumed = Settings {
            since: Some(bad.to_string()),
            ..json_settings()
        };
        let streamed = stream(&mut service, &resumed, 3);
        assert_eq!(streamed.code, 2, "{bad:?}: {}", streamed.err);
        assert!(streamed.lines.is_empty(), "{bad:?}: no priming line");
        assert_eq!(service.requests, [], "{bad:?}: no state was read");
    }
}

#[test]
fn test_an_unknown_version_names_the_supported_one() {
    let forged = encode("jolly-mute-magma", &Positions::new());
    let forged = {
        let bytes = base64_decode(&forged);
        let text = String::from_utf8(bytes)
            .unwrap()
            .replace("\"v\":1", "\"v\":999");
        base64_encode(text.as_bytes())
    };
    let resumed = Settings {
        since: Some(forged),
        ..json_settings()
    };
    let streamed = stream(&mut Scripted::of(vec![]), &resumed, 0);
    assert_eq!(streamed.code, 2);
    let envelope: Value = serde_json::from_str(&streamed.err).unwrap();
    assert_eq!(envelope["code"], code!("cursor-version"));
    assert_eq!(envelope["args"]["found"], 999);
    assert_eq!(envelope["args"]["supported"], 1);
}

fn base64_decode(text: &str) -> Vec<u8> {
    use base64::Engine;
    super::cursor::BASE64.decode(text).unwrap()
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    super::cursor::BASE64.encode(bytes)
}

#[test]
fn test_another_projects_cursor_is_exit_2_before_any_stream_is_read() {
    let mut service = Scripted::of(vec![true]);
    let resumed = Settings {
        since: Some(encode("swift-amber-falcon", &Positions::new())),
        ..json_settings()
    };
    let streamed = stream(&mut service, &resumed, 3);
    assert_eq!(streamed.code, 2, "{}", streamed.err);
    assert!(streamed.lines.is_empty());
    assert_eq!(
        (service.headed, service.scanned.len(), service.polled),
        (0, 0, 0)
    );
    let envelope: Value = serde_json::from_str(&streamed.err).unwrap();
    assert_eq!(envelope["code"], code!("cursor-foreign"));
}

#[test]
fn test_a_project_with_no_store_is_exit_1_with_no_priming_line() {
    let mut service = Scripted::of(vec![]);
    service.no_store = true;
    let streamed = stream(&mut service, &json_settings(), 3);
    assert_eq!(streamed.code, 1);
    assert!(streamed.lines.is_empty());
    let envelope: Value = serde_json::from_str(&streamed.err).unwrap();
    assert_eq!(envelope["code"], code!("unknown-project"));
    assert_eq!(envelope["args"]["project"], "jolly-mute-magma");

    // A cursor of its own does not change that (it is another's only once
    // the project is known).
    let mut service = Scripted::of(vec![]);
    service.no_store = true;
    let resumed = Settings {
        since: Some(encode("swift-amber-falcon", &Positions::new())),
        ..json_settings()
    };
    assert_eq!(stream(&mut service, &resumed, 3).code, 1);
}

#[test]
fn test_a_watch_where_there_is_no_project_says_so_and_writes_no_line() {
    let mut service = Scripted::default();
    let streamed = stream(&mut service, &settings(Format::Text), 3);
    assert_eq!(streamed.code, 1);
    assert!(streamed.lines.is_empty());
    assert_eq!(
        streamed.err,
        "taskloom: no project here yet; add a task first (taskloom tasks add \"<task text>\")\n"
    );
    assert_eq!(service.polled, 0);

    let streamed = stream(&mut Scripted::default(), &json_settings(), 3);
    let envelope: Value = serde_json::from_str(&streamed.err).expect("an envelope");
    assert_eq!(envelope["code"], code!("no-project"));
}

#[test]
fn test_a_pinned_project_is_the_one_watched() {
    let mut service = Scripted::of(vec![]);
    let pinned = Settings {
        project: Some("jolly-mute-magma".to_string()),
        ..settings(Format::Text)
    };
    stream(&mut service, &pinned, 0);
    assert_eq!(
        service.requests,
        [WatchRequest {
            project: Some("jolly-mute-magma".to_string())
        }]
    );
}

#[test]
fn test_a_watch_a_poll_or_a_scan_that_fails_ends_the_stream_with_why() {
    let why = AppError::Failed(Report::of(&AppError::NoProject));
    let mut service = Scripted {
        fails: Some(why.clone()),
        ..Scripted::of(vec![])
    };
    let streamed = stream(&mut service, &json_settings(), 3);
    assert_eq!(streamed.code, 1);
    assert!(streamed.lines.is_empty());
    let envelope: Value = serde_json::from_str(&streamed.err).unwrap();
    assert_eq!(envelope["code"], AppError::NoProject.code());

    let mut service = Scripted::of(vec![false]);
    service.polls.push_back(Err(why.clone()));
    let streamed = stream(&mut service, &settings(Format::Text), 3);
    assert_eq!(streamed.code, 1);
    assert_eq!(streamed.lines.len(), 1, "what was written stays written");
    assert!(streamed.err.starts_with("taskloom: "), "{}", streamed.err);

    let mut service = Scripted::of(vec![true]);
    service.scans.push_back(Err(why.clone()));
    let streamed = stream(&mut service, &json_settings(), 3);
    assert_eq!(streamed.code, 1);
    assert_eq!(streamed.lines.len(), 1);

    // The replay of a resume fails the same way.
    let mut service = Scripted::of(vec![]);
    service.scans.push_back(Err(why));
    let resumed = Settings {
        since: Some(cursor(&[])),
        ..json_settings()
    };
    assert_eq!(stream(&mut service, &resumed, 3).code, 1);
}

// ------------------------------------------------------------ the heartbeat

#[test]
fn test_a_heartbeat_comes_after_that_many_quiet_ticks_with_the_current_cursor() {
    let mut service = Scripted::of(vec![]);
    service.heads = [(stream_of(A), 3)].into();
    let beating = Settings {
        heartbeat: Some(3),
        ..json_settings()
    };
    let streamed = stream(&mut service, &beating, 7);
    let lines = streamed.json();
    // Priming, then a beat at the third and sixth quiet tick.
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(
        streamed.lines[1],
        format!(
            r#"{{"v":1,"event":"heartbeat","project":"jolly-mute-magma","cursor":"{}"}}"#,
            cursor(&[(A, 3)])
        )
    );
    assert_eq!(lines[2]["event"], "heartbeat");
}

#[test]
fn test_a_line_written_resets_the_quiet_a_heartbeat_waits_for() {
    // Ticks: quiet, a change (a line), quiet, quiet: the beat is two
    // quiet ticks after the line, not after the first tick.
    let mut service = Scripted::of(vec![false, true, false, false]).scanning(vec![vec![event(
        A,
        0,
        "swift-amber",
        "Alpha",
        &[],
    )]]);
    let beating = Settings {
        heartbeat: Some(2),
        ..json_settings()
    };
    let streamed = stream(&mut service, &beating, 4);
    let events: Vec<String> = streamed
        .json()
        .iter()
        .map(|l| l["event"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(events, ["primed", "changed", "heartbeat"]);
}

#[test]
fn test_a_change_that_wrote_no_line_is_quiet_and_no_heartbeat_means_none() {
    // The token moved, nothing on a task stream did: a quiet tick.
    let mut service = Scripted::of(vec![true, true]);
    let beating = Settings {
        heartbeat: Some(2),
        ..json_settings()
    };
    assert_eq!(stream(&mut service, &beating, 2).lines.len(), 2);

    let mut service = Scripted::of(vec![]);
    assert_eq!(stream(&mut service, &json_settings(), 50).lines.len(), 1);
}

#[test]
fn test_a_heartbeat_in_text_is_one_line_naming_the_project() {
    let mut service = Scripted::of(vec![]);
    let beating = Settings {
        heartbeat: Some(1),
        ..settings(Format::Text)
    };
    let streamed = stream(&mut service, &beating, 1);
    assert_eq!(streamed.lines[1], "heartbeat jolly-mute-magma");
}

#[test]
fn test_heartbeat_seconds_are_polls_and_zero_is_none() {
    assert_eq!(POLL, Duration::from_millis(100));
    assert_eq!(heartbeat_ticks(0), None);
    assert_eq!(heartbeat_ticks(1), Some(10));
    assert_eq!(heartbeat_ticks(15), Some(150));
    assert_eq!(heartbeat_ticks(u64::MAX), Some(u32::MAX));
}

#[test]
fn test_the_flags_default_to_a_heartbeat_of_fifteen_seconds_and_no_cursor() {
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        group: Group,
    }
    let Group::Watch(args) = Cli::parse_from(["x", "watch"]).group;
    assert_eq!(
        args,
        Args {
            since: None,
            heartbeat: 15
        }
    );
    let Group::Watch(args) =
        Cli::parse_from(["x", "watch", "--since", "abc", "--heartbeat", "0"]).group;
    assert_eq!(
        args,
        Args {
            since: Some("abc".to_string()),
            heartbeat: 0
        }
    );
    assert!(Cli::try_parse_from(["x", "watch", "--heartbeat", "soon"]).is_err());
}

/// A reader that went away.
struct Gone;

impl std::io::Write for Gone {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn test_a_stream_no_one_reads_ends_with_why() {
    let mut service = Scripted::of(vec![]);
    let (mut ticks, stop) = steps(3);
    let mut err = Vec::new();
    let code = ready(run(
        &mut service,
        &settings(Format::Json),
        &mut ticks,
        stop,
        &catalog(),
        &mut Gone,
        &mut err,
    ));
    assert_eq!(code, 1);
    let envelope: Value = serde_json::from_str(&String::from_utf8(err).unwrap()).unwrap();
    assert_eq!(envelope["code"], code!("output"));
    assert_eq!(service.polled, 0);
}

#[test]
fn test_an_interval_ticks_no_sooner_than_its_period() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let period = Duration::from_millis(20);
    let started = Instant::now();
    runtime.block_on(async {
        let mut interval = Interval::every(period);
        interval.tick().await;
        interval.tick().await;
    });
    assert!(started.elapsed() >= 2 * period, "{:?}", started.elapsed());
}

// ------------------------------------------------------------ help

#[test]
fn test_the_long_help_names_unixs_stops_on_unix() {
    assert_eq!(long_about(os::Flavor::Unix), reviewed::ON_UNIX);
}

/// A departure from Go's words (reviewed, 2026-10-28): on Windows the
/// help names what stops the watch there, not SIGTERM.
#[test]
fn test_the_long_help_names_windows_stops_on_windows() {
    let said = reviewed::said(os::Flavor::Windows);
    assert!(
        said.contains(
            "(Ctrl-C or Ctrl-Break, or the console's\nclose, the user's logoff or the system's shutdown)"
        ),
        "{said}"
    );
    assert_eq!(long_about(os::Flavor::Windows), said);
}

/// The binary's `watch --help` says this host's long help.
#[test]
fn test_the_mounted_command_says_the_hosts_long_help() {
    use clap::CommandFactory;

    #[derive(clap::Parser)]
    struct Cli {
        #[command(subcommand)]
        group: Group,
    }
    let mut cli = Cli::command();
    let watch = cli.find_subcommand_mut("watch").expect("watch is mounted");
    // The help opens with one line (the CLI grammar's rule), so the
    // reviewed words follow the usage, after the example.
    assert_eq!(
        watch.get_after_help().map(ToString::to_string),
        Some(after_help(os::host().flavor()))
    );
    assert!(after_help(os::host().flavor()).ends_with(&reviewed::said(os::host().flavor())));
}
