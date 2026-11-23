//! Why a plan command did not complete.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};
use cli_support::{Exit, ExitClass};
use plans_application::PlanError;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CliError {
    /// The use case failed: its error's diagnostic.
    Plan(PlanError),
    /// The answer could not be written out.
    Output { detail: String },
    /// `plans link` or `unlink` naming no link: a usage error.
    NoLink { command: &'static str },
    /// The stamp-plan hook was given a payload that is not JSON; `detail`
    /// is the parser's text.
    HookPayload { detail: String },
    /// The stamp-plan hook's payload names no file.
    HookNoFile,
    /// The stamp-plan hook could not stamp: said, and the hook ends well
    /// all the same, since it must not block the edit it follows.
    HookFailed { error: PlanError },
    /// The watch could not go on.
    WatchFailed { error: PlanError },
}

/// A decline (48) for a request invalid on its own content (a usage error
/// the parser cannot see, a stamp-plan payload that is no JSON or names no
/// file); the use case's own exit for what it answered; a failure (32)
/// where the answer could not be written out.
impl ExitClass for CliError {
    fn exit(&self) -> Exit {
        match self {
            CliError::NoLink { .. } | CliError::HookPayload { .. } | CliError::HookNoFile => {
                Exit::REFUSED
            }
            CliError::Plan(error)
            | CliError::HookFailed { error }
            | CliError::WatchFailed { error } => error.exit(),
            CliError::Output { .. } => Exit::FAILED,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for CliError {
    fn code(&self) -> &str {
        match self {
            CliError::Plan(error) => error.code(),
            CliError::Output { .. } => code!("output"),
            CliError::NoLink { .. } => code!("no-link"),
            CliError::HookPayload { .. } => code!("hook-payload"),
            CliError::HookNoFile => code!("hook-no-file"),
            CliError::HookFailed { .. } => code!("hook-failed"),
            CliError::WatchFailed { .. } => code!("watch-failed"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            CliError::Plan(error) => error.args(),
            CliError::Output { detail } => vec![("ext_err_txt", detail.as_str().into())],
            CliError::NoLink { command } => vec![("command", (*command).into())],
            CliError::HookPayload { detail } => vec![("ext_err_txt", detail.as_str().into())],
            CliError::HookNoFile => Vec::new(),
            CliError::HookFailed { error } | CliError::WatchFailed { error } => {
                vec![("error", Arg::nested(error))]
            }
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
        Box::new(CliError::Output {
            detail: "broken pipe".to_string(),
        }),
        Box::new(CliError::NoLink { command: "link" }),
        Box::new(CliError::HookPayload {
            detail: "expected value at line 1".to_string(),
        }),
        Box::new(CliError::HookNoFile),
        Box::new(CliError::HookFailed {
            error: PlanError::NoChange,
        }),
        Box::new(CliError::WatchFailed {
            error: PlanError::NoChange,
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
