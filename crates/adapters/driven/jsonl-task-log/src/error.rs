//! Why a task log could not be read: a log is refused whole, never one
//! line.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LogError {
    /// No home to find the logs in: the variable `flavor` reads it from
    /// (HOME, or USERPROFILE on Windows) is not set.
    NoHome { flavor: os::Flavor },
    /// The log, or the directory of logs, could not be read.
    Unreadable { path: String, detail: String },
    /// A line is not a JSON record the log's event decodes from.
    Malformed {
        path: String,
        line: u32,
        detail: String,
    },
    /// A record without an op.
    NoOp { path: String, line: u32 },
    /// A record whose op the log's format does not have.
    UnknownOp { path: String, line: u32, op: String },
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for LogError {
    fn code(&self) -> &str {
        match self {
            LogError::NoHome { .. } => code!("no-home"),
            LogError::Unreadable { .. } => code!("unreadable"),
            LogError::Malformed { .. } => code!("malformed"),
            LogError::NoOp { .. } => code!("no-op"),
            LogError::UnknownOp { .. } => code!("unknown-op"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            LogError::NoHome { flavor } => vec![("var", flavor.home_var().into())],
            LogError::Unreadable { path, detail } => vec![
                ("path", path.as_str().into()),
                ("ext_err_txt", detail.as_str().into()),
            ],
            LogError::Malformed { path, line, detail } => vec![
                ("path", path.as_str().into()),
                ("line", (*line).into()),
                ("ext_err_txt", detail.as_str().into()),
            ],
            LogError::NoOp { path, line } => {
                vec![("path", path.as_str().into()), ("line", (*line).into())]
            }
            LogError::UnknownOp { path, line, op } => vec![
                ("path", path.as_str().into()),
                ("line", (*line).into()),
                ("op", op.as_str().into()),
            ],
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!(concat!(env!("OUT_DIR"), "/en-US.ftl")),
);

/// One value of every error, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let path = || "/home/u/.ctxloom/tasks/jolly-mute-magma.jsonl".to_string();
    vec![
        Box::new(LogError::NoHome {
            flavor: os::Flavor::Unix,
        }),
        Box::new(LogError::Unreadable {
            path: path(),
            detail: "permission denied".to_string(),
        }),
        Box::new(LogError::Malformed {
            path: path(),
            line: 3,
            detail: "expected value at line 1 column 1".to_string(),
        }),
        Box::new(LogError::NoOp {
            path: path(),
            line: 4,
        }),
        Box::new(LogError::UnknownOp {
            path: path(),
            line: 5,
            op: "archive".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
