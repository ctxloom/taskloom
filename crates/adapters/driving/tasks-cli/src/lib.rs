//! The tasks command line: `add`, `status`, `edit`, `list`, `show`,
//! `summary`, `statuses`, `tag`, `tags`, `lint` and `prune`, translated
//! into the application's use cases and their answers rendered through
//! presentation ([`Command`], [`run`]); `import`, a group of its own
//! ([`ImportGroup`], [`run_import`]), so only it reaches the importer; and
//! the companion contract ctxloom reads, `version` and `loadout`. A binary
//! owns the parser: it flattens [`Globals`] and these commands into its
//! own, beside whatever else it mounts, and [`parse`] answers help and
//! usage errors as the task commands do. Text on a terminal, JSON otherwise, unless
//! `--format` or `--json` says; stdout holds the answer alone (one JSON
//! document), and notices, such as the project a write went to, go to
//! stderr. Exit 0, or 1 with the error on stderr: its catalog message in
//! text, its envelope in JSON.

mod commands;
mod companion;
mod error;
mod import;
mod notice;
mod prune;
mod render;

use std::ffi::OsString;
use std::io::{self, Write};

use annals_diagnostic::Diagnostic;
use clap::{Args, Parser, Subcommand};
use presentation::{Catalog, CatalogError, Format, Render, Source, UnknownFormat};
use tasks_application::{DeadLetter, PermitService, PruneService, TaskService, Written};

pub use companion::{Asked, LOADOUT};
pub use error::{diagnostic_samples, CliError, EN_US};
pub use import::{run_import, ImportGroup};

use companion::VersionJson;
use notice::Notice;

/// The program's name: every diagnostic's prefix.
pub const PROG: &str = "taskloom";

/// What the command line reads from its process besides its arguments.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Context {
    /// CTXLOOM_SESSION_HARP: the session a task is added in.
    pub session: String,
    /// Whether stdout is a terminal, which picks text over JSON.
    pub stdout_is_terminal: bool,
    /// The binary's version, as `version` reports it.
    pub version: String,
    /// The lock-taking protocol the binary's lock takers follow, as
    /// `version` reports it: ctxloom asks every running binary for it
    /// before it removes a lock file. 0 when the binary takes no shared
    /// lock, and then left out.
    pub lock_protocol: u32,
}

/// The catalog of every diagnostic the command line prints: its own, the
/// use cases', and `adapters'`, the driven adapters' catalogs, which only
/// the composition root knows.
pub fn catalog(adapters: &[Source]) -> Result<Catalog, CatalogError> {
    let mut sources = vec![EN_US];
    sources.extend(tasks_application::CATALOGS);
    sources.extend_from_slice(adapters);
    Catalog::en_us(&sources)
}

/// The global flags, every command's: a binary flattens them into its
/// parser.
#[derive(Args, Clone, PartialEq, Eq, Debug, Default)]
pub struct Globals {
    /// Output format: text, or json (the default when stdout is not a
    /// terminal); yaml, for loadout alone.
    #[arg(long, global = true)]
    pub format: Option<Asked>,
    /// Shorthand for --format json.
    #[arg(long, global = true)]
    pub json: bool,
    /// Project id to act on (overrides CTXLOOM_PROJECT_ID and the working directory's).
    #[arg(long, global = true)]
    pub project: Option<String>,
    /// Override a configuration key for this run: <path>=<value>, e.g.
    /// tag_schema=<declaration>,... (repeatable; beats TASKLOOM_CONFIG_*
    /// and every config.yaml). The task commands, mcp and watch take it;
    /// import, project and manage do not.
    #[arg(long = "config-set", global = true, value_name = "PATH=VALUE")]
    pub config_set: Vec<String>,
}

/// The task commands and the companion contract's.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Add a new task.
    Add(commands::AddArgs),
    /// List tasks: by default the active ones (not Done, Archived or
    /// Deferred), of this project, or of every project with --global (and
    /// where the working directory is in no project, no git repository and
    /// no CTXLOOM_ROOT, saying so on stderr).
    List(commands::ListArgs),
    /// Change the status of a task, or its trigger: a revive trigger defers
    /// the task, hiding it from the default list until it fires, and
    /// clearing it returns the task to the status it held.
    Status(commands::StatusArgs),
    /// Replace a task's text whole; its status and trigger are kept.
    Edit(commands::EditArgs),
    /// Show tasks whole, in the order asked: status, tags, trigger and
    /// every line of their text. Every id must be found, or nothing is
    /// shown. In JSON one id is an object, several an array.
    Show(commands::ShowArgs),
    /// Check every task's tags, whatever its status, against the tag
    /// schema: a value outside its enum or range, a scalar target with two
    /// values, and a formula naming a value no enum declares. Advisory,
    /// never a write gate; it exits 1 on a violation, and when the schema
    /// gave it no target to check.
    Lint(commands::GlobalArgs),
    /// Count the tasks in each status, every task included whatever list
    /// would hide, and name those In Progress.
    Summary(commands::GlobalArgs),
    /// List the statuses: the defaults in their display order (In
    /// Progress, To Do, Deferred, Done, Archived), then any other in use,
    /// each saying whether it is terminal or requires a trigger. @open and
    /// @terminal are the classes of the statuses not terminal and terminal.
    Statuses(commands::GlobalArgs),
    /// Add and/or remove tags on a task; adds apply before removes.
    Tag(commands::TagArgs),
    /// List the tags in use, with how many tasks hold each: "active" those
    /// a default list shows, "total" every one, finished or Deferred too.
    /// The tag schema's hidden tags are left out. The filters narrow the
    /// tasks counted.
    Tags(commands::TagsArgs),
    /// Drop old finished tasks from the store (reports only, without
    /// --yes). A task is selected when its status is one of --status, its
    /// last status change is older than --older-than, and it matches
    /// --tag-query, if given. With --yes each is removed, so every list
    /// drops it, and its stream purged, freeing its harp to be minted
    /// again; without it nothing changes. A surviving task naming a pruned
    /// one (a relates:<harp> tag, or the harp in its text or trigger) is
    /// reported, and left as written. A task an earlier prune removed but
    /// could not purge is taken too, whatever the flags select. To keep a
    /// finished task, archive it and leave Archived out of --status.
    Prune(commands::PruneArgs),
    /// Permit a tag in this project, in its .taskloom/config.yaml
    /// (tag_permits): a namespace, open to any key (`area:`); a key of a
    /// namespace, which closes it to its permitted keys (`area:cli`); or a
    /// flat tag (`urgent`). Once a project permits anything, a write adding
    /// a tag it does not permit is refused. A person's command: the MCP
    /// server does not offer it.
    Permit(commands::PermitArgs),
    /// Revoke a tag permit, spelled as `permit` takes it; refused, naming
    /// them, while any task (whatever its status) still carries a tag it
    /// permits.
    Revoke(commands::PermitArgs),
    /// List what this project permits (its .taskloom/config.yaml's
    /// tag_permits): each namespace, open to any key or closed to its keys;
    /// the flat tags; and the targets the tag schema declares, which are
    /// permitted without a permit. A project that declares none permits
    /// every tag the schema admits. Read-only; the MCP server's
    /// taskloom://tag-permits resource says the same.
    Permits,
    /// Print the taskloom version.
    Version,
    /// Print the context, commands, hooks and MCP servers taskloom
    /// contributes to a session: the loadout document ctxloom reads
    /// (`taskloom loadout --format yaml`).
    Loadout,
}

impl Globals {
    /// The format asked for, else text on a terminal and JSON off one; or,
    /// when it is yaml, which only `loadout` speaks, the exit code of
    /// refusing it on `err`.
    pub fn format(
        &self,
        context: &Context,
        catalog: &Catalog,
        err: &mut dyn Write,
    ) -> Result<Format, u8> {
        let asked = match self.format {
            Some(Asked::Yaml) => {
                let refused = CliError::Format(UnknownFormat {
                    value: "yaml".to_string(),
                });
                return Err(finish(Err(refused), self.fallback(context), catalog, err));
            }
            Some(Asked::Format(format)) => Some(format),
            None => None,
        };
        Ok(Format::choose(self.json, asked, context.stdout_is_terminal))
    }

    /// The format an error is reported in when `--format` gave none: JSON
    /// under `--json` or off a terminal.
    fn fallback(&self, context: &Context) -> Format {
        Format::choose(self.json, None, context.stdout_is_terminal)
    }
}

/// The format `args` ask for, read without parsing them: for a usage
/// error, where clap parsed nothing.
fn asked_format(args: &[OsString], context: &Context) -> Format {
    let (mut json, mut asked) = (false, None);
    let mut words = args.iter().skip(1).map(|a| a.to_str().unwrap_or_default());
    while let Some(word) = words.next() {
        let value = match word {
            "--" => break,
            "--json" => {
                json = true;
                continue;
            }
            "--format" => words.next(),
            _ => word.strip_prefix("--format="),
        };
        asked = value.and_then(|v| v.parse().ok()).or(asked);
    }
    Format::choose(json, asked, context.stdout_is_terminal)
}

/// The command line `args` (the program name first) parsed as `P`, or the
/// exit code of answering it without a command: help and --version on
/// `out`, a usage error on `err` (in the format the words ask for, since
/// clap parsed nothing).
pub fn parse<P: Parser>(
    args: impl IntoIterator<Item = OsString>,
    context: &Context,
    catalog: &Catalog,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<P, u8> {
    let args: Vec<OsString> = args.into_iter().collect();
    P::try_parse_from(&args).map_err(|e| {
        if e.use_stderr() {
            let usage = CliError::Usage {
                detail: e.render().to_string(),
            };
            let _ = report(&usage, asked_format(&args, context), catalog, err);
            return 1;
        }
        let _ = write!(out, "{}", e.render());
        0
    })
}

/// Run `command` under the global flags against `service`, writing to
/// `out` and `err` with messages from `catalog`; the exit code.
pub async fn run<S: TaskService + PruneService + PermitService>(
    command: Command,
    globals: Globals,
    context: &Context,
    catalog: &Catalog,
    service: &mut S,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    if let Command::Loadout = command {
        // Printed in its own format.
        let printed = companion::loadout(globals.json, globals.format, out);
        return finish(printed, globals.fallback(context), catalog, err);
    }
    let format = match globals.format(context, catalog, err) {
        Ok(format) => format,
        Err(code) => return code,
    };
    let ctx = commands::Ctx {
        project: globals.project,
        format,
        catalog,
        session: &context.session,
    };
    let result = match command {
        Command::Add(args) => commands::add(args, ctx, service, out, err).await,
        Command::Status(args) => commands::status(args, ctx, service, out, err).await,
        Command::Edit(args) => commands::edit(args, ctx, service, out, err).await,
        Command::Show(args) => commands::show(args, ctx, service, out, err).await,
        Command::Lint(args) => commands::lint(args, ctx, service, out, err).await,
        Command::Summary(args) => commands::summary(args, ctx, service, out, err).await,
        Command::Statuses(args) => commands::statuses(args, ctx, service, out, err).await,
        Command::Tag(args) => commands::tag(args, ctx, service, out, err).await,
        Command::Tags(args) => commands::tags(args, ctx, service, out, err).await,
        Command::List(args) => commands::list(args, ctx, service, out, err).await,
        Command::Prune(args) => commands::prune(args, ctx, service, out, err).await,
        Command::Permit(args) => commands::permit(args, ctx, service, out).await,
        Command::Revoke(args) => commands::revoke(args, ctx, service, out).await,
        Command::Permits => commands::permits(ctx, service, out).await,
        Command::Version => version(context, format, out),
        // Answered above: it has its own format.
        Command::Loadout => Ok(()),
    };
    finish(result, format, catalog, err)
}

/// The exit code of `result`, its error reported in `format`, after any
/// dead letters the use case that failed carries.
pub(crate) fn finish(
    result: Result<(), CliError>,
    format: Format,
    catalog: &Catalog,
    err: &mut dyn Write,
) -> u8 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            // A failed use case's dead letters are said before its error.
            if let CliError::App(failed) = &error {
                let _ = dead_lettered(failed.dead_letters(), catalog, err);
            }
            let _ = report(&error, format, catalog, err);
            1
        }
    }
}

/// `error` on stderr: its envelope in JSON; in text its catalog message
/// after the program's name, or a usage error as clap words it.
fn report(
    error: &CliError,
    format: Format,
    catalog: &Catalog,
    err: &mut dyn Write,
) -> io::Result<()> {
    match (format, error) {
        (Format::Json, _) => presentation::write_json(err, &catalog.envelope(error)),
        (Format::Text, CliError::Usage { .. }) => write!(err, "{}", catalog.message(error)),
        (Format::Text, _) => writeln!(err, "{PROG}: {}", catalog.message(error)),
    }
}

/// `notice` on stderr, after the program's name.
pub(crate) fn notify(
    notice: &dyn Diagnostic,
    catalog: &Catalog,
    err: &mut dyn Write,
) -> io::Result<()> {
    writeln!(err, "{PROG}: {}", catalog.message(notice))
}

/// Say each dead letter annals' kernel wrote on stderr: almost certainly
/// a bug, so never left unsaid.
pub(crate) fn dead_lettered(
    letters: &[DeadLetter],
    catalog: &Catalog,
    err: &mut dyn Write,
) -> io::Result<()> {
    letters
        .iter()
        .try_for_each(|letter| notify(letter, catalog, err))
}

/// Name the project on stderr, and any dead letter, then render the task.
pub(crate) fn wrote(
    written: &Written,
    view: &impl Render,
    format: Format,
    catalog: &Catalog,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    notify(&Notice::project(&written.project), catalog, err)
        .and_then(|()| dead_lettered(&written.dead_letters, catalog, err))
        .and_then(|()| presentation::render(out, format, view))
        .map_err(output_failed)
}

/// The version, as ctxloom's boot probe reads it.
fn version(context: &Context, format: Format, out: &mut dyn Write) -> Result<(), CliError> {
    let version = VersionJson {
        name: PROG,
        version: &context.version,
        lock_protocol: context.lock_protocol,
    };
    presentation::render(out, format, &version).map_err(output_failed)
}

pub(crate) fn output_failed(error: io::Error) -> CliError {
    CliError::Output {
        detail: error.to_string(),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
