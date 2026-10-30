//! Why a project's store could not be opened, beyond what annals-store-sql
//! reports itself.

use std::fmt;

use annals_diagnostic::{Arg, Diagnostic};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum StoreOpenError {
    /// The directory the database goes in could not be made.
    CreateDir { path: String, detail: String },
    /// The filesystem could not say where the database is on disk, for
    /// SQLite to open.
    Unplaced { path: String, detail: String },
    /// No runtime could be started to open the database and check it.
    NoRuntime { detail: String },
    /// The store at `path` was written before taskloom moved onto
    /// angzarr-embedded (annals-store-sql's): it is not migrated in place
    /// (the owner's ruling D2), but moved aside to `backup` and started
    /// afresh.
    PredatesEmbedded { path: String, backup: String },
}

impl fmt::Display for StoreOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreOpenError::CreateDir { path, detail } => {
                write!(f, "cannot create the store's directory {path}: {detail}")
            }
            StoreOpenError::Unplaced { path, detail } => {
                write!(f, "cannot find where the store {path} is on disk: {detail}")
            }
            StoreOpenError::NoRuntime { detail } => {
                write!(f, "cannot start a runtime to check the store: {detail}")
            }
            StoreOpenError::PredatesEmbedded { path, backup } => write!(
                f,
                "the store {path} predates angzarr-embedded: move it to {backup} to start afresh"
            ),
        }
    }
}

impl Diagnostic for StoreOpenError {
    fn code(&self) -> &'static str {
        match self {
            StoreOpenError::CreateDir { .. } => "task-store-sqlite-create-dir",
            StoreOpenError::Unplaced { .. } => "task-store-sqlite-unplaced",
            StoreOpenError::NoRuntime { .. } => "task-store-sqlite-no-runtime",
            StoreOpenError::PredatesEmbedded { .. } => "task-store-sqlite-predates-embedded",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        match self {
            StoreOpenError::CreateDir { path, detail }
            | StoreOpenError::Unplaced { path, detail } => vec![
                ("path", path.as_str().into()),
                ("detail", detail.as_str().into()),
            ],
            StoreOpenError::NoRuntime { detail } => vec![("detail", detail.as_str().into())],
            StoreOpenError::PredatesEmbedded { path, backup } => vec![
                ("path", path.as_str().into()),
                ("backup", backup.as_str().into()),
            ],
        }
    }
}

impl StoreOpenError {
    /// The error as angzarr-ports' cause, its code and args: how the
    /// angzarr-embedded store carries it in a `StoreError`.
    pub fn cause(&self) -> angzarr_ports::Cause {
        let args = self.args().into_iter().filter_map(|(name, arg)| match arg {
            Arg::Str(text) => Some((name, angzarr_ports::Arg::Str(text))),
            _ => None,
        });
        angzarr_ports::Cause::new(self.code(), args)
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    "task-store-sqlite",
    include_str!("../locales/en-US/task-store-sqlite.ftl"),
);

/// One value of every error, for the catalog test.
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let path = || "/home/u/.ctxloom/projects/jolly-mute-magma".to_string();
    vec![
        Box::new(StoreOpenError::CreateDir {
            path: path(),
            detail: "permission denied".to_string(),
        }),
        Box::new(StoreOpenError::Unplaced {
            path: format!("{}/state.db", path()),
            detail: "not an absolute path: C:state.db".to_string(),
        }),
        Box::new(StoreOpenError::NoRuntime {
            detail: "too many open files".to_string(),
        }),
        Box::new(StoreOpenError::PredatesEmbedded {
            path: format!("{}/state.db", path()),
            backup: format!("{}/state.db.pre-embedded", path()),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
