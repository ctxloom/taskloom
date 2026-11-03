//! The `plans` command group: `add`, `list`, `show`, `set`, `tag`, `link`,
//! `unlink`, `check` and `watch`, and the `stamp-plan` hook, translated
//! into the plans use cases ([`Plans`]) and their answers rendered through
//! presentation, as text or one JSON document. It depends on no tasks
//! crate, so any binary mounts it: taskloom does, beside its task
//! commands.
//!
//! A binary mounts [`Command`] under its own `plans` group, and [`Group`]
//! beside it for `watch`, parses its own global flags (the format, the
//! pinned project), and hands the command, those and its catalog (with
//! this crate's [`EN_US`] and the use cases') to [`run`]. Text on a
//! terminal, JSON otherwise, unless `--format` says: `plans show` too,
//! whose text is the plan's markdown, raw (the VS Code extension asks for
//! it with `--format text`).
//!
//! Exit codes: 0; 2 for a request invalid on its own content (a status
//! that is none, a tag no rule admits, a blank plan root, a link naming
//! nothing, a supersede cycle); 1 when it ran against state and the answer
//! was negative (no such plan or task, a `check` finding), with the error
//! on stderr (its catalog message in text, its envelope in JSON).
//!
//! The VS Code extension's contract (ctxloom-vscode `plans-data.ts`):
//! `plans list --format json` is an array of objects each with `path`
//! (absolute), `name`, `title`, `session` and `sessions`.

mod error;
mod hook;
mod render;
mod watch;

use std::io::{self, Write};

use angzarr_presentation::{Catalog, Format, Render};
use clap::{Args, Subcommand};
use plans_application::{Link, NewPlan, PlanQuery, SetPlan};

#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{CliError, EN_US};
pub use hook::stamp;
pub use plans_application::Plans;
use render::{Checked, Listed, Shown, Written};
pub use watch::{run as watch, Ticks};

/// The `plans` verbs that need nothing of the binary but the use cases.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Add a plan: write a new plan file with a minted id
    #[command(
        after_help = "Examples:\n  taskloom plans add --title 'Move plans into files'\n  taskloom plans add --title 'Parser' --governs swift-amber-falcon"
    )]
    Add(AddArgs),
    /// List plans
    #[command(
        after_help = "Examples:\n  taskloom plans list --governs swift-amber-falcon\n  taskloom plans list --tag-query area:cli/human:decision/or\n\nTasks keep no plan link. To find the plans governing a task, use --governs <harp>;\nto find the tasks a plan governs, `taskloom plans show <plan>` lists its\n`governs:` harps, then `taskloom tasks show <harp>...`."
    )]
    List(ListArgs),
    /// Print a plan's markdown (in JSON, its metadata and content)
    #[command(
        after_help = "Examples:\n  taskloom plans show brisk-cedar-otter\n  taskloom plans show docs/design.plan.md\n\nTo find the tasks a plan governs, read its `governs:` harps, then run\n`taskloom tasks show <harp>...`. To find the plans governing a task, run\n`taskloom plans list --governs <harp>`."
    )]
    Show(ShowArgs),
    /// Set a plan's title or status
    #[command(
        after_help = "Examples:\n  taskloom plans set brisk-cedar-otter --status active\n  taskloom plans set docs/design.plan.md --title 'Design'"
    )]
    Set(SetArgs),
    /// Add or remove a plan's tags
    #[command(subcommand)]
    Tag(TagCommand),
    /// Link a plan to the tasks it governs, or to a plan it supersedes
    #[command(
        after_help = "Examples:\n  taskloom plans link brisk-cedar-otter --governs swift-amber-falcon\n  taskloom plans link brisk-cedar-otter --supersedes quiet-amber-falcon"
    )]
    Link(LinkArgs),
    /// Remove a link `plans link` made
    #[command(
        after_help = "Examples:\n  taskloom plans unlink brisk-cedar-otter --governs swift-amber-falcon"
    )]
    Unlink(LinkArgs),
    /// Check every plan file; exit 1 on a finding
    #[command(
        after_help = "Examples:\n  taskloom plans check\n\nA finding is a frontmatter that does not parse, no id, a duplicate id, a tag the\nstore's rules do not admit, a governed task or a superseded plan that does not\nexist, or a status written by hand that is not one."
    )]
    Check,
}

/// The `plans` verb that streams.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Group {
    /// Stream plan-file changes as JSONL, one line per changed file
    #[command(after_help = "Examples:\n  taskloom plans watch\n\n\
Long-lived: runs until interrupted (Ctrl-C / SIGTERM); a closed stdin does not\n\
stop it. One line at once, then one per changed plan file, e.g.\n\
{\"event\":\"changed\",\"kind\":\"plans\",\"id\":\"quiet-amber-falcon\",\"path\":\"/work/p.plan.md\",\n\
\"changes\":{\"status\":{\"from\":\"draft\",\"to\":\"active\"}}}. `event` is created, changed or\n\
deleted; `changes` names each frontmatter key that differs (status, title, id as\n\
from/to; tags, governs, supersedes, sessions as added/removed) and `body: true` when\n\
the markdown differs. A line carries no file text: read the file for it.")]
    Watch,
}

/// `plans tag`.
#[derive(Subcommand, Debug)]
pub enum TagCommand {
    /// Add a tag to a plan
    #[command(after_help = "Examples:\n  taskloom plans tag add brisk-cedar-otter area:cli")]
    Add(TagArgs),
    /// Remove a tag from a plan
    #[command(after_help = "Examples:\n  taskloom plans tag remove brisk-cedar-otter area:cli")]
    Remove(TagArgs),
}

#[derive(Args, Debug)]
pub struct TagArgs {
    /// A plan's harp id, or its path.
    pub plan: String,
    /// A tag, in tagma's grammar: (namespace:)key(=value).
    pub tag: String,
}

#[derive(Args, Debug)]
pub struct AddArgs {
    /// The plan's title.
    #[arg(long)]
    pub title: String,
    /// Where to write the file (relative to the work root, or absolute),
    /// ending .plan.md; else the first plan root.
    #[arg(long)]
    pub path: Option<String>,
    /// draft (the default), active, done or abandoned.
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
    /// Only plans whose tags a postfix tag query selects.
    #[arg(long)]
    pub tag_query: Option<String>,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// A plan's harp id, or its path (relative to the work root, or
    /// absolute as `plans list` gives it).
    pub plan: String,
}

#[derive(Args, Debug)]
pub struct SetArgs {
    /// A plan's harp id, or its path.
    pub plan: String,
    /// The plan's title.
    #[arg(long)]
    pub title: Option<String>,
    /// draft, active, done or abandoned (superseded is derived from
    /// `plans link --supersedes`).
    #[arg(long)]
    pub status: Option<String>,
}

#[derive(Args, Debug)]
pub struct LinkArgs {
    /// A plan's harp id, or its path.
    pub plan: String,
    /// A task (repeatable).
    #[arg(long)]
    pub governs: Vec<String>,
    /// A plan, by id or path.
    #[arg(long, conflicts_with = "governs")]
    pub supersedes: Option<String>,
}

/// What the mounting binary parsed and read for the group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Text, or JSON (one document).
    pub format: Format,
    /// CTXLOOM_SESSION_HARP: the session the stamp-plan hook records.
    pub session: String,
    /// The program's name: every diagnostic's prefix.
    pub prog: &'static str,
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
    match answer(command, settings, catalog, plans, out).await {
        Ok(found) => u8::from(found),
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

fn rendered(out: &mut dyn Write, format: Format, view: &impl Render) -> Result<(), CliError> {
    angzarr_presentation::render(out, format, view).map_err(|e| CliError::Output {
        detail: e.to_string(),
    })
}

/// The command answered; whether it found something (`check`'s findings),
/// which is exit 1.
async fn answer<P: Plans>(
    command: Command,
    settings: &Settings,
    catalog: &Catalog,
    plans: &mut P,
    out: &mut dyn Write,
) -> Result<bool, CliError> {
    let format = settings.format;
    let plan_error = CliError::Plan;
    match command {
        Command::Add(args) => {
            let new = NewPlan {
                title: args.title,
                path: args.path,
                status: args.status,
                governs: args.governs,
            };
            let plan = plans.add(new).await.map_err(plan_error)?;
            rendered(out, format, &Written(&plan))?;
        }
        Command::List(args) => {
            let query = PlanQuery {
                statuses: args.status,
                governs: args.governs,
                tag_query: args.tag_query,
            };
            let listed = plans.list(&query).await.map_err(plan_error)?;
            rendered(out, format, &Listed(&listed))?;
        }
        Command::Show(args) => {
            let shown = plans.show(&args.plan).await.map_err(plan_error)?;
            rendered(out, format, &Shown(&shown))?;
        }
        Command::Set(args) => {
            let change = SetPlan {
                title: args.title,
                status: args.status,
            };
            let plan = plans.set(&args.plan, change).await.map_err(plan_error)?;
            rendered(out, format, &Written(&plan))?;
        }
        Command::Tag(TagCommand::Add(args)) => {
            let plan = plans.tag(&args.plan, &args.tag, true).await;
            rendered(out, format, &Written(&plan.map_err(plan_error)?))?;
        }
        Command::Tag(TagCommand::Remove(args)) => {
            let plan = plans.tag(&args.plan, &args.tag, false).await;
            rendered(out, format, &Written(&plan.map_err(plan_error)?))?;
        }
        Command::Link(args) => linking(args, true, plans, out, format).await?,
        Command::Unlink(args) => linking(args, false, plans, out, format).await?,
        Command::Check => {
            let report = plans.check().await.map_err(plan_error)?;
            rendered(
                out,
                format,
                &Checked {
                    report: &report,
                    catalog,
                },
            )?;
            return Ok(report.failed());
        }
    }
    Ok(false)
}

/// `plans link` (`linking`) or `unlink`: one call per link named, in
/// order, stopping at the first refused; the plan after the last.
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
    let mut last = None;
    for link in links {
        last = Some(
            plans
                .link(&args.plan, link, linking)
                .await
                .map_err(CliError::Plan)?,
        );
    }
    match &last {
        Some(plan) => rendered(out, format, &Written(plan)),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
