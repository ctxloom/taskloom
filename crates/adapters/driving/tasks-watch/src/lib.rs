//! `taskloom tasks watch`: a project's task events as a stream, one line
//! per event, resumable. In JSON (the machine format, JSON Lines) the
//! lines are, each with `v`, the version of the line and cursor format:
//!
//! - `{"v":1,"event":"primed","kind":"tasks","project":P,"cursor":C}`,
//!   first, before any change: the cursor is the store's position now, or
//!   the one asked for with `--since`;
//! - `{"v":1,"event":"changed","project":P,"stream":S,"sequence":N,
//!   "harp_id":H,"task":{"text","status","tags","removed"},"cursor":C}`
//!   for each event of a task stream, the task as that event leaves it;
//! - `{"v":1,"event":"expired","project":P,"stream":S,"cursor":C}` where a
//!   stream the cursor had was pruned (or minted again, shorter): it is
//!   reported once and read from its start;
//! - `{"v":1,"event":"heartbeat","project":P,"cursor":C}` after
//!   `--heartbeat` quiet seconds (default 15, 0: none).
//!
//! The field names are a PROPOSAL pending owner review. A cursor on a line
//! covers every line before it on the wire (see [`cursor`]); no order
//! across streams is promised, only within one. Text is one line per
//! event, for a person.
//!
//! Changes come from the store, not the filesystem: the watch use case is
//! polled on every tick of a [`Ticks`] (every [`POLL`] in the binary), and
//! a tick that finds the store's change token moved reads the task streams
//! from the cursor. Any commit moves the token, including ones no task
//! stream sees; they write nothing. With `--since` the lines after the
//! cursor are replayed at once, after the priming line. The stream runs
//! until its stop future resolves (the binary's: SIGINT or SIGTERM, or on
//! Windows Ctrl-C, Ctrl-Break or the console's close, logoff or shutdown,
//! as its long help says; never its stdin closing, since a subscriber
//! such as the VS Code extension starts it with stdin from /dev/null), or
//! the watch fails. A failure is reported as every command's is: exit 2
//! where the cursor is invalid on its own content, 1 where the watch ran
//! against state and failed (no such project).

mod cursor;
mod error;

use std::future::{poll_fn, Future};
use std::io::{self, Write};
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use angzarr_presentation::{Catalog, Format};
use clap::Subcommand;
use serde::Serialize;
use tasks_application::{Found, Positions, WatchRequest, WatchService};

pub use cursor::VERSION;
use cursor::{encode, name, Cursor};

#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{WatchError, EN_US};

/// How often the binary polls the store's change token.
pub const POLL: Duration = Duration::from_millis(100);

/// `--heartbeat`'s default, in seconds.
pub const HEARTBEAT_SECONDS: u64 = 15;

/// The polls a quiet `seconds` is: none for 0 (no heartbeat).
pub fn heartbeat_ticks(seconds: u64) -> Option<u32> {
    let ticks = seconds.saturating_mul(1000) / u64::try_from(POLL.as_millis()).unwrap_or(1);
    match ticks {
        0 => None,
        ticks => Some(u32::try_from(ticks).unwrap_or(u32::MAX)),
    }
}

/// `taskloom tasks watch --help`'s long text as a host of `flavor` says it: what
/// stops the watch is what the os crate's stop signal hears there, Ctrl-C
/// and SIGTERM on unix (Go's words); on Windows (a reviewed departure from
/// Go's words, 2026-10-28) Ctrl-C and Ctrl-Break, and the console's close,
/// the user's logoff and the system's shutdown.
pub fn long_about(flavor: os::Flavor) -> String {
    let stops = match flavor {
        os::Flavor::Unix => "(Ctrl-C / SIGTERM)",
        os::Flavor::Windows => {
            "(Ctrl-C or Ctrl-Break, or the console's\n\
             close, the user's logoff or the system's shutdown)"
        }
    };
    format!(
        "Watch the project's task store and write one line per task event, resumable.\n\n\
Long-lived: runs until interrupted {stops}; a closed stdin does not\n\
stop it. A priming line at once, then one line per event with its stream,\n\
sequence and the task as it is then, each carrying a cursor, e.g.\n\
{{\"v\":1,\"event\":\"changed\",\"project\":\"swift-amber-falcon\",...,\"cursor\":\"eyJ2...\"}}.\n\
Pass a line's cursor as --since to resume after it: the lines you missed are\n\
replayed, none twice. Cursors are per stream, so no order across tasks is\n\
promised. A stream pruned since is reported once as expired. A heartbeat\n\
line comes after each --heartbeat quiet seconds (default 15, 0 for none).\n\
Exit 2 for a cursor that is malformed, of an unknown version or another\n\
project's; 1 for a project with no store."
    )
}

/// The text after `watch --help`'s usage: an example, then the long help
/// (the reviewed words) as a host of `flavor` says it.
pub fn after_help(flavor: os::Flavor) -> String {
    format!(
        "Examples:\n  taskloom tasks watch --format json\n  taskloom tasks watch --since <cursor> --heartbeat 5\n\n{}",
        long_about(flavor)
    )
}

/// The command, for the binary to mount as `tasks watch`.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Group {
    /// Stream task events as lines, resumable with --since <cursor>
    #[command(after_help = after_help(os::host().flavor()))]
    Watch(Args),
}

/// `tasks watch`'s own flags.
#[derive(clap::Args, Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// Resume after this cursor, a `cursor` of an earlier line
    #[arg(long, value_name = "CURSOR")]
    pub since: Option<String>,
    /// Write a heartbeat line after this many quiet seconds (0: none)
    #[arg(long, value_name = "SECONDS", default_value_t = HEARTBEAT_SECONDS)]
    pub heartbeat: u64,
}

/// What the stream is run with.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Settings {
    /// How an error is reported, and whether the lines are JSON or text.
    pub format: Format,
    /// `--project`: the project to watch, overriding resolution.
    pub project: Option<String>,
    /// `--since`, as given.
    pub since: Option<String>,
    /// The polls of quiet before a heartbeat; none for no heartbeat
    /// ([`heartbeat_ticks`]).
    pub heartbeat: Option<u32>,
    /// The program's name: every diagnostic's prefix.
    pub prog: &'static str,
}

/// When to poll: the stream polls once each tick.
pub trait Ticks {
    fn tick(&mut self) -> impl Future<Output = ()>;
}

/// Ticks every `period`, the first a period after it starts; a tick late
/// (a slow poll) delays the ones after it rather than bunching them.
pub struct Interval(tokio::time::Interval);

impl Interval {
    /// Ticks every `period`. Needs a tokio runtime with its timer.
    pub fn every(period: Duration) -> Interval {
        let start = tokio::time::Instant::now() + period;
        let mut interval = tokio::time::interval_at(start, period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Interval(interval)
    }
}

impl Ticks for Interval {
    async fn tick(&mut self) {
        self.0.tick().await;
    }
}

/// One JSON line of the stream.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
struct Line<'a> {
    v: u64,
    event: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    project: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    harp_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task: Option<TaskLine<'a>>,
    cursor: &'a str,
}

/// The task as an event left it.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
struct TaskLine<'a> {
    text: &'a str,
    status: &'a str,
    tags: &'a [String],
    removed: bool,
}

impl<'a> Line<'a> {
    fn new(event: &'static str, project: &'a str, cursor: &'a str) -> Line<'a> {
        Line {
            v: VERSION,
            event,
            kind: None,
            project,
            stream: None,
            sequence: None,
            harp_id: None,
            task: None,
            cursor,
        }
    }
}

/// The stream's lines, in one format, to one writer.
struct Lines<'a> {
    format: Format,
    project: &'a str,
    out: &'a mut dyn Write,
}

impl Lines<'_> {
    fn write(&mut self, line: &Line<'_>, text: &str) -> Result<(), WatchError> {
        let written = match self.format {
            Format::Json => angzarr_presentation::write_json_line(self.out, line),
            Format::Text => writeln!(self.out, "{text}").and_then(|()| self.out.flush()),
        };
        written.map_err(|e| WatchError::Output {
            detail: e.to_string(),
        })
    }

    fn primed(&mut self, cursor: &str) -> Result<(), WatchError> {
        let mut line = Line::new("primed", self.project, cursor);
        line.kind = Some("tasks");
        self.write(&line, &format!("watching {} from {cursor}", self.project))
    }

    fn heartbeat(&mut self, cursor: &str) -> Result<(), WatchError> {
        let line = Line::new("heartbeat", self.project, cursor);
        self.write(&line, &format!("heartbeat {}", self.project))
    }

    /// `found`, one line each; `positions` moves on with each, so a line's
    /// cursor covers every line before it. How many lines were written.
    fn found(&mut self, found: Vec<Found>, positions: &mut Positions) -> Result<usize, WatchError> {
        let lines = found.len();
        for one in found {
            match one {
                Found::Expired { stream } => {
                    positions.remove(&stream);
                    let (cursor, stream) = (encode(self.project, positions), name(&stream));
                    let mut line = Line::new("expired", self.project, &cursor);
                    line.stream = Some(&stream);
                    self.write(&line, &format!("expired {stream}"))?;
                }
                Found::Event {
                    stream,
                    sequence,
                    task,
                } => {
                    positions.insert(stream.clone(), sequence + 1);
                    let (cursor, stream) = (encode(self.project, positions), name(&stream));
                    let mut line = Line::new("changed", self.project, &cursor);
                    line.stream = Some(&stream);
                    line.sequence = Some(sequence);
                    line.harp_id = Some(&task.harp_id);
                    line.task = Some(TaskLine {
                        text: &task.text,
                        status: &task.status,
                        tags: &task.tags,
                        removed: task.removed,
                    });
                    let removed = if task.removed { " removed" } else { "" };
                    let text = format!(
                        "changed {} [{}]{removed} {} ({stream} #{sequence})",
                        task.harp_id,
                        task.status,
                        angzarr_presentation::text::headline(&task.text, 80),
                    );
                    self.write(&line, &text)?;
                }
            }
        }
        Ok(lines)
    }
}

/// Stream `service`'s events to `out`, polling on each of `ticks`, until
/// `stop` resolves (exit 0) or the watch fails (the error on `err`,
/// rendered from `catalog`; exit 2 for an invalid cursor, else 1).
pub async fn run<W: WatchService>(
    service: &mut W,
    settings: &Settings,
    ticks: &mut impl Ticks,
    stop: impl Future<Output = ()>,
    catalog: &Catalog,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    match stream(service, settings, ticks, stop, out).await {
        Ok(()) => 0,
        Err(error) => {
            let _ = report(&error, settings, catalog, err);
            error.exit_code()
        }
    }
}

/// What woke the stream.
enum Woken {
    Stop,
    Tick,
}

async fn stream<W: WatchService>(
    service: &mut W,
    settings: &Settings,
    ticks: &mut impl Ticks,
    stop: impl Future<Output = ()>,
    out: &mut dyn Write,
) -> Result<(), WatchError> {
    // Invalid on its own content is refused before any state is read.
    let resume = settings.since.as_deref().map(Cursor::parse).transpose()?;
    let request = WatchRequest {
        project: settings.project.clone(),
    };
    let mut watch = service
        .watch(request)
        .await
        .map_err(WatchError::App)?
        .ok_or(WatchError::NoProject)?;
    let project = watch.project.id.clone();
    if watch.lacks_store() {
        return Err(WatchError::UnknownProject { project });
    }
    // The cursor is the project's: refused before any stream is read.
    if let Some(cursor) = &resume {
        cursor.check_project(&project)?;
    }
    let mut positions = match &resume {
        Some(cursor) => cursor.positions.clone(),
        None => service.heads(&watch).await.map_err(WatchError::App)?,
    };
    let mut lines = Lines {
        format: settings.format,
        project: &project,
        out,
    };
    match (&settings.since, &resume) {
        (Some(asked), Some(_)) => lines.primed(asked)?,
        _ => lines.primed(&encode(&project, &positions))?,
    }
    if resume.is_some() {
        let found = service
            .after(&watch, &positions)
            .await
            .map_err(WatchError::App)?;
        lines.found(found, &mut positions)?;
    }
    let mut stop = pin!(stop);
    let mut quiet = 0u32;
    loop {
        let woken = {
            let mut tick = pin!(ticks.tick());
            poll_fn(|cx| {
                if stop.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(Woken::Stop);
                }
                tick.as_mut().poll(cx).map(|()| Woken::Tick)
            })
            .await
        };
        if let Woken::Stop = woken {
            return Ok(());
        }
        let mut wrote = false;
        if service.poll(&mut watch).await.map_err(WatchError::App)? {
            let found = service
                .after(&watch, &positions)
                .await
                .map_err(WatchError::App)?;
            wrote = lines.found(found, &mut positions)? > 0;
        }
        quiet = if wrote { 0 } else { quiet + 1 };
        if settings.heartbeat.is_some_and(|every| quiet >= every) {
            lines.heartbeat(&encode(&project, &positions))?;
            quiet = 0;
        }
    }
}

/// `error` on stderr: its envelope in JSON; in text its catalog message
/// after the program's name.
fn report(
    error: &WatchError,
    settings: &Settings,
    catalog: &Catalog,
    err: &mut dyn Write,
) -> io::Result<()> {
    match settings.format {
        Format::Json => angzarr_presentation::write_json(err, &catalog.envelope(error)),
        Format::Text => writeln!(err, "{}: {}", settings.prog, catalog.message(error)),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
