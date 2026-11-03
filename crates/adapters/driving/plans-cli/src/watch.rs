//! `taskloom plans watch`: the plan files' changes as a JSON Lines stream.
//! One line at once, `{"event":"changed","kind":"plans"}`, written when
//! the watch is established (the files' state taken first), so a change
//! after it is never missed. Then one line per plan file that changed:
//!
//! ```json
//! {"event":"changed","kind":"plans","id":"quiet-amber-falcon",
//!  "path":"/work/docs/p.plan.md",
//!  "changes":{"status":{"from":"draft","to":"active"},
//!             "tags":{"added":["area:cli"],"removed":[]},"body":true}}
//! ```
//!
//! `event` is `created`, `changed` or `deleted`; `id` is the plan's (the
//! last known, for a deleted file; null where it has none); `changes` has
//! an entry for each frontmatter key that differs: `id`, `title` and
//! `status` as `{from, to}` (null where the key is absent), `tags`,
//! `governs`, `supersedes` and `sessions` as `{added, removed}`, and
//! `"body": true` where the markdown differs. A created file lists every
//! key it has as an addition; a deleted one has no changes. A line never
//! carries the body or any of the file's text: a subscriber reads the file.
//! A file whose frontmatter cannot be read has `"invalid"` too, the reason.
//!
//! The files are polled on every tick of a [`Ticks`], and a change is
//! reported once a tick finds the files as the tick before did, so a file
//! written in several steps is one line. The stream runs until its stop
//! future resolves (the binary's: SIGINT or SIGTERM), or a line cannot be
//! written; both formats are the stream.

use std::future::{poll_fn, Future};
use std::io::Write;
use std::pin::pin;
use std::task::Poll;

use angzarr_presentation::Catalog;
use plans_application::{changes, Change, Delta, Event, Plans, Snapshot};
use serde_json::{json, Map, Value};

use crate::{report, CliError, Settings};

/// When to look at the files again.
pub trait Ticks {
    fn tick(&mut self) -> impl Future<Output = ()>;
}

/// The priming line.
fn priming() -> Value {
    json!({"event": "changed", "kind": "plans"})
}

/// `change` as its line.
fn line(change: &Change, catalog: &Catalog) -> Value {
    let event = match change.event {
        Event::Created => "created",
        Event::Changed => "changed",
        Event::Deleted => "deleted",
    };
    let mut changes = Map::new();
    for delta in &change.deltas {
        match delta {
            Delta::Scalar { key, from, to } => {
                changes.insert((*key).to_string(), json!({"from": from, "to": to}));
            }
            Delta::List {
                key,
                added,
                removed,
            } => {
                changes.insert(
                    (*key).to_string(),
                    json!({"added": added, "removed": removed}),
                );
            }
            Delta::Body => {
                changes.insert("body".to_string(), json!(true));
            }
        }
    }
    let mut line = json!({
        "event": event,
        "kind": "plans",
        "id": change.id,
        "path": change.path,
        "changes": changes,
    });
    if let Some(error) = &change.invalid {
        line["invalid"] = json!(catalog.message(error));
    }
    line
}

/// What woke the stream.
enum Woken {
    Stop,
    Tick,
}

fn emit(out: &mut dyn Write, line: &Value) -> Result<(), CliError> {
    angzarr_presentation::write_json_line(out, line).map_err(|e| CliError::Output {
        detail: e.to_string(),
    })
}

/// Stream the plan files' changes to `out`, looking on each of `ticks`,
/// until `stop` resolves (exit 0) or the watch fails (exit 1, the error on
/// `err`, rendered from `catalog`).
pub async fn run<P: Plans>(
    plans: &mut P,
    settings: &Settings,
    ticks: &mut impl Ticks,
    stop: impl Future<Output = ()>,
    catalog: &Catalog,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    match stream(plans, ticks, stop, catalog, out).await {
        Ok(()) => 0,
        Err(error) => {
            let _ = report(&error, settings, catalog, err);
            1
        }
    }
}

async fn stream<P: Plans>(
    plans: &mut P,
    ticks: &mut impl Ticks,
    stop: impl Future<Output = ()>,
    catalog: &Catalog,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let failed = |error| CliError::WatchFailed { error };
    let mut emitted: Snapshot = plans.snapshot().map_err(failed)?;
    emit(out, &priming())?;
    let mut seen = emitted.clone();
    let mut stop = pin!(stop);
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
        let now = plans.snapshot().map_err(failed)?;
        if now != seen {
            // Still moving: the files are looked at again.
            seen = now;
            continue;
        }
        if seen != emitted {
            for change in changes(&emitted, &seen) {
                emit(out, &line(&change, catalog))?;
            }
            emitted = seen.clone();
        }
    }
}

#[cfg(test)]
#[path = "watch.test.rs"]
mod tests;
