//! The tasks command line: the verbs of `tasks` (`add`, `list`, `show`,
//! `set`, `tag add|remove`, `summary`, `prune`, `check`), of `manage
//! schema` (`show`, `export`, `check`, `declare`, `permits`, `tags`,
//! `statuses`) and `manage migrate schema`, translated into the
//! application's use cases and their answers rendered through presentation
//! ([`Tasks`], [`Schema`], [`Command`], [`run`]); `manage migrate tasks`, a
//! group of its own ([`ImportGroup`], [`run_import`]), so only it reaches
//! the importer; and the companion contract ctxloom reads, `manage version`
//! and `loadout`. A binary owns the parser: it flattens [`Globals`] and
//! mounts these into its own, beside whatever else it carries, and
//! [`parse`] answers help and usage errors as the task commands do, and
//! refuses the removed spellings ([`removed`]). Text on a terminal, JSON
//! otherwise, unless `--format` says; stdout holds the answer alone (one
//! JSON document), and notices, such as the project a write went to, go to
//! stderr. Exit 0; 1 when the command ran against state and the answer was
//! negative (no such task, a `check` finding), with the error on stderr:
//! its catalog message in text, its envelope in JSON; 2 when the request
//! is invalid on its own content (bad arguments, a tag no permit admits,
//! a removed command).

mod commands;
mod companion;
mod error;
mod import;
mod notice;
mod prune;
mod render;

use std::ffi::OsString;
use std::io::{self, Write};

use angzarr_diagnostic::Diagnostic;
use angzarr_presentation::{Catalog, CatalogError, Format, Render, Source, UnknownFormat};
use clap::{Args, Parser, Subcommand};
use plans_application::Governing;
use tasks_application::{
    dead_letter_line, DeadLetter, PermitService, PruneService, RuleWarning, SchemaService,
    TaskService, Written,
};

pub use companion::{Asked, LOADOUT};
#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{CliError, EN_US};
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
    sources.extend(plans_application::CATALOGS);
    sources.extend_from_slice(adapters);
    Catalog::en_us(&sources)
}

/// The global flags, every command's: a binary flattens them into its
/// parser.
#[derive(Args, Clone, PartialEq, Eq, Debug, Default)]
pub struct Globals {
    /// Output format: text, or json (the default when stdout is not a
    /// terminal).
    #[arg(long, global = true)]
    pub format: Option<Asked>,
    /// Project id to act on (overrides CTXLOOM_PROJECT_ID and the working directory's).
    #[arg(long, global = true)]
    pub project: Option<String>,
    /// Override a configuration key for this run (repeatable).
    #[arg(long = "config-set", global = true, value_name = "PATH=VALUE")]
    pub config_set: Vec<String>,
}

/// The verbs of `tasks`.
#[derive(Subcommand, Debug)]
pub enum Tasks {
    /// Add a task
    #[command(after_help = "Examples:\n  taskloom tasks add 'Fix the parser' --tag area:cli")]
    Add(commands::AddArgs),
    /// List active tasks; --all adds the finished and Deferred
    #[command(
        after_help = "Examples:\n  taskloom tasks list --term parser\n  taskloom tasks list --all --tag-query human:decision\n\nTasks keep no plan link. To find the plans governing a task, run `taskloom plans\nlist --governs <harp>`; to find the tasks a plan governs, `taskloom plans show\n<plan>` lists its `governs:` harps, then `taskloom tasks show <harp>...`."
    )]
    List(commands::ListArgs),
    /// Show tasks whole
    #[command(after_help = "Examples:\n  taskloom tasks show swift-amber-falcon")]
    Show(commands::ShowArgs),
    /// Change a task's text, status or trigger
    #[command(
        after_help = "Examples:\n  taskloom tasks set swift-amber-falcon --status Done\n  taskloom tasks set swift-amber-falcon --text 'New subject'"
    )]
    Set(commands::SetArgs),
    /// Add or remove a task's tags
    #[command(subcommand)]
    Tag(TagCommand),
    /// Count the tasks in each status
    #[command(after_help = "Examples:\n  taskloom tasks summary")]
    Summary,
    /// Remove old finished tasks (a preview unless --yes)
    #[command(
        after_help = "Examples:\n  taskloom tasks prune --older-than 30d\n  taskloom tasks prune --older-than 30d --yes"
    )]
    Prune(commands::PruneArgs),
    /// Check every task's tags against the store's rules; exit 1 on a finding
    #[command(after_help = "Examples:\n  taskloom tasks check")]
    Check,
}

/// The verbs of `tasks tag`.
#[derive(Subcommand, Debug)]
pub enum TagCommand {
    /// Add a tag to a task
    #[command(after_help = "Examples:\n  taskloom tasks tag add swift-amber-falcon area:cli")]
    Add(commands::TagArgs),
    /// Remove a tag from a task
    #[command(after_help = "Examples:\n  taskloom tasks tag remove swift-amber-falcon area:cli")]
    Remove(commands::TagArgs),
}

/// The vocabulary groups of `manage schema`.
#[derive(Subcommand, Debug)]
pub enum Vocabulary {
    /// The tag permits: what the store admits
    #[command(subcommand)]
    Permits(PermitsCommand),
    /// The tags in use
    #[command(subcommand)]
    Tags(TagsCommand),
    /// The statuses
    #[command(subcommand)]
    Statuses(StatusesCommand),
}

/// The verbs of `manage schema permits`.
#[derive(Subcommand, Debug)]
pub enum PermitsCommand {
    /// List what the store permits
    #[command(after_help = "Examples:\n  taskloom manage schema permits list")]
    List,
    /// Permit a tag, a key or a namespace (a person's command)
    #[command(after_help = "Examples:\n  taskloom manage schema permits add area:cli")]
    Add(commands::PermitArgs),
    /// Remove a permit (a preview unless --yes; a person's command)
    #[command(
        after_help = "Examples:\n  taskloom manage schema permits remove area:cli\n  taskloom manage schema permits remove area:cli --yes"
    )]
    Remove(commands::RemovePermitArgs),
}

/// The verbs of `manage schema tags`.
#[derive(Subcommand, Debug)]
pub enum TagsCommand {
    /// List the tags in use, with how many tasks hold each
    #[command(after_help = "Examples:\n  taskloom manage schema tags list --status @open")]
    List(commands::TagsArgs),
}

/// The verbs of `manage schema statuses`.
#[derive(Subcommand, Debug)]
pub enum StatusesCommand {
    /// List the statuses, in display order
    #[command(after_help = "Examples:\n  taskloom manage schema statuses list")]
    List,
}

/// The commands of `manage schema`: [`commands::SchemaCommand`] and the
/// [`Vocabulary`] groups, as one set to mount.
#[derive(Subcommand, Debug)]
pub enum Schema {
    #[command(flatten)]
    Rules(commands::SchemaCommand),
    #[command(flatten)]
    Vocabulary(Vocabulary),
}

/// What [`run`] runs: the commands the task use cases answer.
#[derive(Debug)]
pub enum Command {
    Tasks(Tasks),
    Schema(Schema),
    /// `manage migrate schema`.
    ImportConfig,
    /// `manage version`.
    Version,
    /// `loadout`.
    Loadout,
    /// `manage hook stamp-plan`: only its place is built.
    StampPlan,
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
        Ok(Format::choose(false, asked, context.stdout_is_terminal))
    }

    /// The format an error is reported in when `--format` gave none: JSON
    /// off a terminal.
    fn fallback(&self, context: &Context) -> Format {
        Format::choose(false, None, context.stdout_is_terminal)
    }
}

/// The format `args` ask for, read without parsing them: for a usage
/// error, where clap parsed nothing.
fn asked_format(args: &[OsString], context: &Context) -> Format {
    let mut asked = None;
    let mut words = args.iter().skip(1).map(|a| a.to_str().unwrap_or_default());
    while let Some(word) = words.next() {
        let value = match word {
            "--" => break,
            "--format" => words.next(),
            _ => word.strip_prefix("--format="),
        };
        asked = value.and_then(|v| v.parse().ok()).or(asked);
    }
    Format::choose(false, asked, context.stdout_is_terminal)
}

/// The command line `args` (the program name first) parsed as `P`, or the
/// exit code of answering it without a command: help (asked for, or a
/// group given no command) and `--version` (or `-v`) on `out`, exit 0; a
/// usage error, or a removed spelling ([`removed`]), on `err`, exit 2 (in
/// the format the words ask for, since clap parsed nothing).
/// `--version` prints Go's cobra line, `<prog> version <version>`, the
/// version `manage version` reports, in text whatever the format: the VS
/// Code extension's probe reads the word after `version` on its first line.
pub fn parse<P: Parser>(
    args: impl IntoIterator<Item = OsString>,
    context: &Context,
    catalog: &Catalog,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<P, u8> {
    let args: Vec<OsString> = args.into_iter().collect();
    if let Some(refusal) = removed(&args) {
        let _ = report(&refusal, asked_format(&args, context), catalog, err);
        return Err(refusal.exit_code());
    }
    let command = without_help_command(
        P::command()
            .version(context.version.clone())
            .disable_version_flag(true)
            .arg(
                clap::Arg::new("version")
                    .short('v')
                    .long("version")
                    .help("Print the taskloom version")
                    .action(clap::ArgAction::Version),
            ),
    );
    let parsed = command
        .try_get_matches_from(&args)
        .and_then(|mut matches| P::from_arg_matches_mut(&mut matches));
    parsed.map_err(|e| {
        use clap::error::ErrorKind;
        match e.kind() {
            ErrorKind::DisplayVersion => {
                let _ = writeln!(out, "{PROG} version {}", context.version);
                return 0;
            }
            // A group given no command prints its help, as `--help` does.
            ErrorKind::DisplayHelp | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
                let _ = write!(out, "{}", e.render());
                return 0;
            }
            _ => {}
        }
        let usage = CliError::Usage {
            detail: e.render().to_string(),
        };
        let _ = report(&usage, asked_format(&args, context), catalog, err);
        usage.exit_code()
    })
}

/// `command` and every command under it with no `help` subcommand: help
/// is `-h` and `--help`, and a group given no command prints its own.
fn without_help_command(command: clap::Command) -> clap::Command {
    let names: Vec<String> = command
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect();
    let mut command = command;
    if !names.is_empty() {
        command = command.disable_help_subcommand(true);
    }
    for name in names {
        command = command.mut_subcommand(name, without_help_command);
    }
    command
}

/// The global flags that take a value, as a word.
const VALUED_FLAGS: [&str; 3] = ["--format", "--project", "--config-set"];

/// What replaced the spelling `args` use, if it is one that was removed:
/// the old verbs (`add`, `status`, `plan`, ...), and the `--json` flag. A
/// removed spelling is refused whole, before anything is read: no alias
/// (ruling 4), a message saying what to use.
pub fn removed(args: &[OsString]) -> Option<CliError> {
    let instead = |instead: &str| CliError::Removed {
        instead: instead.to_string(),
    };
    let words: Vec<&str> = args
        .iter()
        .skip(1)
        .map(|a| a.to_str().unwrap_or_default())
        .take_while(|w| *w != "--")
        .collect();
    if words.contains(&"--json") {
        return Some(instead("--format json"));
    }
    if words.contains(&"--global") {
        return Some(instead("--project <id> per project"));
    }
    let mut rest = words.iter().copied();
    let mut positional = Vec::new();
    while let Some(word) = rest.next() {
        if VALUED_FLAGS.contains(&word) {
            rest.next();
        } else if !word.starts_with('-') {
            positional.push(word);
        }
        if positional.len() == 2 {
            break;
        }
    }
    let (first, second) = (positional.first()?, positional.get(1).copied());
    let replaced = match (*first, second) {
        ("add", _) => "tasks add",
        ("list", _) => "tasks list",
        ("show", _) => "tasks show",
        ("status" | "edit", _) => "tasks set",
        ("tag", _) => "tasks tag",
        ("tags", _) => "manage schema tags list",
        ("summary", _) => "tasks summary",
        ("statuses", _) => "manage schema statuses list",
        ("lint", _) => "tasks check",
        ("prune", _) => "tasks prune",
        ("permit", _) => "manage schema permits add",
        ("revoke", _) => "manage schema permits remove",
        ("permits", _) => "manage schema permits list",
        ("schema", Some("check")) => "manage schema check",
        ("schema", Some("declare")) => "manage schema declare",
        ("schema", Some("import-config")) => "manage migrate schema",
        ("schema", _) => "manage schema",
        ("import", _) => "manage migrate tasks",
        ("project", Some("heal")) => "manage doctor heal",
        ("project", Some("repoint")) => "manage doctor repoint",
        ("project", Some("orphans")) => "manage doctor orphans",
        ("project", _) => "manage project show",
        ("plan", Some("import")) => return Some(CliError::PlanImportRemoved),
        ("plan", Some(verb)) if verb.chars().all(|c| c.is_ascii_lowercase()) => {
            return Some(instead(&format!("plans {verb}")))
        }
        ("plan", _) => "plans",
        ("watch", _) => "tasks watch",
        ("version", _) => "manage version",
        _ => return None,
    };
    Some(instead(replaced))
}

/// The use cases a task command runs on: the tasks', and the plans'
/// query naming the plans governing each task `show` shows (the tasks know
/// no plan).
pub struct UseCases<'a, S, P> {
    pub tasks: &'a mut S,
    pub plans: &'a mut P,
}

/// Run `command` under the global flags against `use_cases`, writing to
/// `out` and `err` with messages from `catalog`; the exit code.
pub async fn run<S, P>(
    command: Command,
    globals: Globals,
    context: &Context,
    catalog: &Catalog,
    use_cases: UseCases<'_, S, P>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8
where
    S: TaskService + PruneService + PermitService + SchemaService,
    P: Governing,
{
    let UseCases {
        tasks: service,
        plans,
    } = use_cases;
    if let Command::Loadout = command {
        // Printed in its own format.
        let printed = companion::loadout(globals.format, out);
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
        Command::Tasks(tasks) => match tasks {
            Tasks::Add(args) => commands::add(args, ctx, service, out, err).await,
            Tasks::Set(args) => commands::set(args, ctx, service, out, err).await,
            Tasks::Show(args) => commands::show(args, ctx, service, plans, out, err).await,
            Tasks::Check => commands::check(ctx, service, out, err).await,
            Tasks::Summary => commands::summary(ctx, service, out, err).await,
            Tasks::Tag(TagCommand::Add(args)) => {
                commands::tag(args, true, ctx, service, out, err).await
            }
            Tasks::Tag(TagCommand::Remove(args)) => {
                commands::tag(args, false, ctx, service, out, err).await
            }
            Tasks::List(args) => commands::list(args, ctx, service, out, err).await,
            Tasks::Prune(args) => commands::prune(args, ctx, service, out, err).await,
        },
        Command::Schema(Schema::Rules(command)) => {
            commands::schema(command, ctx, service, out, err).await
        }
        Command::Schema(Schema::Vocabulary(vocabulary)) => match vocabulary {
            Vocabulary::Permits(PermitsCommand::List) => commands::permits(ctx, service, out).await,
            Vocabulary::Permits(PermitsCommand::Add(args)) => {
                commands::permit(args, ctx, service, out).await
            }
            Vocabulary::Permits(PermitsCommand::Remove(args)) => {
                commands::revoke(args, ctx, service, out, err).await
            }
            Vocabulary::Tags(TagsCommand::List(args)) => {
                commands::tags(args, ctx, service, out, err).await
            }
            Vocabulary::Statuses(StatusesCommand::List) => {
                commands::statuses(ctx, service, out, err).await
            }
        },
        Command::ImportConfig => commands::import_config(ctx, service, out).await,
        Command::Version => version(context, format, out),
        Command::StampPlan => Err(CliError::NotBuilt {
            command: "manage hook stamp-plan",
        }),
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
                let _ = dead_lettered(failed.dead_letters(), err);
            }
            let _ = report(&error, format, catalog, err);
            error.exit_code()
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
        (Format::Json, _) => angzarr_presentation::write_json(err, &catalog.envelope(error)),
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

/// Say each dead letter the kernel wrote on stderr, as its line
/// ([`dead_letter_line`]: keys and values, never English): almost
/// certainly a bug, so never left unsaid.
pub(crate) fn dead_lettered(letters: &[DeadLetter], err: &mut dyn Write) -> io::Result<()> {
    letters
        .iter()
        .try_for_each(|letter| writeln!(err, "{PROG}: {}", dead_letter_line(&letter.letter)))
}

/// Name the project on stderr, and any dead letter, then render the task.
/// What the store's rules found already wrong with it (tagma's `Existing`
/// findings, which never refuse a write) is said too: on stderr, one a
/// line, as every notice is; and in JSON in the document as well, as
/// `warnings`, tagma's §13 shape.
pub(crate) fn wrote(
    written: &Written,
    view: &impl Render,
    format: Format,
    catalog: &Catalog,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    notify(&Notice::project(&written.project), catalog, err)
        .and_then(|()| dead_lettered(&written.dead_letters, err))
        .and_then(|()| {
            written
                .warnings()
                .iter()
                .try_for_each(|w| notify(&RuleWarning(w.clone()), catalog, err))
        })
        .and_then(|()| match (format, written.warnings().is_empty()) {
            (Format::Json, false) => {
                let doc = Warned {
                    doc: view.json(),
                    warnings: tasks_presentation::FindingJson::all(written.warnings()),
                };
                angzarr_presentation::write_json(out, &doc)
            }
            _ => angzarr_presentation::render(out, format, view),
        })
        .map_err(output_failed)
}

/// A written task's document with the warnings beside its fields.
#[derive(serde::Serialize)]
struct Warned<T: serde::Serialize> {
    #[serde(flatten)]
    doc: T,
    warnings: Vec<tasks_presentation::FindingJson>,
}

/// The version, as ctxloom's boot probe reads it.
fn version(context: &Context, format: Format, out: &mut dyn Write) -> Result<(), CliError> {
    let version = VersionJson {
        name: PROG,
        version: &context.version,
        lock_protocol: context.lock_protocol,
    };
    angzarr_presentation::render(out, format, &version).map_err(output_failed)
}

pub(crate) fn output_failed(error: io::Error) -> CliError {
    CliError::Output {
        detail: error.to_string(),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
