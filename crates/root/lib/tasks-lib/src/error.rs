//! Why the wiring could not be built.

use std::fmt;

use annals_diagnostic::{Arg, Diagnostic};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BuildError {
    /// The working directory given is not absolute, so the real
    /// filesystem has nothing to resolve relative paths against.
    RelativeCwd { cwd: String },
    /// The process's working directory could not be read (it was
    /// deleted, or is not searchable): there is nothing to resolve
    /// relative paths against, and no fallback is assumed.
    NoWorkingDirectory { reason: String },
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::RelativeCwd { cwd } => {
                write!(f, "the working directory is not an absolute path: {cwd}")
            }
            BuildError::NoWorkingDirectory { reason } => {
                write!(f, "cannot read the working directory: {reason}")
            }
        }
    }
}

impl std::error::Error for BuildError {}

impl Diagnostic for BuildError {
    fn code(&self) -> &'static str {
        match self {
            BuildError::RelativeCwd { .. } => "tasks-lib-relative-cwd",
            BuildError::NoWorkingDirectory { .. } => "tasks-lib-no-working-directory",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        match self {
            BuildError::RelativeCwd { cwd } => vec![("cwd", cwd.as_str().into())],
            BuildError::NoWorkingDirectory { reason } => vec![("reason", reason.as_str().into())],
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = ("tasks-lib", include_str!("../locales/en-US/tasks-lib.ftl"));

/// One value of every error, for the catalog test.
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(BuildError::RelativeCwd {
            cwd: "work/taskloom".to_string(),
        }),
        Box::new(BuildError::NoWorkingDirectory {
            reason: "No such file or directory (os error 2)".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
