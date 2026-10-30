//! What `manage` says besides its answer, why it gave none; and this
//! crate's catalog.

use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};
use registration_application::RegistrationError;

/// Why `manage` did not answer.
#[derive(Clone, PartialEq, Debug)]
pub enum ManageError {
    /// The use case failed: its error's diagnostic.
    App(RegistrationError),
    /// The command line did not parse: clap's words.
    Usage { detail: String },
    /// The answer could not be written.
    Output { detail: String },
}

/// What `manage` tells the person on stderr: each line after the first of
/// a notice is indented under it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ManageNotice {
    /// The server's command is not on this PATH (an agent looks for it on
    /// its own). The search path is named as `flavor` refers to it
    /// (`$PATH`, or `%PATH%` on Windows).
    Unresolvable {
        command: String,
        flavor: os::Flavor,
    },
    /// Under [`ManageNotice::Unresolvable`]: what that means.
    UnresolvableHint {
        command: String,
    },
    Registered {
        engine: String,
    },
    Removed {
        engine: String,
    },
    NotRegistered {
        engine: String,
    },
    /// An uninstall found no agent in use.
    NothingToRemove,
    /// Under a registration's notice: the file.
    Config {
        path: String,
    },
}

impl fmt::Display for ManageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl std::error::Error for ManageError {}

impl fmt::Display for ManageNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for ManageError {
    fn code(&self) -> &'static str {
        match self {
            ManageError::App(error) => error.code(),
            ManageError::Usage { .. } => "registration-cli-usage",
            ManageError::Output { .. } => "registration-cli-output",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        match self {
            ManageError::App(error) => error.args(),
            ManageError::Usage { detail } | ManageError::Output { detail } => {
                vec![("ext_err_txt", detail.as_str().into())]
            }
        }
    }
}

impl Diagnostic for ManageNotice {
    fn code(&self) -> &'static str {
        match self {
            ManageNotice::Unresolvable { .. } => "registration-cli-unresolvable",
            ManageNotice::UnresolvableHint { .. } => "registration-cli-unresolvable-hint",
            ManageNotice::Registered { .. } => "registration-cli-registered",
            ManageNotice::Removed { .. } => "registration-cli-removed",
            ManageNotice::NotRegistered { .. } => "registration-cli-not-registered",
            ManageNotice::NothingToRemove => "registration-cli-nothing-to-remove",
            ManageNotice::Config { .. } => "registration-cli-config",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        match self {
            ManageNotice::Unresolvable { command, flavor } => vec![
                ("command", command.as_str().into()),
                ("path_var", flavor.var_ref("PATH").into()),
            ],
            ManageNotice::UnresolvableHint { command } => {
                vec![("command", command.as_str().into())]
            }
            ManageNotice::Registered { engine }
            | ManageNotice::Removed { engine }
            | ManageNotice::NotRegistered { engine } => vec![("engine", engine.as_str().into())],
            ManageNotice::NothingToRemove => Vec::new(),
            ManageNotice::Config { path } => vec![("path", path.as_str().into())],
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    "registration-cli",
    include_str!("../locales/en-US/registration-cli.ftl"),
);

/// One value of every error and notice this crate defines (not the
/// wrapper), for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let engine = || "claude-code".to_string();
    let command = || "taskloom".to_string();
    vec![
        Box::new(ManageError::Usage {
            detail: "error: unexpected argument '--frob' found".to_string(),
        }),
        Box::new(ManageError::Output {
            detail: "Broken pipe (os error 32)".to_string(),
        }),
        Box::new(ManageNotice::Unresolvable {
            command: command(),
            flavor: os::Flavor::Unix,
        }),
        Box::new(ManageNotice::UnresolvableHint { command: command() }),
        Box::new(ManageNotice::Registered { engine: engine() }),
        Box::new(ManageNotice::Removed { engine: engine() }),
        Box::new(ManageNotice::NotRegistered { engine: engine() }),
        Box::new(ManageNotice::NothingToRemove),
        Box::new(ManageNotice::Config {
            path: "/home/me/.claude.json".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
