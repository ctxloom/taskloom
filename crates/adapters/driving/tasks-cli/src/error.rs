//! Why a command did not complete.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};
use angzarr_presentation::UnknownFormat;
use plans_application::PlanError;
use tasks_application::AppError;

#[cfg(any(test, feature = "test-support"))]
use crate::Notice;

#[derive(Clone, PartialEq, Debug)]
pub enum CliError {
    /// The use case failed: its error's diagnostic.
    App(AppError),
    /// The plans' half of `show` (the plans governing each task) failed:
    /// its error's diagnostic.
    Plans(PlanError),
    /// The answer could not be written out.
    Output { detail: String },
    /// An import left something behind: projects it stopped in, and
    /// records it could not bring over.
    Incomplete { stopped: usize, lost: usize },
    /// The command line could not be parsed: clap's account of why, with
    /// the usage.
    Usage { detail: String },
    /// A format the command does not speak: yaml, which only `loadout`
    /// does.
    Format(UnknownFormat),
    /// `loadout` asked for in a format other than yaml.
    LoadoutFormat { format: String },
    /// A lint found violations.
    LintViolations { count: usize },
    /// A lint checked no target, so its finding nothing proves nothing.
    LintCheckedNothing,
    /// `schema check` found something wrong with the store's tag rules.
    SchemaFindings { count: usize },
    /// A spelling that was removed, not aliased: what to use instead.
    Removed { instead: String },
    /// A command whose place is built and whose work is not yet.
    NotBuilt { command: &'static str },
    /// `plan import`, removed: plans are files, so there is nothing to
    /// import.
    PlanImportRemoved,
}

impl CliError {
    /// The process exit code, by the owner's rule: 2 when the request is
    /// invalid on its own content and was refused before any state was read
    /// (a usage error, a format the command does not speak, a removed
    /// spelling, and what the use case judged invalid, as
    /// [`AppError::exit_code`] says); 1 when it ran against state and the
    /// answer was negative (a `check` that found something, a task that is
    /// not there, an output that could not be written).
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::App(error) => error.exit_code(),
            CliError::Plans(_)
            | CliError::NotBuilt { .. }
            | CliError::Output { .. }
            | CliError::Incomplete { .. }
            | CliError::LintViolations { .. }
            | CliError::LintCheckedNothing
            | CliError::SchemaFindings { .. } => 1,
            CliError::Usage { .. }
            | CliError::Format(_)
            | CliError::LoadoutFormat { .. }
            | CliError::Removed { .. }
            | CliError::PlanImportRemoved => 2,
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
            CliError::App(error) => error.code(),
            CliError::Plans(error) => error.code(),
            CliError::Output { .. } => code!("output"),
            CliError::Incomplete { .. } => code!("import-incomplete"),
            CliError::Usage { .. } => code!("usage"),
            CliError::Format(error) => error.code(),
            CliError::LoadoutFormat { .. } => code!("loadout-format"),
            CliError::LintViolations { .. } => code!("lint-violations"),
            CliError::LintCheckedNothing => code!("lint-checked-nothing"),
            CliError::SchemaFindings { .. } => code!("schema-findings"),
            CliError::Removed { .. } => code!("removed"),
            CliError::PlanImportRemoved => code!("plan-import-removed"),
            CliError::NotBuilt { .. } => code!("not-built"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            CliError::App(error) => error.args(),
            CliError::Plans(error) => error.args(),
            CliError::Output { detail } | CliError::Usage { detail } => {
                vec![("ext_err_txt", detail.as_str().into())]
            }
            CliError::Incomplete { stopped, lost } => {
                vec![("stopped", (*stopped).into()), ("lost", (*lost).into())]
            }
            CliError::Format(error) => error.args(),
            CliError::LoadoutFormat { format } => vec![("format", format.as_str().into())],
            CliError::LintViolations { count } | CliError::SchemaFindings { count } => {
                vec![("count", (*count).into())]
            }
            CliError::LintCheckedNothing | CliError::PlanImportRemoved => Vec::new(),
            CliError::Removed { instead } => vec![("instead", instead.as_str().into())],
            CliError::NotBuilt { command } => vec![("command", (*command).into())],
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!(concat!(env!("OUT_DIR"), "/en-US.ftl")),
);

/// One value of every error and notice this crate defines (not the
/// wrapper), for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(CliError::Output {
            detail: "broken pipe".to_string(),
        }),
        Box::new(CliError::Incomplete {
            stopped: 1,
            lost: 2,
        }),
        Box::new(CliError::Usage {
            detail: "error: unrecognized subcommand 'frobnicate'\n".to_string(),
        }),
        Box::new(CliError::LintViolations { count: 2 }),
        Box::new(CliError::LintCheckedNothing),
        Box::new(CliError::SchemaFindings { count: 1 }),
        Box::new(CliError::Removed {
            instead: "tasks add".to_string(),
        }),
        Box::new(CliError::PlanImportRemoved),
        Box::new(CliError::NotBuilt {
            command: "manage hook stamp-plan",
        }),
        Box::new(Notice::Preview {
            command: "taskloom manage schema permits remove area:cli --yes".to_string(),
        }),
        Box::new(Notice::Project {
            root: "/work/tree".to_string(),
            id: "jolly-mute-magma".to_string(),
        }),
        Box::new(Notice::UnrootedProject {
            id: "jolly-mute-magma".to_string(),
        }),
        Box::new(CliError::LoadoutFormat {
            format: "json".to_string(),
        }),
        Box::new(Notice::NoProject),
        Box::new(Notice::EveryProject {
            directory: "/tmp/somewhere".to_string(),
        }),
        Box::new(Notice::Hidden {
            hidden: 3,
            completed: 2,
            deferred: 1,
        }),
        Box::new(Notice::Omitted { omitted: 4 }),
        Box::new(Notice::NothingToPrune),
        Box::new(Notice::ReportOnly {
            command: "taskloom tasks prune --older-than 7d --status Done --yes".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
