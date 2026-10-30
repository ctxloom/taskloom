//! The tasks use cases: add a task, move, edit or retag one, read a
//! project's tasks (or every project's), watch them change, prune old
//! finished ones, read what the project's tag permits permit; and, apart
//! from them, import the JSONL task logs ([`Importer`], the one use case
//! that reads them), and grant and revoke tag permits ([`PermitService`],
//! which no MCP server is handed).
//!
//! Each resolves its project through [`ProjectIdentity`], opens the
//! project's store through [`TaskStores`], and runs its command through
//! angzarr-embedded's kernel ([`KERNEL_RETRY`]: taskloom's loop is the one
//! that retries); a list is the task-list projector caught up from the
//! store. The ports are the only way out: this crate does no I/O, keeps no
//! clock and starts no runtime.

mod dead_letter;
mod error;
mod import;
mod lint;
mod permit;
mod priority;
mod prune;
mod query;
mod read;
mod request;
mod retry;
mod schema;
mod semver;
mod tags;
mod view;
mod watch;

use std::future::Future;

use angzarr_embedded::next::v1::{command_page, Command, CommandBook, CommandPage, Cover, Uuid};
use angzarr_embedded::next::{
    ids, KernelError, Payload, ReadStore, ReadUnit, Registry, StoreError, Stream, StreamRef,
};
use angzarr_embedded_diagnostic::{Diagnostic, Report};
use dead_letter::DeadLetters;
use ports::{Clock, CommandIds, Configuration, Delay, Jitter, Names, ProjectIdentity, TaskStores};
use tasks::{
    AddTask, EditText, Retag, SetStatus, TaskAggregate, TaskCommand, TaskRejection, TaskState,
    TASK_DOMAIN,
};

pub use dead_letter::DeadLetter;
#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{AppError, CATALOGS, EN_US};
pub use import::{ImportReport, ImportRequest, ImportService, Importer, Lost, ProjectImport, Why};
pub use lint::{Finding, LintReport, Violation};
pub use permit::{PermitChange, PermitList, PermitService, Permitted};
pub use ports::Timestamp;
pub use ports::{NamespacePermit, Project, TagPermits};
pub use priority::{PriorityNote, PriorityWarning};
pub use prune::{
    Dangling, PruneRequest, PruneService, Pruned, Pruning, Via, DEFAULT_OLDER_THAN,
    DEFAULT_PRUNE_STATUSES,
};
pub use read::{Details, Hidden, Listing, Reach, Statuses, Summary, TagCount, Vocabulary};
pub use request::{request_command_id, REQUEST_NAMESPACE};
pub use retry::{Backoff, KERNEL_RETRY};
pub use schema::{Range, TagSchema};
pub use semver::{Semver, Version};
pub use tags::{parse as parse_tag, Tag};
pub use view::{text_hash, Row, Shown, StatusInfo, Task};

// Used across the crate's modules by these paths; nothing outside uses them.
pub(crate) use tags::TagRefusal;
pub(crate) use view::DONE;
pub use watch::{Watch, WatchRequest, WatchService};

/// How many task ids an add mints before giving up, each refused because a
/// task already holds it.
pub(crate) const HARP_ATTEMPTS: u32 = 100;

/// A task to add. A value given (`Some`) is never read as one left out:
/// given blank, it is refused ([`AppError::Blank`]).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct NewTask {
    /// An explicit project id, overriding resolution.
    pub project: Option<String>,
    pub text: String,
    /// `None` means To Do.
    pub status: Option<String>,
    /// Given, the task is added Deferred, returning to To Do when the
    /// trigger is cleared. Required when the status is Deferred; refused
    /// with any other.
    pub trigger: Option<String>,
    pub tags: Vec<String>,
    /// The session adding it, if one is.
    pub origin_session: Option<String>,
    /// The request's key: refused, given at all, until fix O pins the
    /// harp an add mints by it ([`AppError::AddRequestIdNotYet`]). An
    /// add's stream is its minted harp's, and the kernel deduplicates within a
    /// stream, so a retried add would mint another harp and add the task
    /// twice: the key would promise what it cannot keep.
    pub request_id: Option<String>,
}

/// Tags to add to and remove from a task: the adds first, then the
/// removes, so a tag named in both ends up removed. Each is a tag in
/// tagma's grammar; one that does not parse refuses the change, as does an
/// added one the tag schema refuses. An added value of a scalar target
/// displaces the value the task holds.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TagChange {
    pub project: Option<String>,
    pub harp_id: String,
    pub add: Vec<String>,
    pub remove: Vec<String>,
    /// The request's key: the id the caller gave the request, and gives
    /// again when it retries it (no edge mints one). The command id derives
    /// from it ([`request_command_id`]), so a retry is answered DUPLICATE
    /// and applies nothing, answering with the task as it stands. `None`, a
    /// fresh command id. Given blank, refused ([`AppError::Blank`]).
    pub request_id: Option<String>,
}

/// A task's status, its trigger, or both, changed. A trigger given defers
/// the task, keeping the status it held to return to; given with a
/// status, the status must be Deferred. `clear_trigger` clears the one
/// held, returning a Deferred task to that status (or to `status`, given).
/// A trigger left out (`None`) keeps the one held while the task stays
/// Deferred; moving to any other status clears it. A status or trigger
/// given blank is refused ([`AppError::Blank`]), and a trigger given with
/// `clear_trigger` ([`AppError::TriggerAndClear`]).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct StatusChange {
    pub project: Option<String>,
    pub harp_id: String,
    pub status: Option<String>,
    pub trigger: Option<String>,
    pub clear_trigger: bool,
    /// The request's key: the id the caller gave the request, and gives
    /// again when it retries it (no edge mints one). The command id derives
    /// from it ([`request_command_id`]), so a retry is answered DUPLICATE
    /// and applies nothing, answering with the task as it stands. `None`, a
    /// fresh command id. Given blank, refused ([`AppError::Blank`]).
    pub request_id: Option<String>,
}

/// A task's text replaced whole; its status and trigger untouched.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TextEdit {
    pub project: Option<String>,
    pub harp_id: String,
    pub text: String,
    /// The request's key: the id the caller gave the request, and gives
    /// again when it retries it (no edge mints one). The command id derives
    /// from it ([`request_command_id`]), so a retry is answered DUPLICATE
    /// and applies nothing, answering with the task as it stands. `None`, a
    /// fresh command id. Given blank, refused ([`AppError::Blank`]).
    pub request_id: Option<String>,
}

/// The tasks to show, by id, every one of which must be found: in the
/// work root's project, or with `global` in every project.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ShowQuery {
    pub project: Option<String>,
    pub global: bool,
    pub harp_ids: Vec<String>,
}

/// Where a read that takes no filter reads: the work root's project (or
/// the one pinned), or with `global` every project.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Scope {
    pub project: Option<String>,
    pub global: bool,
}

/// Which tasks to list. By default the active ones: neither terminal nor
/// Deferred. `all` lists every task; naming statuses (or the classes
/// `@open` and `@terminal`) lists exactly the tasks in them. A term keeps
/// those whose text mentions it, case-insensitively, and a tag query those
/// it selects. `global` reads every project. A term, tag query, sort or
/// limit given blank (a limit of 0) is refused ([`AppError::Blank`]),
/// never read as none.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ListQuery {
    pub project: Option<String>,
    pub statuses: Vec<String>,
    pub all: bool,
    /// A postfix tag query (see [`QUERY_GRAMMAR`]).
    pub tag_query: Option<String>,
    /// A substring of the text, any case.
    pub term: Option<String>,
    /// Every project's tasks, not the work root's project's.
    pub global: bool,
    /// At most this many tasks, the rest counted as omitted; `None` for
    /// no limit. A limit of 0 is given blank, and refused.
    pub limit: Option<usize>,
    /// The order: `None` for the listing's own, or [`PRIORITY`], the one
    /// sort.
    pub sort: Option<String>,
}

/// The sort by derived priority, `--sort priority`.
pub(crate) const PRIORITY: &str = "priority";

/// A task written, the project it was written in, which of its tags a
/// person is shown, and the dead letters the kernel wrote while the
/// write ran (almost certainly bugs, for the caller to say loudly).
#[derive(Clone, PartialEq, Debug)]
pub struct Written {
    pub project: Project,
    pub task: Task,
    pub shown: Shown,
    pub dead_letters: Vec<DeadLetter>,
}

/// The use cases, as a driving adapter calls them. Their futures are
/// `Send`, so a server can spawn them.
pub trait TaskService {
    fn add(&mut self, task: NewTask) -> impl Future<Output = Result<Written, AppError>> + Send;
    fn tag(&mut self, change: TagChange) -> impl Future<Output = Result<Written, AppError>> + Send;
    fn list(&mut self, query: ListQuery) -> impl Future<Output = Result<Listing, AppError>> + Send;
    fn set_status(
        &mut self,
        change: StatusChange,
    ) -> impl Future<Output = Result<Written, AppError>> + Send;
    fn edit(&mut self, edit: TextEdit) -> impl Future<Output = Result<Written, AppError>> + Send;
    fn show(&mut self, query: ShowQuery) -> impl Future<Output = Result<Details, AppError>> + Send;
    fn summary(&mut self, scope: Scope) -> impl Future<Output = Result<Summary, AppError>> + Send;
    fn statuses(&mut self, scope: Scope)
        -> impl Future<Output = Result<Statuses, AppError>> + Send;
    fn tags(
        &mut self,
        query: ListQuery,
    ) -> impl Future<Output = Result<Vocabulary, AppError>> + Send;
    fn lint(&mut self, scope: Scope) -> impl Future<Output = Result<LintReport, AppError>> + Send;
    /// The tag schema a write is held to: the default one, extended by
    /// what is configured. No project is resolved.
    fn tag_schema(&mut self) -> impl Future<Output = Result<TagSchema, AppError>> + Send;
    /// What the project's tag permits permit, read-only: its permits as
    /// configured (none, where it declares none), and the targets the
    /// resolved tag schema declares, which are permitted without one. No
    /// project is resolved and no store opened: permits are configuration.
    /// Granting and revoking are [`PermitService`]'s, which no MCP server
    /// is handed; reading them is here, so an agent can see them.
    fn permits(&mut self) -> impl Future<Output = Result<PermitList, AppError>> + Send;
}

/// The registry the tasks domain runs under: the task aggregate.
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.aggregate::<TaskAggregate>(TaskAggregate::default);
    registry
}

/// The use cases over their ports: `clock` stamps commands and mints their
/// ids; `delay` waits, with jitter, between retries; `configuration` is
/// what is configured, nothing until [`Service::with_configuration`] gives
/// it. It reads no JSONL task log: only an [`Importer`] does.
pub struct Service<I, S, N, C, D, G = NotConfigured> {
    pub identity: I,
    pub stores: S,
    pub names: N,
    pub clock: C,
    pub delay: D,
    pub configuration: G,
    pub backoff: Backoff,
    registry: Registry,
    /// The log the kernel writes dead letters to, taken into each
    /// result, or a write's error.
    pub(crate) dead_letters: DeadLetters,
}

/// Configuration, as a service without any has it: nothing configured, so
/// the tag schema is the default one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NotConfigured;

impl Configuration for NotConfigured {
    // Equivalent: `Ok(vec![])` is the empty list this returns.
    #[cfg_attr(test, mutants::skip)]
    fn tag_schema(&mut self) -> Result<Vec<String>, Report> {
        Ok(Vec::new())
    }
}

impl<I, S, N, C, D> Service<I, S, N, C, D> {
    pub fn new(identity: I, stores: S, names: N, clock: C, delay: D) -> Self {
        Service {
            identity,
            stores,
            names,
            clock,
            delay,
            configuration: NotConfigured,
            backoff: Backoff::DEFAULT,
            registry: registry(),
            dead_letters: DeadLetters::default(),
        }
    }
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G> {
    /// The same service, configured by `configuration`.
    pub fn with_configuration<H>(self, configuration: H) -> Service<I, S, N, C, D, H> {
        Service {
            identity: self.identity,
            stores: self.stores,
            names: self.names,
            clock: self.clock,
            delay: self.delay,
            configuration,
            backoff: self.backoff,
            registry: self.registry,
            dead_letters: self.dead_letters,
        }
    }
}

impl<I, S, N, C, D, G: Configuration> Service<I, S, N, C, D, G> {
    /// The tag schema: the default one, extended by what is configured.
    fn schema(&mut self) -> Result<TagSchema, AppError> {
        let configured = self
            .configuration
            .tag_schema()
            .map_err(AppError::Configuration)?;
        TagSchema::resolved(&configured).map_err(|e| AppError::Configuration(Report::of(&e)))
    }

    /// The project's tag permits as configured: `None` where it declares
    /// none.
    fn tag_permits(&mut self) -> Result<Option<TagPermits>, AppError> {
        self.configuration
            .tag_permits()
            .map_err(AppError::Configuration)
    }

    /// Whether the project's tag permits permit every tag of `add`, the
    /// tags a write adds: refused at the first they do not.
    fn check_permitted(&mut self, add: &[Tag], schema: &TagSchema) -> Result<(), AppError> {
        let permits = self.tag_permits()?;
        for tag in add {
            tags::permitted(tag, permits.as_ref(), schema).map_err(|r| refused(&r))?;
        }
        Ok(())
    }
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    I: ProjectIdentity,
    S: TaskStores,
    C: Clock + CommandIds,
    D: Delay + Jitter,
{
    /// The project of a change to an existing task, resolved as a read
    /// resolves it: where there is none, or no store yet, there is no task
    /// to change, and nothing is minted or created to say so.
    async fn existing(&mut self, pinned: Option<&str>) -> Result<Project, AppError> {
        let project = self
            .identity
            .for_read(pinned)
            .map_err(AppError::Identity)?
            .ok_or(AppError::NoProject)?;
        match self.stores.open_existing(&project).await.map_err(failed)? {
            Some(_) => Ok(project),
            None => Err(rejected(&TaskRejection::NoTask)),
        }
    }

    /// The command id `operation` sends: derived from the request's key
    /// when one is given ([`request_command_id`]), so a retry sends the
    /// same; a fresh one when none is.
    fn command_id(&mut self, operation: &str, request_id: Option<&str>) -> Uuid {
        match request_id {
            Some(key) => request_command_id(operation, key),
            None => self.clock.command_id(),
        }
    }

    /// Run `command` on the task `harp_id` in `project`'s store, under
    /// `command_id`, retried as taskloom's loop says; the task after it. A command
    /// already applied under `command_id` is answered DUPLICATE and
    /// applies nothing: the task is answered as it stands.
    async fn commit(
        &mut self,
        project: &Project,
        harp_id: &str,
        command: TaskCommand,
        command_id: Uuid,
    ) -> Result<Task, AppError> {
        let unshaped: Option<&fn(&TaskState) -> Option<CommandBook>> = None;
        self.commit_shaped(project, harp_id, command, command_id, unshaped)
            .await
    }

    /// [`Service::commit`] of what `shape` makes of the task as each
    /// attempt reads it ([`retry::Runner::run_shaped`]),
    /// or of `command` when there is no `shape`.
    async fn commit_shaped<F: Fn(&TaskState) -> Option<CommandBook> + Sync>(
        &mut self,
        project: &Project,
        harp_id: &str,
        command: TaskCommand,
        command_id: Uuid,
        shape: Option<&F>,
    ) -> Result<Task, AppError> {
        let store = self.stores.open(project).await.map_err(failed)?;
        let book = book(harp_id, command);
        let mut runner = retry::Runner {
            registry: &self.registry,
            clock: &mut self.clock,
            delay: &mut self.delay,
            backoff: self.backoff,
            dead_letters: &self.dead_letters,
        };
        match runner.run_shaped(store, &book, &command_id, shape).await? {
            None | Some(retry::Outcome::Committed) => {}
            Some(retry::Outcome::Refused(rejection)) => return Err(rejected(&rejection)),
        }
        // The read-back is a read of its own, after the commit: a
        // remove committed between the two makes a change that committed
        // report failure (Vanished). Accepted: the change did land, and a
        // retry of it finds no task to change.
        read(store, &self.registry, harp_id)
            .await?
            .ok_or_else(|| AppError::Vanished {
                harp_id: harp_id.to_string(),
            })
    }
}

/// The writes, each failure carrying the dead letters written while it
/// ran ([`TaskService`] wraps them).
impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    I: ProjectIdentity,
    S: TaskStores,
    N: Names,
    C: Clock + CommandIds,
    D: Delay + Jitter,
    G: Configuration,
{
    async fn adding(&mut self, task: NewTask) -> Result<Written, AppError> {
        if task.request_id.is_some() {
            return Err(AppError::AddRequestIdNotYet);
        }
        let status = given(task.status, "status")?;
        let trigger = given(task.trigger, "trigger")?;
        let origin_session = given(task.origin_session, "origin_session")?;
        let schema = self.schema()?;
        let tags = tags::admit_all(&task.tags, &schema).map_err(|r| refused(&r))?;
        self.check_permitted(&tags, &schema)?;
        tags::check_scalars(&schema, &tags).map_err(|r| refused(&r))?;
        let (tags, _) = tags::collapse(&schema, &[], tags);
        // Each attempt mints its own harp; everything else is this, held to
        // the domain's rules now, before a project is bound or a store
        // created for an add they refuse.
        let template = AddTask {
            harp_id: String::new(),
            text: task.text,
            status,
            trigger,
            tags,
            origin_session,
            defers: view::DEFERS.to_string(),
            initial: view::INITIAL.to_string(),
        };
        tasks::check(&template.clone().into()).map_err(|r| rejected(&r))?;
        let project = self
            .identity
            .for_write(task.project.as_deref())
            .map_err(AppError::Identity)?;
        let store = self.stores.open(&project).await.map_err(failed)?;
        let mut runner = retry::Runner {
            registry: &self.registry,
            clock: &mut self.clock,
            delay: &mut self.delay,
            backoff: self.backoff,
            dead_letters: &self.dead_letters,
        };
        for _ in 0..HARP_ATTEMPTS {
            let harp_id = self.names.task_id();
            let command_id = runner.clock.command_id();
            let add = AddTask {
                harp_id: harp_id.clone(),
                ..template.clone()
            };
            let book = book(&harp_id, add.into());
            match runner.run(store, &book, &command_id).await? {
                retry::Outcome::Committed => {
                    // The read-back is a read of its own, after the
                    // commit: a remove committed between the two makes an
                    // add that committed report failure (Vanished), and a
                    // caller that retries it adds the task again. Accepted.
                    let task = read(store, &self.registry, &harp_id)
                        .await?
                        .ok_or(AppError::Vanished { harp_id })?;
                    let shown = Shown::of(&schema);
                    return Ok(Written {
                        project,
                        task,
                        shown,
                        dead_letters: self.dead_letters.take(),
                    });
                }
                refused if refused.refused_as("tasks-task-exists") => continue,
                retry::Outcome::Refused(rejection) => return Err(rejected(&rejection)),
            }
        }
        Err(AppError::HarpsExhausted {
            attempts: HARP_ATTEMPTS,
        })
    }

    async fn tagging(&mut self, change: TagChange) -> Result<Written, AppError> {
        let request_id = given(change.request_id, "request_id")?;
        // Nothing to add or remove is refused before anything is read.
        let add = tags::parse_all(&change.add).map_err(|r| refused(&r))?;
        let remove = tags::parse_all(&change.remove).map_err(|r| refused(&r))?;
        let retag: TaskCommand = Retag {
            add: add.clone(),
            remove: remove.clone(),
        }
        .into();
        tasks::check(&retag).map_err(|r| rejected(&r))?;
        let schema = self.schema()?;
        for tag in &add {
            tags::admit(tag, &schema).map_err(|r| refused(&r))?;
        }
        self.check_permitted(&add, &schema)?;
        tags::check_scalars(&schema, &add).map_err(|r| refused(&r))?;
        let project = self.existing(change.project.as_deref()).await?;
        // A scalar target's new value displaces the one the task holds:
        // resolved here, against the task as the retag's own unit of work
        // reads it, so decide never reads the schema and no value written
        // since is left beside the new one.
        let shape = |task: &TaskState| {
            let (add, displaced) = tags::collapse(&schema, &task.tags, add.clone());
            let mut remove = remove.clone();
            remove.extend(displaced);
            Some(book(&change.harp_id, Retag { add, remove }.into()))
        };
        let command_id = self.command_id("tag", request_id.as_deref());
        let task = self
            .commit_shaped(&project, &change.harp_id, retag, command_id, Some(&shape))
            .await?;
        Ok(Written {
            project,
            task,
            shown: Shown::of(&schema),
            dead_letters: self.dead_letters.take(),
        })
    }

    async fn setting_status(&mut self, change: StatusChange) -> Result<Written, AppError> {
        let request_id = given(change.request_id, "request_id")?;
        let status = given(change.status, "status")?;
        let trigger = given(change.trigger, "trigger")?;
        // SetStatus's trigger given empty is its clearing, which no edge
        // spells so: a trigger given blank is refused above.
        let trigger = match (trigger, change.clear_trigger) {
            (Some(_), true) => return Err(AppError::TriggerAndClear),
            (None, true) => Some(String::new()),
            (trigger, false) => trigger,
        };
        let set: TaskCommand = SetStatus {
            status,
            trigger,
            defers: view::DEFERS.to_string(),
            initial: view::INITIAL.to_string(),
        }
        .into();
        tasks::check(&set).map_err(|r| rejected(&r))?;
        let schema = self.schema()?;
        let project = self.existing(change.project.as_deref()).await?;
        let command_id = self.command_id("set_status", request_id.as_deref());
        let task = self
            .commit(&project, &change.harp_id, set, command_id)
            .await?;
        Ok(Written {
            project,
            task,
            shown: Shown::of(&schema),
            dead_letters: self.dead_letters.take(),
        })
    }

    async fn editing(&mut self, edit: TextEdit) -> Result<Written, AppError> {
        let request_id = given(edit.request_id, "request_id")?;
        let command: TaskCommand = EditText { text: edit.text }.into();
        tasks::check(&command).map_err(|r| rejected(&r))?;
        let schema = self.schema()?;
        let project = self.existing(edit.project.as_deref()).await?;
        let command_id = self.command_id("edit", request_id.as_deref());
        let task = self
            .commit(&project, &edit.harp_id, command, command_id)
            .await?;
        Ok(Written {
            project,
            task,
            shown: Shown::of(&schema),
            dead_letters: self.dead_letters.take(),
        })
    }
}

impl<I, S, N, C, D, G> TaskService for Service<I, S, N, C, D, G>
where
    I: ProjectIdentity + Send,
    S: TaskStores + Send,
    N: Names + Send,
    C: Clock + CommandIds + Send,
    D: Delay + Jitter + Send,
    G: Configuration + Send,
{
    async fn add(&mut self, task: NewTask) -> Result<Written, AppError> {
        let written = self.adding(task).await;
        self.dead_letters.onto(written)
    }

    async fn tag(&mut self, change: TagChange) -> Result<Written, AppError> {
        let written = self.tagging(change).await;
        self.dead_letters.onto(written)
    }

    async fn set_status(&mut self, change: StatusChange) -> Result<Written, AppError> {
        let written = self.setting_status(change).await;
        self.dead_letters.onto(written)
    }

    async fn edit(&mut self, edit: TextEdit) -> Result<Written, AppError> {
        let written = self.editing(edit).await;
        self.dead_letters.onto(written)
    }

    async fn list(&mut self, query: ListQuery) -> Result<Listing, AppError> {
        self.listing(query).await
    }

    async fn show(&mut self, query: ShowQuery) -> Result<Details, AppError> {
        self.details(query).await
    }

    async fn summary(&mut self, scope: Scope) -> Result<Summary, AppError> {
        self.summed(scope).await
    }

    async fn statuses(&mut self, scope: Scope) -> Result<Statuses, AppError> {
        self.taxonomy(scope).await
    }

    async fn tags(&mut self, query: ListQuery) -> Result<Vocabulary, AppError> {
        self.vocabulary(query).await
    }

    async fn lint(&mut self, scope: Scope) -> Result<LintReport, AppError> {
        self.linted(scope).await
    }

    async fn tag_schema(&mut self) -> Result<TagSchema, AppError> {
        self.schema()
    }

    async fn permits(&mut self) -> Result<PermitList, AppError> {
        let schema = self.schema()?;
        Ok(PermitList {
            permits: self.tag_permits()?,
            schema_targets: schema.targets(),
        })
    }
}

/// `value`, refused when it is given (`Some`) blank: a value given is never
/// read as one left out. `field` names it in the refusal.
pub(crate) fn given<T: AsRef<str>>(
    value: Option<T>,
    field: &'static str,
) -> Result<Option<T>, AppError> {
    match value {
        Some(v) if v.as_ref().trim().is_empty() => Err(AppError::Blank { field }),
        value => Ok(value),
    }
}

/// The stream of the task `harp_id` names.
fn cover(harp_id: &str) -> Cover {
    Cover {
        domain: TASK_DOMAIN.to_string(),
        root: ids::compute_root(TASK_DOMAIN, harp_id).ok(),
        edition: None,
    }
}

/// `command` for the task `harp_id`. Its command id is left for the
/// kernel to take from the [`Sampled`](angzarr_embedded::next::Sampled) the runner
/// gives it, the same on every attempt.
fn book(harp_id: &str, command: TaskCommand) -> CommandBook {
    CommandBook {
        cover: Some(cover(harp_id)),
        page: Some(CommandPage {
            body: Some(command_page::Body::Command(Command {
                payload: Some(command.to_any()),
            })),
            ..CommandPage::default()
        }),
        handling: None,
    }
}

/// The task `harp_id` names, as the store holds it now, read in a
/// read-only unit; `None` when its stream holds no task.
async fn read<S: ReadStore>(
    store: &S,
    registry: &Registry,
    harp_id: &str,
) -> Result<Option<Task>, AppError> {
    let cover = cover(harp_id);
    let stream = StreamRef::try_from(&cover).map_err(failed)?;
    let mut reads = store.begin_read().await.map_err(failed)?;
    let slice = reads.read(stream, 0, None).await.map_err(failed)?;
    let stream = Stream::<TaskAggregate>::replay(cover, &slice.pages, registry)
        .map_err(|e| AppError::Failed(Report::of(&e)))?;
    // A stream that replays to no task (one with no events, or read back
    // with none) is no task, never a blank one.
    let state = stream.state().state();
    Ok((!state.harp_id.is_empty()).then(|| Task::of(state)))
}

/// A store's failure, as the kernel reports one: its key and args.
pub(crate) fn store_report(error: StoreError) -> Report {
    Report::of(&KernelError::Store(error))
}

/// A store's failure, as the use cases fail with it.
fn failed(error: StoreError) -> AppError {
    AppError::Failed(store_report(error))
}

fn refused(refusal: &TagRefusal) -> AppError {
    AppError::TagRefused(Report::of(refusal))
}

fn rejected(rejection: &dyn Diagnostic) -> AppError {
    AppError::Rejected(Report::of(rejection))
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;

#[cfg(test)]
#[path = "english.test.rs"]
mod english;

#[cfg(test)]
#[path = "fakes.test.rs"]
mod fakes;
