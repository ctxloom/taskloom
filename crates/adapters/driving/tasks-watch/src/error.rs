//! Why a watch stopped other than by being asked to; and this crate's
//! catalog.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};
use tasks_application::AppError;

#[derive(Clone, PartialEq, Debug)]
pub enum WatchError {
    /// The use case failed: its error's diagnostic.
    App(AppError),
    /// The work root has no project yet, so there is nothing to follow;
    /// a watch mints none to get one.
    NoProject,
    /// `--since` is no cursor this build wrote: it does not decode, or
    /// names a stream that is no task stream's. Invalid on its own
    /// content: exit 2, before any state is read.
    CursorMalformed,
    /// A cursor of a version this build does not read, naming the one it
    /// does. Exit 2.
    CursorVersion { found: u64, supported: u64 },
    /// A cursor taken from another project's watch. Exit 2, before any
    /// stream is read.
    CursorForeign {
        cursor_project: String,
        project: String,
    },
    /// The project has no store: it was never written to, or there is no
    /// such project. Exit 1: it was looked for and not found.
    UnknownProject { project: String },
    /// A line could not be written: the reader went away.
    Output { detail: String },
}

impl fmt::Display for WatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for WatchError {}

impl WatchError {
    /// The exit code: 2 where the request is invalid on its own content,
    /// 1 where it ran against state and failed.
    pub fn exit_code(&self) -> u8 {
        match self {
            WatchError::CursorMalformed
            | WatchError::CursorVersion { .. }
            | WatchError::CursorForeign { .. } => 2,
            _ => 1,
        }
    }
}

impl Diagnostic for WatchError {
    fn code(&self) -> &str {
        match self {
            WatchError::App(error) => error.code(),
            WatchError::NoProject => code!("no-project"),
            WatchError::CursorMalformed => code!("cursor-malformed"),
            WatchError::CursorVersion { .. } => code!("cursor-version"),
            WatchError::CursorForeign { .. } => code!("cursor-foreign"),
            WatchError::UnknownProject { .. } => code!("unknown-project"),
            WatchError::Output { .. } => code!("output"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            WatchError::App(error) => error.args(),
            WatchError::NoProject | WatchError::CursorMalformed => Vec::new(),
            WatchError::CursorVersion { found, supported } => {
                vec![
                    ("found", (*found).into()),
                    ("supported", (*supported).into()),
                ]
            }
            WatchError::CursorForeign {
                cursor_project,
                project,
            } => vec![
                ("cursor_project", cursor_project.as_str().into()),
                ("project", project.as_str().into()),
            ],
            WatchError::UnknownProject { project } => vec![("project", project.as_str().into())],
            WatchError::Output { detail } => vec![("ext_err_txt", detail.as_str().into())],
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!(concat!(env!("OUT_DIR"), "/en-US.ftl")),
);

/// One value of every error this crate defines (not the wrapper), for the
/// catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(WatchError::NoProject),
        Box::new(WatchError::CursorMalformed),
        Box::new(WatchError::CursorVersion {
            found: 999,
            supported: 1,
        }),
        Box::new(WatchError::CursorForeign {
            cursor_project: "jolly-mute-magma".to_string(),
            project: "swift-amber-falcon".to_string(),
        }),
        Box::new(WatchError::UnknownProject {
            project: "no-such-project".to_string(),
        }),
        Box::new(WatchError::Output {
            detail: "Broken pipe (os error 32)".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
