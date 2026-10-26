//! Why a project command did not complete.

use std::fmt;

use annals_diagnostic::{Arg, Diagnostic};
use project_application::ProjectError;

#[derive(Clone, PartialEq, Debug)]
pub enum CliError {
    /// The use case failed: its error's diagnostic.
    Project(ProjectError),
    /// The answer could not be written out.
    Output { detail: String },
    /// `--project` given to `command`, which acts on this directory's
    /// project and takes no pin: a usage error, never ignored.
    ProjectNotTaken { command: String },
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Project(error) => error.fmt(f),
            CliError::Output { detail } => write!(f, "cannot write the output: {detail}"),
            CliError::ProjectNotTaken { command } => write!(
                f,
                "--project is not taken by project {command}, which acts on this directory's project: only project show takes it"
            ),
        }
    }
}

impl Diagnostic for CliError {
    fn code(&self) -> &'static str {
        match self {
            CliError::Project(error) => error.code(),
            CliError::Output { .. } => "project-cli-output",
            CliError::ProjectNotTaken { .. } => "project-cli-project-not-taken",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        match self {
            CliError::Project(error) => error.args(),
            CliError::Output { detail } => vec![("detail", detail.as_str().into())],
            CliError::ProjectNotTaken { command } => vec![("command", command.as_str().into())],
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    "project-cli",
    include_str!("../locales/en-US/project-cli.ftl"),
);

/// One value of every error this crate defines (not the wrapper), for the
/// catalog test.
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(CliError::Output {
            detail: "broken pipe".to_string(),
        }),
        Box::new(CliError::ProjectNotTaken {
            command: "heal".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
