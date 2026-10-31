//! What a command tells the person besides its answer, on stderr: never
//! on stdout, which stays one JSON document.

use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};
use tasks_application::Project;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Notice {
    /// The project a write went to.
    Project { root: String, id: String },
    /// The project a write went to, which has no root: a pinned project
    /// the registry holds no path for.
    UnrootedProject { id: String },
    /// A listing found no project here, so lists nothing.
    NoProject,
    /// A read found no project in `directory`, which lies in no work
    /// root, so it read every project.
    EveryProject { directory: String },
    /// A search's matches the default view hid: `completed` terminal,
    /// `deferred` Deferred.
    Hidden {
        hidden: usize,
        completed: usize,
        deferred: usize,
    },
    /// The tasks a limit left out.
    Omitted { omitted: usize },
    /// A prune found no project here, so prunes nothing.
    NothingToPrune,
    /// A prune reported what it would take, and took nothing: `command`
    /// takes it.
    ReportOnly { command: String },
}

impl Notice {
    pub fn project(project: &Project) -> Notice {
        let id = project.id.clone();
        match &project.root {
            Some(root) => Notice::Project {
                root: root.clone(),
                id,
            },
            None => Notice::UnrootedProject { id },
        }
    }
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for Notice {
    fn code(&self) -> &str {
        match self {
            Notice::Project { .. } => "tasks-cli-project",
            Notice::UnrootedProject { .. } => "tasks-cli-unrooted-project",
            Notice::NoProject => "tasks-cli-no-project",
            Notice::EveryProject { .. } => "tasks-cli-every-project",
            Notice::Hidden { .. } => "tasks-cli-hidden",
            Notice::Omitted { .. } => "tasks-cli-omitted",
            Notice::NothingToPrune => "tasks-cli-nothing-to-prune",
            Notice::ReportOnly { .. } => "tasks-cli-report-only",
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            Notice::Project { root, id } => {
                vec![("root", root.as_str().into()), ("id", id.as_str().into())]
            }
            Notice::UnrootedProject { id } => vec![("id", id.as_str().into())],
            Notice::EveryProject { directory } => vec![("directory", directory.as_str().into())],
            Notice::Hidden {
                hidden,
                completed,
                deferred,
            } => vec![
                ("hidden", (*hidden).into()),
                ("completed", (*completed).into()),
                ("deferred", (*deferred).into()),
            ],
            Notice::Omitted { omitted } => vec![("omitted", (*omitted).into())],
            Notice::ReportOnly { command } => vec![("command", command.as_str().into())],
            Notice::NoProject | Notice::NothingToPrune => Vec::new(),
        }
    }
}
