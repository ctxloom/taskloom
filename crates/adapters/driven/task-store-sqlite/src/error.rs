//! Why a project's store could not be opened, beyond what the store reports
//! itself. A third party's text (the OS's, through std's `io::Error`) is
//! the arg `ext_err_txt`, verbatim.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};

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
    /// The store at `path` was written by an older build, in an angzarr
    /// wire this build does not decode (an event page's `payload`, now
    /// `body`): it is an archive, never opened to write, migrated or backed
    /// up by this build. `archive` is where to move it, and the pre-port
    /// build reads it.
    OlderGeneration { path: String, archive: String },
    /// A store's export could not be written to its output.
    NoOutput { detail: String },
    /// The store's directory, file, log or shared memory at `path` could not
    /// be made its owner's alone (0700, 0600), so it is not opened.
    Restrict { path: String, detail: String },
}

/// Its key, then each arg as ` name=value`, a value quoted where it would
/// not parse back: never English (angzarr decision 14.37), as
/// angzarr-diagnostic's errors display themselves.
impl fmt::Display for StoreOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl StoreOpenError {
    /// Its key: a literal, so angzarr-ports' cause can hold it.
    fn key(&self) -> &'static str {
        match self {
            StoreOpenError::CreateDir { .. } => code!("create-dir"),
            StoreOpenError::Unplaced { .. } => code!("unplaced"),
            StoreOpenError::NoRuntime { .. } => code!("no-runtime"),
            StoreOpenError::PredatesEmbedded { .. } => code!("predates-embedded"),
            StoreOpenError::OlderGeneration { .. } => code!("older-generation"),
            StoreOpenError::NoOutput { .. } => code!("no-output"),
            StoreOpenError::Restrict { .. } => code!("restrict"),
        }
    }

    /// Its args, named by literals, so angzarr-ports' cause can hold them.
    fn named(&self) -> Vec<(&'static str, Arg)> {
        match self {
            StoreOpenError::CreateDir { path, detail }
            | StoreOpenError::Unplaced { path, detail }
            | StoreOpenError::Restrict { path, detail } => vec![
                ("path", path.as_str().into()),
                ("ext_err_txt", detail.as_str().into()),
            ],
            StoreOpenError::NoRuntime { detail } | StoreOpenError::NoOutput { detail } => {
                vec![("ext_err_txt", detail.as_str().into())]
            }
            StoreOpenError::PredatesEmbedded { path, backup } => vec![
                ("path", path.as_str().into()),
                ("backup", backup.as_str().into()),
            ],
            StoreOpenError::OlderGeneration { path, archive } => vec![
                ("path", path.as_str().into()),
                ("archive", archive.as_str().into()),
            ],
        }
    }
}

impl Diagnostic for StoreOpenError {
    fn code(&self) -> &str {
        self.key()
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        self.named()
    }
}

impl StoreOpenError {
    /// The error as angzarr-ports' cause, its code and args: how the
    /// angzarr-embedded store carries it in a `StoreError`.
    pub fn cause(&self) -> angzarr_ports::Cause {
        let args = self
            .named()
            .into_iter()
            .filter_map(|(name, arg)| match arg {
                Arg::Str(text) => Some((name, angzarr_ports::Arg::Str(text))),
                _ => None,
            });
        angzarr_ports::Cause::new(self.key(), args)
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
        Box::new(StoreOpenError::OlderGeneration {
            path: format!("{}/state.db", path()),
            archive: format!("{}/state.db.archive-<date>", path()),
        }),
        Box::new(StoreOpenError::NoOutput {
            detail: "broken pipe".to_string(),
        }),
        Box::new(StoreOpenError::Restrict {
            path: format!("{}/state.db-wal", path()),
            detail: "operation not permitted".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
