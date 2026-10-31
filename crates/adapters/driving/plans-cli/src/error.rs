//! Why a plan command did not complete.

use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};
use plans_application::PlanError;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CliError {
    /// The use case failed: its error's diagnostic.
    Plan(PlanError),
    /// The answer could not be written out.
    Output { detail: String },
    /// A status that is none of the plan statuses: a usage error.
    UnknownStatus { status: String },
    /// `plan link` or `unlink` naming no link: a usage error.
    NoLink { command: &'static str },
    /// `plan show` in text, whose answer is the file, of a plan whose file
    /// is missing.
    FileMissing { plan: String, path: String },
}

impl CliError {
    /// The exit code it ends the command with: 2 for a usage error, else 1.
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::UnknownStatus { .. } | CliError::NoLink { .. } => 2,
            _ => 1,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for CliError {
    fn code(&self) -> &'static str {
        match self {
            CliError::Plan(error) => error.code(),
            CliError::Output { .. } => "plans-cli-output",
            CliError::UnknownStatus { .. } => "plans-cli-unknown-status",
            CliError::NoLink { .. } => "plans-cli-no-link",
            CliError::FileMissing { .. } => "plans-cli-file-missing",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        match self {
            CliError::Plan(error) => error.args(),
            CliError::Output { detail } => vec![("ext_err_txt", detail.as_str().into())],
            CliError::UnknownStatus { status } => vec![("status", status.as_str().into())],
            CliError::NoLink { command } => vec![("command", (*command).into())],
            CliError::FileMissing { plan, path } => vec![
                ("plan", plan.as_str().into()),
                ("path", path.as_str().into()),
            ],
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = ("plans-cli", include_str!("../locales/en-US/plans-cli.ftl"));

/// One value of every error this crate defines (not the wrapper), for the
/// catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(CliError::Output {
            detail: "broken pipe".to_string(),
        }),
        Box::new(CliError::UnknownStatus {
            status: "nope".to_string(),
        }),
        Box::new(CliError::NoLink { command: "link" }),
        Box::new(CliError::FileMissing {
            plan: "brisk-cedar-otter".to_string(),
            path: ".taskloom/plans/brisk-cedar-otter.plan.md".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
