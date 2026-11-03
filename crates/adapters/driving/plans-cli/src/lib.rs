//! The `plans` command group: `add`, `list`, `show`, `status`, `link`,
//! `unlink` and `check`, translated into the plans use cases
//! ([`Plans`]) and their answers rendered through presentation, as text or
//! one JSON document. It depends on no tasks crate, so any binary mounts
//! it: taskloom does, beside its task commands.
//!
//! A binary mounts [`Command`] under its own `plans` group, parses its own
//! global flags (the format, the pinned project), and hands the command,
//! those and its catalog (with this crate's [`EN_US`] and the use cases')
//! to [`run`]. Text on a terminal, JSON otherwise, unless `--format` says:
//! `plans show` too, whose text is the plan's markdown, raw (the VS Code
//! extension asks for it with `--format text`). Exit 0; 1 with the error
//! on stderr (its catalog message in text, its envelope in JSON), or when
//! `plans check` finds drift; 2 on a usage error the parser cannot see (a
//! status that is none, a link naming nothing).
//!
//! The VS Code extension's contract (ctxloom-vscode `plans-data.ts`):
//! `plans list --format json` is an array of objects each with `path`
//! (absolute), `name`, `title`, `session` and `sessions`.

mod error;
mod render;

use std::io::{self, Write};

use angzarr_presentation::{Catalog, Format, Render};
use clap::{Args, Subcommand};
use plans_application::{Link, NewPlan, PlanQuery, PlanStatus, PlanView};

#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{CliError, EN_US};
pub use plans_application::Plans;
use render::{Added, Checked, Linked, Listed, Moved, Shown};

/// The `plans` verbs.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Add a plan: write a new plan file, or register an existing one
    #[command(after_help = "Examples:\n  taskloom plans add --title 'Move plans into files'")]
    Add(AddArgs),
    /// List plans
    #[command(
        after_help = "Examples:\n  taskloom plans list --status active\n  taskloom plans list --governs swift-amber-falcon\n\nTasks keep no plan link. To find the plans governing a task, use --governs <harp>;\nto find the tasks a plan governs, `taskloom plans show <plan>` lists its\n`governs:` harps, then `taskloom tasks show <harp>...`."
    )]
    List(ListArgs),
    /// Print a plan's markdown (in JSON, its record and content)
    #[command(
        after_help = "Examples:\n  taskloom plans show brisk-cedar-otter\n\nTo find the tasks a plan governs, read its `governs:` harps, then run\n`taskloom tasks show <harp>...`."
    )]
    Show(ShowArgs),
    /// Set a plan's status
    #[command(after_help = "Examples:\n  taskloom plans status brisk-cedar-otter active")]
    Status(StatusArgs),
    /// Link a plan to the tasks it governs, or to a plan it supersedes
    #[command(
        after_help = "Examples:\n  taskloom plans link brisk-cedar-otter --governs swift-amber-falcon"
    )]
    Link(LinkArgs),
    /// Remove a link `plans link` made
    #[command(
        after_help = "Examples:\n  taskloom plans unlink brisk-cedar-otter --governs swift-amber-falcon"
    )]
    Unlink(LinkArgs),
    /// Report drift between the plan files and the store; exit 1 on a finding
    #[command(after_help = "Examples:\n  taskloom plans check")]
    Check,
}

#[derive(Args, Debug)]
pub struct AddArgs {
    /// The plan's title.
    #[arg(long)]
    pub title: String,
    /// An existing plan file to register, relative or absolute.
    #[arg(long)]
    pub path: Option<String>,
    /// draft (the default) or active.
    #[arg(long)]
    pub status: Option<String>,
    /// A task this plan governs (repeatable).
    #[arg(long)]
    pub governs: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Only plans in this status (repeatable, or comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub status: Vec<String>,
    /// Only plans governing this task.
    #[arg(long)]
    pub governs: Option<String>,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// A plan's harp id, or its path (relative, or absolute as `plans list`
    /// gives it).
    pub plan: String,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    /// The plan's id.
    pub plan: String,
    /// draft, active, done or abandoned (superseded is set by `plans link`).
    pub status: String,
}

#[derive(Args, Debug)]
pub struct LinkArgs {
    /// The plan's id.
    pub plan: String,
    /// A task (repeatable).
    #[arg(long)]
    pub governs: Vec<String>,
    /// A plan.
    #[arg(long, conflicts_with = "governs")]
    pub supersedes: Option<String>,
}

/// What the mounting binary parsed and read for the group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Text, or JSON (one document).
    pub format: Format,
    /// CTXLOOM_SESSION_HARP: the session a plan is added in.
    pub session: String,
    /// The program's name: every diagnostic's prefix.
    pub prog: &'static str,
}

/// How a command ended, short of an error.
enum Ended {
    Ok,
    /// Answered, but with something found (drift): exit 1.
    Found,
}

/// Run `command` against `plans`, writing to `out` and `err` with messages
/// from `catalog`; the exit code.
pub async fn run<P: Plans>(
    command: Command,
    settings: &Settings,
    catalog: &Catalog,
    plans: &mut P,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let answered = answer(command, settings, plans, out).await;
    // The kernel's dead letters, almost certainly bugs, are never left
    // unsaid, whatever the command came to.
    for letter in plans.dead_letters() {
        let line = plans_application::dead_letter_line(&letter.letter);
        let _ = writeln!(err, "{}: {line}", settings.prog);
    }
    match answered {
        Ok(Ended::Ok) => 0,
        Ok(Ended::Found) => 1,
        Err(error) => {
            let _ = report(&error, settings, catalog, err);
            error.exit_code()
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

fn status(name: &str) -> Result<PlanStatus, CliError> {
    PlanStatus::parse(name).ok_or_else(|| CliError::UnknownStatus {
        status: name.to_string(),
    })
}

fn rendered(out: &mut dyn Write, format: Format, view: &impl Render) -> Result<(), CliError> {
    angzarr_presentation::render(out, format, view).map_err(|e| CliError::Output {
        detail: e.to_string(),
    })
}

async fn answer<P: Plans>(
    command: Command,
    settings: &Settings,
    plans: &mut P,
    out: &mut dyn Write,
) -> Result<Ended, CliError> {
    let format = settings.format;
    match command {
        Command::Add(args) => {
            let new = NewPlan {
                title: args.title,
                path: args.path,
                status: args
                    .status
                    .as_deref()
                    .map(status)
                    .transpose()?
                    .unwrap_or_default(),
                governs: args.governs,
                session: settings.session.clone(),
            };
            let plan = plans.add(new).await.map_err(CliError::Plan)?;
            rendered(out, format, &Added(&plan))?;
        }
        Command::List(args) => {
            let query = PlanQuery {
                statuses: args
                    .status
                    .iter()
                    .map(|s| status(s))
                    .collect::<Result<_, _>>()?,
                governs: args.governs,
            };
            let listed = plans.list(&query).await.map_err(CliError::Plan)?;
            rendered(out, format, &Listed(&listed))?;
        }
        Command::Show(args) => {
            let shown = plans.show(&args.plan).await.map_err(CliError::Plan)?;
            if format == Format::Text && shown.content.is_none() {
                return Err(CliError::FileMissing {
                    plan: shown.plan.harp_id,
                    path: shown.plan.path,
                });
            }
            rendered(out, format, &Shown(&shown))?;
        }
        Command::Status(args) => {
            let to = status(&args.status)?;
            let plan = plans
                .set_status(&args.plan, to)
                .await
                .map_err(CliError::Plan)?;
            rendered(out, format, &Moved(&plan))?;
        }
        Command::Link(args) => linking(args, true, plans, out, format).await?,
        Command::Unlink(args) => linking(args, false, plans, out, format).await?,
        Command::Check => {
            let report = plans.check().await.map_err(CliError::Plan)?;
            rendered(out, format, &Checked(&report))?;
            if report.failed() {
                return Ok(Ended::Found);
            }
        }
    }
    Ok(Ended::Ok)
}

/// `plan link` (`linking`) or `unlink`: one call per link named, in order,
/// stopping at the first refused.
async fn linking<P: Plans>(
    args: LinkArgs,
    linking: bool,
    plans: &mut P,
    out: &mut dyn Write,
    format: Format,
) -> Result<(), CliError> {
    let command = if linking { "link" } else { "unlink" };
    let mut links: Vec<Link> = args.governs.into_iter().map(Link::Governs).collect();
    links.extend(args.supersedes.map(Link::Supersedes));
    if links.is_empty() {
        return Err(CliError::NoLink { command });
    }
    let (mut lines, mut last): (Vec<String>, Option<PlanView>) = (Vec::new(), None);
    for link in links {
        let (verb, target) = match (&link, linking) {
            (Link::Governs(task), true) => ("governs", task.clone()),
            (Link::Governs(task), false) => ("no longer governs", task.clone()),
            (Link::Supersedes(plan), true) => ("supersedes", plan.clone()),
            (Link::Supersedes(plan), false) => ("no longer supersedes", plan.clone()),
        };
        let linked = match linking {
            true => plans.link(&args.plan, link).await,
            false => plans.unlink(&args.plan, link).await,
        };
        let plan = match linked {
            Ok(plan) => plan,
            Err(error) => {
                // The links made before the one refused are said in text.
                if format == Format::Text {
                    lines
                        .iter()
                        .try_for_each(|l| writeln!(out, "{l}"))
                        .map_err(|e| CliError::Output {
                            detail: e.to_string(),
                        })?;
                }
                return Err(CliError::Plan(error));
            }
        };
        lines.push(format!("{} {verb} {target}", args.plan));
        last = Some(plan);
    }
    match &last {
        Some(plan) => rendered(
            out,
            format,
            &Linked {
                plan,
                lines: &lines,
            },
        ),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
