//! The JSONL task logs, `~/.ctxloom/tasks/<project-id>.jsonl`, behind the
//! [`JsonlTaskLogs`] port: one append-only JSON line per change, an event
//! with an op, its task and its fields. Only read: a log is opened
//! read-only and nothing beside it is touched, not even its lock file.
//!
//! Every read goes through the [`Fs`] it is given.
//!
//! The fold (replay order, duplicate lines and duplicate adds) is the
//! importer's, in the application; this reads the records.

mod error;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use annals::next::Timestamp;
use annals_diagnostic::Report;
use file_system::{Fs, Kind};
use ports::{JsonlOp, JsonlRecord, JsonlTaskLogs};
use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub use error::{diagnostic_samples, LogError, EN_US};

/// The log file's extension.
const EXTENSION: &str = "jsonl";

/// 0001-01-01T00:00:00Z in Unix seconds: the instant a record without a
/// `ts` decodes to.
pub const MISSING_TS_INSTANT: i64 = -62_135_596_800;

/// The logs in one directory, `~/.ctxloom/tasks`, on a filesystem.
#[derive(Clone, Debug)]
pub struct JsonlTaskLogDir {
    fs: Arc<dyn Fs>,
    /// `None` where there is no home to find it in.
    dir: Option<PathBuf>,
}

impl JsonlTaskLogDir {
    /// The logs in `dir` on `fs`.
    pub fn new(fs: Arc<dyn Fs>, dir: impl Into<PathBuf>) -> Self {
        JsonlTaskLogDir {
            fs,
            dir: Some(dir.into()),
        }
    }

    /// The logs under `home` on `fs`, `<home>/.ctxloom/tasks`; with no home
    /// (HOME, or USERPROFILE on Windows, unset or empty), every read fails
    /// saying so.
    pub fn in_home(fs: Arc<dyn Fs>, home: Option<&Path>) -> Self {
        JsonlTaskLogDir {
            fs,
            dir: home
                .filter(|h| !h.as_os_str().is_empty())
                .map(|h| h.join(".ctxloom").join("tasks")),
        }
    }

    fn dir(&self) -> Result<&Path, Report> {
        self.dir.as_deref().ok_or_else(|| {
            Report::of(&LogError::NoHome {
                flavor: self.fs.os().flavor(),
            })
        })
    }
}

impl JsonlTaskLogs for JsonlTaskLogDir {
    fn projects(&mut self) -> Result<Vec<String>, Report> {
        let dir = self.dir()?;
        let unreadable = |e: io::Error| {
            Report::of(&LogError::Unreadable {
                path: dir.display().to_string(),
                detail: e.to_string(),
            })
        };
        let entries = match self.fs.read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(unreadable(e)),
        };
        // A log is a file, or a link to one.
        let mut projects: Vec<String> = entries
            .into_iter()
            .map(|entry| dir.join(entry.name))
            .filter(|path| path.extension().is_some_and(|e| e == EXTENSION))
            .filter(|path| self.fs.metadata(path).is_ok_and(|m| m.kind == Kind::File))
            .filter_map(|path| Some(path.file_stem()?.to_str()?.to_string()))
            .collect();
        // By project id, not file name: `a-b.jsonl` lists before `a.jsonl`.
        projects.sort_unstable();
        Ok(projects)
    }

    fn records(&mut self, project_id: &str) -> Result<Option<Vec<JsonlRecord>>, Report> {
        let path = self.dir()?.join(format!("{project_id}.{EXTENSION}"));
        let bytes = match self.fs.read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(Report::of(&LogError::Unreadable {
                    path: path.display().to_string(),
                    detail: e.to_string(),
                }))
            }
        };
        parse(
            &path.display().to_string(),
            &String::from_utf8_lossy(&bytes),
        )
        .map(Some)
        .map_err(|e| Report::of(&e))
    }
}

/// The records of the log `text`, read from `path`: each line trimmed,
/// blank ones skipped.
pub fn parse(path: &str, text: &str) -> Result<Vec<JsonlRecord>, LogError> {
    let mut records = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        let raw = line.trim();
        if raw.is_empty() {
            continue;
        }
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        records.push(record(path, line, raw)?);
    }
    Ok(records)
}

/// A line as its JSON decodes: a missing or null field is empty, an
/// unknown one is ignored.
#[derive(Deserialize)]
struct Line {
    op: Option<String>,
    task: Option<String>,
    text: Option<String>,
    status: Option<String>,
    trigger: Option<String>,
    session: Option<String>,
    repair_of: Option<String>,
    tags: Option<Vec<String>>,
    ts: Option<String>,
}

fn record(path: &str, line: u32, raw: &str) -> Result<JsonlRecord, LogError> {
    let malformed = |detail: String| LogError::Malformed {
        path: path.to_string(),
        line,
        detail,
    };
    let decoded: Line = serde_json::from_str(raw).map_err(|e| malformed(e.to_string()))?;
    let ts = match decoded.ts.as_deref() {
        None => Timestamp {
            seconds: MISSING_TS_INSTANT,
            nanos: 0,
        },
        Some(ts) => instant(ts).map_err(malformed)?,
    };
    let op = match decoded.op.as_deref().unwrap_or_default() {
        "add" => JsonlOp::Add,
        "status" => JsonlOp::Status,
        "text" => JsonlOp::Text,
        "remove" => JsonlOp::Remove,
        "tag" => JsonlOp::Tag,
        "untag" => JsonlOp::Untag,
        "" => {
            return Err(LogError::NoOp {
                path: path.to_string(),
                line,
            })
        }
        other => {
            return Err(LogError::UnknownOp {
                path: path.to_string(),
                line,
                op: other.to_string(),
            })
        }
    };
    Ok(JsonlRecord {
        line,
        raw: raw.to_string(),
        op,
        task: decoded.task.unwrap_or_default(),
        text: decoded.text.unwrap_or_default(),
        status: decoded.status.unwrap_or_default(),
        trigger: decoded.trigger.unwrap_or_default(),
        session: decoded.session.unwrap_or_default(),
        repair_of: decoded.repair_of.unwrap_or_default(),
        tags: decoded.tags.unwrap_or_default(),
        ts,
    })
}

/// An RFC 3339 instant, as a `ts` is written.
fn instant(text: &str) -> Result<Timestamp, String> {
    let at =
        OffsetDateTime::parse(text, &Rfc3339).map_err(|e| format!("parsing time {text:?}: {e}"))?;
    Ok(Timestamp {
        seconds: at.unix_timestamp(),
        nanos: i32::try_from(at.nanosecond()).unwrap_or(0),
    })
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
