//! Why a store command did not complete, and the note an export ends
//! with.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};
use angzarr_store_transfer::ExportError;
use plans_application::PlanError;
use project_application::ProjectError;
use tasks_application::AppError;

#[derive(Clone, PartialEq, Debug)]
pub enum CliError {
    /// The export failed: the project use case's diagnostic.
    Project(ProjectError),
    /// The tasks' digest failed: its use case's diagnostic.
    Tasks(AppError),
    /// The plans' digest failed: its use case's diagnostic.
    Plans(PlanError),
    /// A digest line could not be written: the transfer's diagnostic.
    Digest(ExportError),
    /// The answer could not be written out.
    Output { detail: String },
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for CliError {
    fn code(&self) -> &str {
        match self {
            CliError::Project(error) => error.code(),
            CliError::Tasks(error) => error.code(),
            CliError::Plans(error) => error.code(),
            CliError::Digest(error) => error.code(),
            CliError::Output { .. } => code!("output"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            CliError::Project(error) => error.args(),
            CliError::Tasks(error) => error.args(),
            CliError::Plans(error) => error.args(),
            CliError::Digest(error) => error.args(),
            CliError::Output { detail } => vec![("ext_err_txt", detail.as_str().into())],
        }
    }
}

/// What an export wrote, said on stderr once it is done, so stdout holds
/// the file alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Exported {
    pub records: u64,
    pub streams: u64,
}

impl fmt::Display for Exported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for Exported {
    fn code(&self) -> &str {
        code!("exported")
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        vec![
            ("records", self.records.into()),
            ("streams", self.streams.into()),
        ]
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!(concat!(env!("OUT_DIR"), "/en-US.ftl")),
);

/// One value of every diagnostic this crate defines (not the wrappers),
/// for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(CliError::Output {
            detail: "broken pipe".to_string(),
        }),
        Box::new(Exported {
            records: 6171,
            streams: 412,
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
