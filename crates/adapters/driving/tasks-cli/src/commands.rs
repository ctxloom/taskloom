//! Each task command: its arguments, the use case they become, and how
//! its answer and notices are written.

use std::io::{self, Write};

use angzarr_presentation::{Catalog, Format};
use clap::Args;
use plans_application::Governing;
use tasks_application::{
    Declaration, ListQuery, Listing, NewTask, PermitChange, PermitService, PruneRequest,
    PruneService, Reach, SchemaService, Scope, ShowQuery, TagChange, TaskService, TaskSet,
    DEFAULT_OLDER_THAN, DEFAULT_PRUNE_STATUSES,
};

use crate::prune::{apply_command, PruneReport};
use crate::render::{
    Added, Changed, Checked, Counted, DeclaredView, Detailed, Exported, Linted, Listed, Permits,
    Previewed, Schema, Summed, Tagged, Taxonomy, YesIgnored,
};
use crate::{dead_lettered, notify, output_failed, wrote, CliError, Notice, PROG};

/// What every task command is handed besides its own arguments: the
/// global flags, the format, the catalog and the session.
pub(crate) struct Ctx<'a> {
    pub project: Option<String>,
    pub format: Format,
    pub catalog: &'a Catalog,
    pub session: &'a str,
}

impl Ctx<'_> {
    /// The session a task is added in: none where CTXLOOM_SESSION_HARP is
    /// unset, or set empty, as an empty variable of the environment is
    /// unset.
    fn session(&self) -> Option<String> {
        Some(self.session)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }
}

#[derive(Args, Debug)]
pub struct AddArgs {
    /// The task's text; several words are joined with spaces.
    #[arg(required = true, num_args = 1..)]
    text: Vec<String>,
    /// Initial status (default: "To Do").
    #[arg(long)]
    status: Option<String>,
    /// Revive condition: the task is added Deferred, returning to "To Do"
    /// when the trigger is cleared (required when --status Deferred;
    /// refused with any other status).
    #[arg(long)]
    trigger: Option<String>,
    /// Tag to set at creation (repeatable).
    #[arg(long = "tag")]
    tags: Vec<String>,
    /// An id you choose for this request; resend the same one when retrying
    /// it, and a request already applied is not applied again: the task's
    /// id derives from it, so a retry finds the task it added.
    #[arg(long)]
    request_id: Option<String>,
}

pub(crate) async fn add(
    args: AddArgs,
    ctx: Ctx<'_>,
    service: &mut impl TaskService,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let origin_session = ctx.session();
    let task = NewTask {
        project: ctx.project,
        text: args.text.join(" "),
        status: args.status,
        trigger: args.trigger,
        tags: args.tags,
        origin_session,
        request_id: args.request_id,
    };
    let written = service.add(task).await.map_err(CliError::App)?;
    wrote(
        &written,
        &Added(&written.task),
        ctx.format,
        ctx.catalog,
        out,
        err,
    )
}

#[derive(Args, Debug)]
#[command(group(
    clap::ArgGroup::new("change")
        .args(["text", "status", "trigger", "clear_trigger"])
        .required(true)
        .multiple(true)
))]
pub struct SetArgs {
    /// The task's id.
    harp_id: String,
    /// The new text, whole.
    #[arg(long)]
    text: Option<String>,
    /// The new status (see `taskloom manage schema statuses list`).
    #[arg(long)]
    status: Option<String>,
    /// Revive condition: defers the task, which keeps the status it held.
    #[arg(long)]
    trigger: Option<String>,
    /// Clear the trigger: a Deferred task returns to the status it held.
    #[arg(long)]
    clear_trigger: bool,
    /// An id you choose for this request; resend the same one when retrying
    /// it, and a request already applied is not applied again.
    #[arg(long)]
    request_id: Option<String>,
}

pub(crate) async fn set(
    args: SetArgs,
    ctx: Ctx<'_>,
    service: &mut impl TaskService,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let change = TaskSet {
        project: ctx.project,
        harp_id: args.harp_id,
        text: args.text,
        status: args.status,
        trigger: args.trigger,
        clear_trigger: args.clear_trigger,
        request_id: args.request_id,
    };
    let written = tasks_application::set_task(service, change)
        .await
        .map_err(CliError::App)?;
    wrote(
        &written,
        &Added(&written.task),
        ctx.format,
        ctx.catalog,
        out,
        err,
    )
}

#[derive(Args, Debug)]
pub struct TagArgs {
    /// The task's id.
    harp_id: String,
    /// The tag.
    tag: String,
    /// An id you choose for this request; resend the same one when retrying
    /// it, and a request already applied is not applied again.
    #[arg(long)]
    request_id: Option<String>,
}

/// Add (`adding`) or remove `args.tag` on the task.
pub(crate) async fn tag(
    args: TagArgs,
    adding: bool,
    ctx: Ctx<'_>,
    service: &mut impl TaskService,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let (add, remove) = match adding {
        true => (vec![args.tag], Vec::new()),
        false => (Vec::new(), vec![args.tag]),
    };
    let change = TagChange {
        project: ctx.project,
        harp_id: args.harp_id,
        add,
        remove,
        request_id: args.request_id,
    };
    let written = service.tag(change).await.map_err(CliError::App)?;
    let view = Tagged(&written.task, &written.shown);
    wrote(&written, &view, ctx.format, ctx.catalog, out, err)
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Only tasks in this status (repeatable, or comma-separated); the
    /// classes @open (every status not terminal) and @terminal (Done,
    /// Archived) mix with statuses.
    #[arg(long, value_delimiter = ',')]
    status: Vec<String>,
    /// Only tasks whose text holds this, in any case.
    #[arg(long)]
    term: Option<String>,
    /// Include the tasks hidden by default: Done, Archived and Deferred.
    #[arg(long)]
    all: bool,
    /// The order: priority, highest first, each project's tasks ranked
    /// against all of its own; the default is the order added (across
    /// projects, by project and then id).
    #[arg(long)]
    sort: Option<String>,
    /// In JSON, each task as its id, status, checked, tags and headline;
    /// text is one line per task already.
    #[arg(long)]
    compact: bool,
    /// At most this many tasks (leave it out for no limit; 0 is refused);
    /// how many more there were is said on stderr.
    #[arg(long)]
    limit: Option<usize>,
    /// Only the tasks this tag query selects: postfix, tags first and the
    /// operator after (human:decision/area:cli/and, human:decision/not).
    /// Operators: and, or, not; atoms [ns:]key[op value]
    /// with op one of = != > >= < <= ~, and * (any) or + (present) for a
    /// namespace, key or value.
    #[arg(long = "tag-query")]
    tag_query: Option<String>,
}

pub(crate) async fn list(
    args: ListArgs,
    ctx: Ctx<'_>,
    service: &mut impl TaskService,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let query = ListQuery {
        project: ctx.project,
        statuses: args.status,
        all: args.all,
        tag_query: args.tag_query,
        term: args.term,
        limit: args.limit,
        sort: args.sort,
    };
    let listing = service.list(query).await.map_err(CliError::App)?;
    listing_notices(&listing, ctx.catalog, err)
        .and_then(|()| {
            let view = Listed {
                listing: &listing,
                compact: args.compact,
            };
            angzarr_presentation::render(out, ctx.format, &view)
        })
        .map_err(output_failed)
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// The tasks' ids: every one must be found.
    #[arg(required = true, num_args = 1..)]
    harp_ids: Vec<String>,
}

/// The tasks `args` names, and the plans governing each: the tasks'
/// `show`, then the plans' `governing` of the tasks it found, each in the
/// project it was found in.
pub(crate) async fn show(
    args: ShowArgs,
    ctx: Ctx<'_>,
    service: &mut impl TaskService,
    plans: &mut impl Governing,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let query = ShowQuery {
        project: ctx.project,
        harp_ids: args.harp_ids,
    };
    let details = service.show(query).await.map_err(CliError::App)?;
    let tasks: Vec<_> = details
        .rows
        .iter()
        .map(|row| (row.project.clone(), row.task.harp_id.clone()))
        .collect();
    let governing = plans.governing(&tasks).await.map_err(CliError::Plans)?;
    let view = Detailed {
        details: &details,
        governing: &governing,
    };
    read_in(&details.reach, ctx.catalog, err)
        .and_then(|()| angzarr_presentation::render(out, ctx.format, &view))
        .map_err(output_failed)
}

#[derive(Args, Debug)]
pub struct PermitArgs {
    /// The permit: `area:` (a namespace, any key), `area:cli` (a key of a
    /// namespace) or a flat tag (a key with no namespace); never a value.
    permit: String,
}

/// `permits remove`'s arguments: the permit, and whether to apply it.
#[derive(Args, Debug)]
pub struct RemovePermitArgs {
    /// The permit, spelled as `permits add` takes it.
    permit: String,
    #[command(flatten)]
    apply: ApplyArgs,
}

/// A change that previews unless told to apply.
#[derive(Args, Debug, Clone, Copy)]
pub struct ApplyArgs {
    /// Make the change; without it, only report what it would do.
    #[arg(long)]
    yes: bool,
    /// Report what it would do and change nothing (wins over --yes).
    #[arg(long)]
    dry_run: bool,
}

impl ApplyArgs {
    /// Whether the change is made: asked for, and not a dry run.
    pub fn applies(self) -> bool {
        self.yes && !self.dry_run
    }

    /// Whether `--yes` was given and `--dry-run` overruled it; `would` is
    /// then what the change would do without it.
    fn ignored(self, would: impl FnOnce() -> Vec<String>) -> Option<YesIgnored> {
        (self.yes && self.dry_run).then(|| YesIgnored {
            would_apply: would(),
        })
    }
}

/// Permit `args.permit` and say what changed: in text, `permitted <permit>`
/// or `<permit> was already permitted`; in JSON, `{"permit", "changed"}`.
pub(crate) async fn permit(
    args: PermitArgs,
    ctx: Ctx<'_>,
    service: &mut impl PermitService,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let change = PermitChange {
        permit: args.permit,
        dry_run: false,
    };
    let permitted = service.permit(change).await.map_err(CliError::App)?;
    let view = Changed {
        permitted: &permitted,
        revoked: false,
    };
    angzarr_presentation::render(out, ctx.format, &view).map_err(output_failed)
}

/// Remove `args.permit`, or report that it would be: in text, `revoked
/// <permit>`; in JSON, `{"permit", "changed"}`.
pub(crate) async fn revoke(
    args: RemovePermitArgs,
    ctx: Ctx<'_>,
    service: &mut impl PermitService,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let applies = args.apply.applies();
    let ignored = args
        .apply
        .ignored(|| vec![format!("remove the permit {}", args.permit)]);
    let command = format!("{PROG} manage schema permits remove {} --yes", args.permit);
    let change = PermitChange {
        permit: args.permit,
        dry_run: !applies,
    };
    let permitted = service.revoke(change).await.map_err(CliError::App)?;
    let view = Changed {
        permitted: &permitted,
        revoked: true,
    };
    angzarr_presentation::render(out, ctx.format, &Previewed(&view, !applies, ignored))
        .map_err(output_failed)?;
    previewed(applies, command, ctx.catalog, err)
}

/// After a preview, say on stderr that nothing changed and what applies it.
fn previewed(
    applies: bool,
    command: String,
    catalog: &Catalog,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    match applies {
        true => Ok(()),
        false => notify(&Notice::Preview { command }, catalog, err).map_err(output_failed),
    }
}

/// List what the project permits: in text, its namespaces (open, or
/// closed with their keys), its flat tags and the schema's targets; in
/// JSON, [`tasks_presentation::PermitsJson`].
pub(crate) async fn permits(
    ctx: Ctx<'_>,
    service: &mut impl TaskService,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let list = service.permits().await.map_err(CliError::App)?;
    angzarr_presentation::render(out, ctx.format, &Permits(&list)).map_err(output_failed)
}

/// What `manage schema` does besides its vocabulary groups.
#[derive(clap::Subcommand, Debug)]
pub enum SchemaCommand {
    /// Print the store's tag rules a write is held to
    #[command(after_help = "Examples:\n  taskloom manage schema show")]
    Show,
    /// Print the store's tag rules as tagma config tags
    #[command(after_help = "Examples:\n  taskloom manage schema export")]
    Export,
    /// Check the store's tag rules; exit 1 on any finding
    #[command(after_help = "Examples:\n  taskloom manage schema check")]
    Check,
    /// Add and remove the store's tag rules (a person's command)
    #[command(
        after_help = "Examples:\n  taskloom manage schema declare --add 'tagma.permit:\"area:*\"' --yes"
    )]
    Declare(DeclareArgs),
}

#[derive(Args, Debug)]
pub struct DeclareArgs {
    /// A tag rule to add, in tagma's grammar (repeatable).
    #[arg(long = "add")]
    add: Vec<String>,
    /// A tag rule to remove, in tagma's grammar (repeatable).
    #[arg(long = "remove")]
    remove: Vec<String>,
    #[command(flatten)]
    apply: ApplyArgs,
}

/// Run a `manage schema` command: `show` the store's rules (text one line a
/// target with its facets, its formulas beneath; JSON
/// [`tasks_presentation::SchemaJson`]), `export` them, `check` them
/// (failing on a finding) or `declare` a change to them.
pub(crate) async fn schema(
    command: SchemaCommand,
    ctx: Ctx<'_>,
    service: &mut (impl TaskService + SchemaService + PermitService),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let format = ctx.format;
    match command {
        SchemaCommand::Show => {
            let schema = service.tag_schema().await.map_err(CliError::App)?;
            angzarr_presentation::render(out, format, &Schema(&schema)).map_err(output_failed)
        }
        // Printed in its own format, tagma's syntax, whatever is asked,
        // as `loadout` is in yaml: it is what tagma reads back.
        SchemaCommand::Export => {
            let tags = service
                .schema_export(ctx.project)
                .await
                .map_err(CliError::App)?;
            angzarr_presentation::render(out, Format::Text, &Exported(&tags)).map_err(output_failed)
        }
        SchemaCommand::Check => {
            let findings = service
                .schema_check(ctx.project)
                .await
                .map_err(CliError::App)?;
            angzarr_presentation::render(out, format, &Checked(&findings))
                .map_err(output_failed)?;
            match findings.len() {
                0 => Ok(()),
                count => Err(CliError::SchemaFindings { count }),
            }
        }
        SchemaCommand::Declare(declare) => {
            let applies = declare.apply.applies();
            let ignored = declare.apply.ignored(|| {
                let rules = |verb: &str, rules: &[String]| {
                    rules
                        .iter()
                        .map(|rule| format!("{verb} the rule {rule}"))
                        .collect::<Vec<_>>()
                };
                [rules("add", &declare.add), rules("remove", &declare.remove)].concat()
            });
            let command = declaration_command(&declare);
            let declaration = Declaration {
                add: declare.add,
                remove: declare.remove,
                dry_run: !applies,
            };
            let declared = service.declare(declaration).await.map_err(CliError::App)?;
            let view = DeclaredView(&declared);
            angzarr_presentation::render(out, format, &Previewed(&view, !applies, ignored))
                .map_err(output_failed)?;
            previewed(applies, command, ctx.catalog, err)
        }
    }
}

/// The command that applies exactly the declaration given.
fn declaration_command(declare: &DeclareArgs) -> String {
    let mut words = vec![PROG.to_string(), "manage schema declare".to_string()];
    for (flag, rules) in [("--add", &declare.add), ("--remove", &declare.remove)] {
        for rule in rules {
            words.push(flag.to_string());
            words.push(crate::prune::shell_word(rule));
        }
    }
    words.push("--yes".to_string());
    words.join(" ")
}

/// `manage migrate schema`: move the work root's config file's tag rules
/// into the project's store.
pub(crate) async fn import_config(
    ctx: Ctx<'_>,
    service: &mut impl PermitService,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let declared = service.import_config().await.map_err(CliError::App)?;
    angzarr_presentation::render(out, ctx.format, &DeclaredView(&declared)).map_err(output_failed)
}

/// The scope of a read that takes no filter: the work root's project, or
/// the one pinned.
fn scope(ctx: Ctx<'_>) -> Scope {
    Scope {
        project: ctx.project,
    }
}

pub(crate) async fn summary(
    ctx: Ctx<'_>,
    service: &mut impl TaskService,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let (format, catalog) = (ctx.format, ctx.catalog);
    let summary = service.summary(scope(ctx)).await.map_err(CliError::App)?;
    reached(&summary.reach, catalog, err)
        .and_then(|()| angzarr_presentation::render(out, format, &Summed(&summary)))
        .map_err(output_failed)
}

pub(crate) async fn statuses(
    ctx: Ctx<'_>,
    service: &mut impl TaskService,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let (format, catalog) = (ctx.format, ctx.catalog);
    let statuses = service.statuses(scope(ctx)).await.map_err(CliError::App)?;
    // With no project the defaults are still the answer: only a read of
    // every project for want of one is worth saying.
    let noticed = match statuses.reach {
        Reach::Every { .. } => reached(&statuses.reach, catalog, err),
        Reach::Project(_) | Reach::NoProject => Ok(()),
    };
    noticed
        .and_then(|()| angzarr_presentation::render(out, format, &Taxonomy(&statuses)))
        .map_err(output_failed)
}

#[derive(Args, Debug)]
pub struct TagsArgs {
    /// Count only the tasks in this status (repeatable, or
    /// comma-separated); @open and @terminal as list takes them.
    #[arg(long, value_delimiter = ',')]
    status: Vec<String>,
    /// Count only the tasks whose text holds this, in any case (it narrows
    /// the tasks, never the tags' names).
    #[arg(long)]
    term: Option<String>,
    /// Count only the tasks this tag query selects (see `list --help`).
    #[arg(long = "tag-query")]
    tag_query: Option<String>,
}

pub(crate) async fn tags(
    args: TagsArgs,
    ctx: Ctx<'_>,
    service: &mut impl TaskService,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let query = ListQuery {
        project: ctx.project,
        statuses: args.status,
        tag_query: args.tag_query,
        term: args.term,
        ..ListQuery::default()
    };
    let vocabulary = service.tags(query).await.map_err(CliError::App)?;
    reached(&vocabulary.reach, ctx.catalog, err)
        .and_then(|()| angzarr_presentation::render(out, ctx.format, &Counted(&vocabulary)))
        .map_err(output_failed)
}

/// Check the tasks' tags, then refuse when it found a violation or checked no target, so
/// that CI sees it in the exit code.
pub(crate) async fn check(
    ctx: Ctx<'_>,
    service: &mut impl TaskService,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let (format, catalog) = (ctx.format, ctx.catalog);
    let report = service.lint(scope(ctx)).await.map_err(CliError::App)?;
    let view = Linted {
        report: &report,
        catalog,
    };
    reached(&report.reach, catalog, err)
        .and_then(|()| angzarr_presentation::render(out, format, &view))
        .map_err(output_failed)?;
    if report.checked_targets == 0 {
        return Err(CliError::LintCheckedNothing);
    }
    match report.violations.len() {
        0 => Ok(()),
        count => Err(CliError::LintViolations { count }),
    }
}

/// `prune`'s flags.
#[derive(Args, Debug)]
pub struct PruneArgs {
    /// Only tasks whose last status change is older than this age (30d,
    /// 12w, 720h) or date (2026-01-01).
    #[arg(long = "older-than", default_value = DEFAULT_OLDER_THAN)]
    older_than: String,
    /// The statuses that qualify a task (repeatable, or comma-separated).
    #[arg(long, value_delimiter = ',', default_values = DEFAULT_PRUNE_STATUSES)]
    status: Vec<String>,
    /// Only tasks matching this tag query (see list --tag-query).
    #[arg(long = "tag-query")]
    tag_query: Option<String>,
    #[command(flatten)]
    apply: ApplyArgs,
}

/// Run the prune the flags ask for. Say on stderr when there is no
/// project here, then render the report, and after a report that would
/// take something, say nothing was taken and the command that takes it.
pub(crate) async fn prune(
    args: PruneArgs,
    ctx: Ctx<'_>,
    service: &mut impl PruneService,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let apply = apply_command(&args.older_than, &args.status, args.tag_query.as_deref());
    let apply_args = args.apply;
    let request = PruneRequest {
        project: ctx.project,
        statuses: args.status,
        older_than: args.older_than,
        tag_query: args.tag_query,
        apply: args.apply.applies(),
    };
    let (format, catalog) = (ctx.format, ctx.catalog);
    let pruning = service.prune(request).await.map_err(CliError::App)?;
    if pruning.project.is_none() {
        notify(&Notice::NothingToPrune, catalog, err).map_err(output_failed)?;
    }
    dead_lettered(&pruning.dead_letters, err).map_err(output_failed)?;
    let report = PruneReport(&pruning);
    let ignored = apply_args.ignored(|| {
        pruning
            .pruned
            .iter()
            .map(|p| {
                format!(
                    "remove and purge the task {} ({})",
                    p.task.harp_id, p.task.status
                )
            })
            .collect()
    });
    match ignored {
        Some(ignored) => {
            angzarr_presentation::render(out, format, &Previewed(&report, true, Some(ignored)))
        }
        None => angzarr_presentation::render(out, format, &report),
    }
    .map_err(output_failed)?;
    if pruning.applied || pruning.pruned.is_empty() {
        return Ok(());
    }
    let report_only = Notice::ReportOnly { command: apply };
    notify(&report_only, catalog, err).map_err(output_failed)
}

/// Say on stderr which project a read was in: the one here, by name, or
/// why it was not.
pub(crate) fn read_in(reach: &Reach, catalog: &Catalog, err: &mut dyn Write) -> io::Result<()> {
    match reach {
        Reach::Project(project) => notify(&Notice::project(project), catalog, err),
        Reach::NoProject | Reach::Every { .. } => reached(reach, catalog, err),
    }
}

/// Say on stderr where a read went when it is not the project here: no
/// project, or every project for want of one.
pub(crate) fn reached(reach: &Reach, catalog: &Catalog, err: &mut dyn Write) -> io::Result<()> {
    match reach {
        Reach::NoProject => notify(&Notice::NoProject, catalog, err),
        Reach::Every {
            fallback: Some(directory),
            ..
        } => notify(
            &Notice::EveryProject {
                directory: directory.clone(),
            },
            catalog,
            err,
        ),
        Reach::Project(_) | Reach::Every { fallback: None, .. } => Ok(()),
    }
}

/// Where the listing was read, if not here; why its ranking by priority
/// means little; what the default view hid of a search's matches; and
/// what the limit left out.
fn listing_notices(listing: &Listing, catalog: &Catalog, err: &mut dyn Write) -> io::Result<()> {
    reached(&listing.reach, catalog, err)?;
    for note in &listing.priority_notes {
        notify(note, catalog, err)?;
    }
    let hidden = listing.hidden;
    if listing.searched && hidden.total() > 0 {
        let notice = Notice::Hidden {
            hidden: hidden.total(),
            completed: hidden.completed,
            deferred: hidden.deferred,
        };
        notify(&notice, catalog, err)?;
    }
    if listing.omitted > 0 {
        let notice = Notice::Omitted {
            omitted: listing.omitted,
        };
        notify(&notice, catalog, err)?;
    }
    Ok(())
}
