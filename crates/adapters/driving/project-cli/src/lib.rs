//! The project commands: `manage project show` ([`ProjectCommand`]) and
//! `manage doctor heal|repoint|orphans` ([`DoctorCommand`]), translated into
//! the project use cases and their answers rendered through presentation,
//! as text or one JSON document. It depends on no tasks crate, so any
//! binary mounts it: taskloom does, beside its task commands, and ctxloom
//! can.
//!
//! A binary mounts the two enums under its own groups, parses its own
//! global flags (the format, the pinned project), and hands the [`Command`],
//! those and its catalog (with this crate's [`EN_US`], the use cases' and
//! the store's, whose damage `heal` reports) to [`run`]. Exit 0, or 1 with
//! the error on stderr: its catalog message in text, its envelope in JSON.
//! Only `show` takes the pinned project; the others act on this
//! directory's and refuse one. A change (`heal`, `repoint`, `orphans
//! remove`) only reports what it would do unless `--yes` applies it;
//! `--dry-run` always wins over `--yes`.

mod error;
mod render;

use std::io::{self, Write};

use angzarr_presentation::{Catalog, Format};
use clap::Subcommand;
use project_application::{ProjectAdmin, Projects, StoreCheck};

#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{CliError, EN_US};
use render::{Orphans, Planned, Show, YesIgnored};

/// `manage project`.
#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum ProjectCommand {
    /// Show which project this directory is
    #[command(
        after_help = "Examples:\n  taskloom manage project show\n\nThe project is the work root's: CTXLOOM_ROOT if set, else the enclosing git work tree,\nelse the directory's own. Its id is committed in .project-id; the registry at\n~/.ctxloom/projects maps it to the trees sharing it, and holds its database."
    )]
    Show,
}

/// `manage doctor`.
#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum DoctorCommand {
    /// Repair the skew between .project-id and the registry (a preview unless --yes)
    #[command(
        after_help = "Examples:\n  taskloom manage doctor heal\n  taskloom manage doctor heal --yes"
    )]
    Heal {
        /// Make the changes.
        #[arg(long)]
        yes: bool,
        /// Report what it would change and change nothing (wins over --yes).
        #[arg(long)]
        dry_run: bool,
    },
    /// Bind this directory to an existing project (a preview unless --yes)
    #[command(after_help = "Examples:\n  taskloom manage doctor repoint swift-amber-falcon --yes")]
    Repoint {
        /// The project to bind to.
        project_id: String,
        /// Bind it even though another existing tree holds the project:
        /// the two trees are meant to share it, as clones with one
        /// .project-id do.
        #[arg(long)]
        share: bool,
        /// Make the changes.
        #[arg(long)]
        yes: bool,
        /// Report what it would change and change nothing (wins over --yes).
        #[arg(long)]
        dry_run: bool,
    },
    /// List the databases no live registered path points at
    Orphans {
        #[command(subcommand)]
        action: Option<OrphansAction>,
    },
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum OrphansAction {
    /// Delete orphaned databases (a preview unless --yes)
    #[command(
        after_help = "Examples:\n  taskloom manage doctor orphans remove swift-amber-falcon --yes"
    )]
    Remove {
        /// The orphaned databases' project ids.
        #[arg(required = true, num_args = 1..)]
        project_ids: Vec<String>,
        /// Delete them.
        #[arg(long)]
        yes: bool,
        /// Report what it would delete and delete nothing (wins over --yes).
        #[arg(long)]
        dry_run: bool,
    },
}

/// What [`run`] runs.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Project(ProjectCommand),
    Doctor(DoctorCommand),
}

/// What the mounting binary parsed for the group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Text, or JSON (one document).
    pub format: Format,
    /// `--project`: the id `show` reports, overriding the directory's;
    /// every other command refuses it.
    pub project: Option<String>,
    /// The program's name: every diagnostic's prefix.
    pub prog: &'static str,
}

/// Run `command` against `projects`, writing to `out` and `err` with
/// messages from `catalog`; the exit code.
pub fn run<A: ProjectAdmin, C: StoreCheck>(
    command: Command,
    settings: &Settings,
    catalog: &Catalog,
    projects: &mut Projects<A, C>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    match answer(command, settings, catalog, projects, out) {
        Ok(()) => 0,
        Err(error) => {
            let _ = report(&error, settings, catalog, err);
            1
        }
    }
}

/// `error` on stderr: its envelope in JSON; in text its catalog message
/// after the program's name.
fn report(
    error: &CliError,
    settings: &Settings,
    catalog: &Catalog,
    err: &mut dyn Write,
) -> io::Result<()> {
    match settings.format {
        Format::Json => angzarr_presentation::write_json(err, &catalog.envelope(error)),
        Format::Text => writeln!(err, "{}: {}", settings.prog, catalog.message(error)),
    }
}

fn answer<A: ProjectAdmin, C: StoreCheck>(
    command: Command,
    settings: &Settings,
    catalog: &Catalog,
    projects: &mut Projects<A, C>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let unpinned = match &command {
        Command::Project(ProjectCommand::Show) => None,
        Command::Doctor(DoctorCommand::Heal { .. }) => Some("heal"),
        Command::Doctor(DoctorCommand::Repoint { .. }) => Some("repoint"),
        Command::Doctor(DoctorCommand::Orphans { .. }) => Some("orphans"),
    };
    if let (Some(command), Some(_)) = (unpinned, &settings.project) {
        return Err(CliError::ProjectNotTaken {
            command: command.to_string(),
        });
    }
    let wrote = match command {
        Command::Project(ProjectCommand::Show) => {
            let shown = projects
                .show(settings.project.as_deref())
                .map_err(CliError::Project)?;
            angzarr_presentation::render(out, settings.format, &Show(&shown, catalog))
        }
        Command::Doctor(DoctorCommand::Heal { yes, dry_run }) => {
            let plan = projects.heal(yes && !dry_run).map_err(CliError::Project)?;
            planned(out, settings, catalog, &plan, yes && dry_run)
        }
        Command::Doctor(DoctorCommand::Repoint {
            project_id,
            share,
            yes,
            dry_run,
        }) => {
            let plan = projects
                .repoint(&project_id, share, yes && !dry_run)
                .map_err(CliError::Project)?;
            planned(out, settings, catalog, &plan, yes && dry_run)
        }
        Command::Doctor(DoctorCommand::Orphans { action: None }) => {
            let orphans = projects.orphans().map_err(CliError::Project)?;
            angzarr_presentation::render(out, settings.format, &Orphans(&orphans))
        }
        Command::Doctor(DoctorCommand::Orphans {
            action:
                Some(OrphansAction::Remove {
                    project_ids,
                    yes,
                    dry_run,
                }),
        }) => {
            let plan = projects
                .remove_orphans(&project_ids, yes && !dry_run)
                .map_err(CliError::Project)?;
            planned(out, settings, catalog, &plan, yes && dry_run)
        }
    };
    wrote.map_err(output_failed)
}

/// A plan, written; if `--yes` was overruled by `--dry-run`, saying so.
fn planned(
    out: &mut dyn Write,
    settings: &Settings,
    catalog: &Catalog,
    plan: &project_application::Plan,
    yes_ignored: bool,
) -> io::Result<()> {
    match yes_ignored {
        true => angzarr_presentation::render(out, settings.format, &YesIgnored(plan, catalog)),
        false => angzarr_presentation::render(out, settings.format, &Planned(plan, catalog)),
    }
}

fn output_failed(error: io::Error) -> CliError {
    CliError::Output {
        detail: error.to_string(),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
