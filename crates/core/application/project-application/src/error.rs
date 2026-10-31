//! Why a project use case did not complete.

use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic, Report};

/// Why a project use case did not complete. [`ProjectError::Port`] is the
/// port's own diagnostic: its code, args and English.
#[derive(Clone, PartialEq, Debug)]
pub enum ProjectError {
    /// The registry, a marker or the projects directory failed, or the
    /// check of a project's database could not run.
    Port(Report),
    /// A repoint to an id neither registered nor with a database.
    UnknownProject { id: String },
    /// A repoint to an id registered for another tree that still exists.
    InUse { id: String, path: String },
    /// A removal named a database something still points at, or none.
    NotOrphan { id: String },
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for ProjectError {
    fn code(&self) -> &str {
        match self {
            ProjectError::Port(cause) => cause.code(),
            ProjectError::UnknownProject { .. } => "project-application-unknown-project",
            ProjectError::InUse { .. } => "project-application-in-use",
            ProjectError::NotOrphan { .. } => "project-application-not-orphan",
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            ProjectError::Port(cause) => cause.args(),
            ProjectError::UnknownProject { id } | ProjectError::NotOrphan { id } => {
                vec![("id", id.as_str().into())]
            }
            ProjectError::InUse { id, path } => {
                vec![("id", id.as_str().into()), ("path", path.as_str().into())]
            }
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    "project-application",
    include_str!("../locales/en-US/project-application.ftl"),
);

/// One value of every error this crate defines (not the wrapper), for the
/// catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(ProjectError::UnknownProject {
            id: "tpyo-id".to_string(),
        }),
        Box::new(ProjectError::InUse {
            id: "jolly-mute-magma".to_string(),
            path: "/work/old".to_string(),
        }),
        Box::new(ProjectError::NotOrphan {
            id: "jolly-mute-magma".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
