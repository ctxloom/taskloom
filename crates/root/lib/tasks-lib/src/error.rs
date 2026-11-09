//! Why the wiring could not be built.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};
use project_registry::RegistryError;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BuildError {
    /// The working directory given is not absolute, so the real
    /// filesystem has nothing to resolve relative paths against.
    RelativeCwd { cwd: String },
    /// The marker's location given is not a path under the project's root:
    /// the registry's own refusal, with its code, args and message.
    InvalidMarker(RegistryError),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for BuildError {}

impl Diagnostic for BuildError {
    fn code(&self) -> &str {
        match self {
            BuildError::RelativeCwd { .. } => code!("relative-cwd"),
            BuildError::InvalidMarker(refusal) => refusal.code(),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            BuildError::RelativeCwd { cwd } => vec![("cwd", cwd.as_str().into())],
            BuildError::InvalidMarker(refusal) => refusal.args(),
        }
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
    vec![Box::new(BuildError::RelativeCwd {
        cwd: "work/taskloom".to_string(),
    })]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
