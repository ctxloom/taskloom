//! Why the wiring could not be built.

use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BuildError {
    /// The working directory given is not absolute, so the real
    /// filesystem has nothing to resolve relative paths against.
    RelativeCwd { cwd: String },
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl std::error::Error for BuildError {}

impl Diagnostic for BuildError {
    fn code(&self) -> &'static str {
        match self {
            BuildError::RelativeCwd { .. } => "tasks-lib-relative-cwd",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        match self {
            BuildError::RelativeCwd { cwd } => vec![("cwd", cwd.as_str().into())],
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = ("tasks-lib", include_str!("../locales/en-US/tasks-lib.ftl"));

/// One value of every error, for the catalog test.
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![Box::new(BuildError::RelativeCwd {
        cwd: "work/taskloom".to_string(),
    })]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
