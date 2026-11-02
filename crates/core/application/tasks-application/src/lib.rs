//! The tasks use cases, each a unit of its own over only the ports it
//! uses, borrowed from one [`Service`] that owns the ports and composes
//! the units behind the traits a driving adapter calls:
//!
//! - add a task ([`TaskService::add`]);
//! - change one: its status, deferring it on a trigger or reviving it
//!   ([`TaskService::set_status`]), its text ([`TaskService::edit`]), its
//!   tags ([`TaskService::tag`]);
//! - read them: list, show, count, the statuses and tags in use, lint
//!   ([`TaskService::list`] and the rest), and the store's rules
//!   ([`TaskService::tag_schema`], [`TaskService::permits`],
//!   [`SchemaService`]);
//! - prune old finished ones ([`PruneService`]);
//! - grant and revoke tag permits, and declare the store's rules
//!   ([`PermitService`], a person's, apart so a frontend can leave it out);
//! - watch a project's store change ([`WatchService`]);
//! - and, apart from them, import the JSONL task logs ([`Importer`], the
//!   one use case that reads them).
//!
//! Each reaches its project through one collaborator (`project`:
//! [`ProjectIdentity`], [`TaskStores`], the configuration's moved keys
//! refused), and a write runs its commands through angzarr-embedded's
//! kernel through kernel-runner's [`Runner`] (`write`), which retries
//! them ([`KERNEL_RETRY`]: the kernel owns race re-runs, its waits
//! taskloom's ports); a read is the task-list projector caught up from the
//! store (`lists`). The ports are the only way out: this crate does no
//! I/O, keeps no clock and starts no runtime.

mod add;
mod change;
mod error;
mod import;
mod lint;
mod lists;
mod permit;
mod priority;
mod project;
mod prune;
mod query;
mod read;
mod request;
mod rules;
mod schema;
mod semver;
mod tags;
mod task;
mod view;
mod watch;
mod write;

use std::future::Future;

use angzarr_diagnostic::{Diagnostic, Report};
use angzarr_embedded::{KernelError, OnRetryable, Registry, StoreError};
use kernel_runner::{Runner, KERNEL_RETRY};
use ports::{
    Clock, CommandIds, Configuration, ConfigurationWriter, Delay, Jitter, Names, ProjectIdentity,
    TaskStores, WatchedStores,
};
use tasks::schema::SchemaAggregate;
use tasks::TaskAggregate;

use add::Adding;
use change::Changing;
use error::carrying;
use lists::Lists;
use permit::Permitting;
use project::Projects;
use prune::Pruner;
use read::Reading;
use watch::Watching;
use write::Writes;

/// A dead letter, as the kernel answers it and the store keeps it: angzarr's
/// `AngzarrDeadLetter` whole, keyed by the stream it is about. A use case
/// returns every one its kernel calls answered, beside its result or on
/// its error, for its driving adapter to say loudly ([`dead_letter_line`]):
/// each almost certainly marks a bug. A command taskloom waits for is
/// answered, never dead-lettered.
pub use angzarr_embedded::{dead_letter_line, DeadLetter};

/// A dead letter as the kernel answers one, from `component`, about the
/// task swift-amber's stream, its line `DEAD_LETTER <component>:
/// ROUTER_REASON_UNHANDLED_HANDLER_ERROR`: for a test that hands a use
/// case, or a driving adapter, a letter a kernel call answered.
#[cfg(any(test, feature = "test-support"))]
pub fn dead_letter_sample(component: &str) -> DeadLetter {
    use angzarr_embedded::v1::{AngzarrDeadLetter, Diagnostic};
    DeadLetter {
        stream: task_cover("swift-amber"),
        at: angzarr_embedded::DeadLetterAt::Sequence(0),
        letter: AngzarrDeadLetter {
            source_component: component.to_string(),
            reason: Some(Diagnostic {
                code: "ROUTER_REASON_UNHANDLED_HANDLER_ERROR".to_string(),
                args: Default::default(),
            }),
            ..AngzarrDeadLetter::default()
        },
    }
}
pub use add::{NewTask, KEYED_PROBES};
pub use change::{StatusChange, TagChange, TextEdit};
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
pub use read::{
    Details, Hidden, ListQuery, Listing, Reach, Scope, ShowQuery, Statuses, Summary, TagCount,
    Vocabulary,
};
pub use request::{request_command_id, REQUEST_NAMESPACE};
pub use rules::{
    Code as RuleCode, Finding as RuleFinding, Origin, RuleWarning, RulesRefusal, SchemaSummary,
    TargetSummary, SEEDED_SCHEMA, TASKLOOM_NAMESPACE,
};
pub use schema::{Declaration, Declared, SchemaService};
pub use semver::{Semver, Version, SEMVER};
/// A finding's tag is tagma's own type, as tagma reports it.
pub use tagma_core::Tag as RuleTag;
pub use tags::{
    is_retired as tag_is_retired, parse as parse_tag, schema_free as schema_free_tag, Tag,
    RETIRED_TAG,
};
pub use task::Written;
pub use view::{text_hash, Row, Shown, StatusInfo, Task};
pub use watch::{Watch, WatchRequest, WatchService};

// Used across the crate's modules by these paths; nothing outside uses them.
#[cfg(test)]
pub(crate) use add::HARP_ATTEMPTS;
pub(crate) use tags::TagRefusal;
pub(crate) use task::task_cover;
pub(crate) use view::DONE;

/// The use cases a driving adapter calls for a task: the writes (add,
/// status, edit, tag) and the reads. Their futures are `Send`, so a server
/// can spawn them.
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
    /// The store's rules a write is held to, as `taskloom schema show`
    /// prints them: the work root's project's store's (the seed, where it
    /// has none yet).
    fn tag_schema(&mut self) -> impl Future<Output = Result<SchemaSummary, AppError>> + Send;
    /// What the store's permits permit, read-only: its `tagma.permit`
    /// patterns, whether its world is closed, and the targets its rules
    /// constrain. Granting and revoking are [`PermitService`]'s, a
    /// person's; reading them is here, so an agent can see them.
    fn permits(&mut self) -> impl Future<Output = Result<PermitList, AppError>> + Send;
}

/// The registry the tasks domain runs under: the task aggregate, and the
/// store's schema stream's.
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    registry
        .aggregate::<TaskAggregate>(TaskAggregate::default)
        .and_then(|registry| registry.aggregate::<SchemaAggregate>(SchemaAggregate::default))
        .expect("the task and schema domains, each once");
    registry
}

/// The ports the use cases run over, owned once, and what they share
/// across calls: `identity` resolves a project and `stores` opens its
/// store; `names` mints task ids (only an add does); `clock` stamps
/// commands and mints their ids; `delay` waits, with jitter, between
/// retries; `configuration` is what is configured, nothing until
/// [`Service::with_configuration`] gives it. Each call lends its use case
/// only the ports it uses. It reads no JSONL task log: only an
/// [`Importer`] does.
pub struct Service<I, S, N, C, D, G = NotConfigured> {
    pub identity: I,
    pub stores: S,
    pub names: N,
    pub clock: C,
    pub delay: D,
    pub configuration: G,
    /// How the kernel retries a command ([`KERNEL_RETRY`]).
    pub retry: OnRetryable,
    pub(crate) registry: Registry,
    /// Each project's task list, kept caught up across the service's
    /// reads.
    pub(crate) lists: Lists,
    /// The dead letters the use case's kernel calls answered, taken into
    /// its result, or its error.
    pub(crate) dead_letters: Vec<DeadLetter>,
}

/// Configuration, as a service without any has it: nothing configured, so
/// no moved key set and none to import.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NotConfigured;

/// Nothing configured is nothing to write: nothing calls it
/// ([`ports::ConfigurationWriter::write_tag_permits`] goes with the
/// import), and there is no moved key to retire.
impl ports::ConfigurationWriter for NotConfigured {
    // Equivalent: nothing is configured, so nothing is written.
    #[cfg_attr(test, mutants::skip)]
    fn write_tag_permits(&mut self, _: &ports::TagPermits) -> Result<(), Report> {
        Ok(())
    }
}

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
            retry: KERNEL_RETRY,
            registry: registry(),
            lists: Lists::default(),
            dead_letters: Vec::new(),
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
            retry: self.retry,
            registry: self.registry,
            lists: self.lists,
            dead_letters: self.dead_letters,
        }
    }

    /// The ports and shared state, each lent apart, for one call.
    fn parts(&mut self) -> Parts<'_, I, S, N, C, D, G> {
        Parts {
            projects: Projects {
                identity: &mut self.identity,
                stores: &mut self.stores,
                configuration: &mut self.configuration,
                registry: &self.registry,
            },
            names: &mut self.names,
            runner: Runner {
                registry: &self.registry,
                clock: &mut self.clock,
                delay: &mut self.delay,
                retry: self.retry,
                dead_letters: &mut self.dead_letters,
            },
            lists: &mut self.lists,
        }
    }

    /// The writes' share of the parts: the project and the kernel.
    fn writes(&mut self) -> Writes<'_, I, S, G, C, D> {
        self.parts().writes().0
    }

    fn adding(&mut self) -> Adding<'_, I, S, N, G, C, D> {
        let (writes, parts) = self.parts().writes();
        Adding {
            writes,
            names: parts.names,
        }
    }

    fn changing(&mut self) -> Changing<'_, I, S, G, C, D> {
        Changing {
            writes: self.writes(),
        }
    }

    fn reading(&mut self) -> Reading<'_, I, S, G, C> {
        let parts = self.parts();
        Reading {
            projects: parts.projects,
            clock: parts.runner.clock,
            lists: parts.lists,
        }
    }

    fn pruner(&mut self) -> Pruner<'_, I, S, G, C, D> {
        let (writes, rest) = self.parts().writes();
        Pruner {
            writes,
            lists: rest.lists,
        }
    }

    fn permitting(&mut self) -> Permitting<'_, I, S, G, C, D> {
        let (writes, rest) = self.parts().writes();
        Permitting {
            writes,
            lists: rest.lists,
        }
    }

    fn watching(&mut self) -> Watching<'_, I, S> {
        Watching {
            identity: &mut self.identity,
            stores: &mut self.stores,
        }
    }
}

/// A [`Service`]'s ports and shared state, each borrowed apart, from
/// which a call's use case takes what it uses.
struct Parts<'s, I, S, N, C, D, G> {
    projects: Projects<'s, I, S, G>,
    names: &'s mut N,
    runner: Runner<'s, C, D>,
    lists: &'s mut Lists,
}

/// What the parts lend beside a write's [`Writes`].
struct Rest<'s, N> {
    names: &'s mut N,
    lists: &'s mut Lists,
}

impl<'s, I, S, N, C, D, G> Parts<'s, I, S, N, C, D, G> {
    /// The project and the kernel, as a write takes them, and the rest.
    fn writes(self) -> (Writes<'s, I, S, G, C, D>, Rest<'s, N>) {
        let writes = Writes {
            projects: self.projects,
            runner: self.runner,
        };
        let rest = Rest {
            names: self.names,
            lists: self.lists,
        };
        (writes, rest)
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
        let written = self.adding().add(task).await;
        carrying(written, &mut self.dead_letters)
    }

    async fn tag(&mut self, change: TagChange) -> Result<Written, AppError> {
        let written = self.changing().retag(change).await;
        carrying(written, &mut self.dead_letters)
    }

    async fn set_status(&mut self, change: StatusChange) -> Result<Written, AppError> {
        let written = self.changing().set_status(change).await;
        carrying(written, &mut self.dead_letters)
    }

    async fn edit(&mut self, edit: TextEdit) -> Result<Written, AppError> {
        let written = self.changing().edit(edit).await;
        carrying(written, &mut self.dead_letters)
    }

    async fn list(&mut self, query: ListQuery) -> Result<Listing, AppError> {
        self.reading().list(query).await
    }

    async fn show(&mut self, query: ShowQuery) -> Result<Details, AppError> {
        self.reading().show(query).await
    }

    async fn summary(&mut self, scope: Scope) -> Result<Summary, AppError> {
        self.reading().summary(scope).await
    }

    async fn statuses(&mut self, scope: Scope) -> Result<Statuses, AppError> {
        self.reading().statuses(scope).await
    }

    async fn tags(&mut self, query: ListQuery) -> Result<Vocabulary, AppError> {
        self.reading().tags(query).await
    }

    async fn lint(&mut self, scope: Scope) -> Result<LintReport, AppError> {
        self.reading().lint(scope).await
    }

    async fn tag_schema(&mut self) -> Result<SchemaSummary, AppError> {
        let rules = self.parts().projects.rules_for(None).await?;
        Ok(rules.summary())
    }

    async fn permits(&mut self) -> Result<PermitList, AppError> {
        let rules = self.parts().projects.rules_for(None).await?;
        Ok(PermitList {
            closed: rules.closed(),
            patterns: rules.permits(),
            schema_targets: rules.targets(),
        })
    }
}

impl<I, S, N, C, D, G> PruneService for Service<I, S, N, C, D, G>
where
    I: ProjectIdentity + Send,
    S: TaskStores + Send,
    N: Send,
    C: Clock + CommandIds + Send,
    D: Delay + Jitter + Send,
    G: Configuration + Send,
{
    async fn prune(&mut self, request: PruneRequest) -> Result<Pruning, AppError> {
        let pruning = self.pruner().prune(request).await;
        carrying(pruning, &mut self.dead_letters)
    }
}

impl<I, S, N, C, D, G> PermitService for Service<I, S, N, C, D, G>
where
    I: ProjectIdentity + Send,
    S: TaskStores + Send,
    N: Send,
    C: Clock + CommandIds + Send,
    D: Delay + Jitter + Send,
    G: Configuration + ConfigurationWriter + Send,
{
    async fn permit(&mut self, change: PermitChange) -> Result<Permitted, AppError> {
        self.permitting().permit(change).await
    }

    async fn revoke(&mut self, change: PermitChange) -> Result<Permitted, AppError> {
        self.permitting().revoke(change).await
    }

    async fn declare(&mut self, declaration: Declaration) -> Result<Declared, AppError> {
        self.permitting().declare(declaration).await
    }

    async fn import_config(&mut self) -> Result<Declared, AppError> {
        self.permitting().import_config().await
    }
}

impl<I, S, N, C, D, G> SchemaService for Service<I, S, N, C, D, G>
where
    I: ProjectIdentity + Send,
    S: TaskStores + Send,
    N: Send,
    C: Clock + CommandIds + Send,
    D: Delay + Jitter + Send,
    G: Configuration + Send,
{
    async fn schema_export(&mut self, project: Option<String>) -> Result<Vec<String>, AppError> {
        schema::export(&mut self.writes(), project.as_deref()).await
    }

    async fn schema_check(
        &mut self,
        project: Option<String>,
    ) -> Result<Vec<RuleFinding>, AppError> {
        let rules = self.parts().projects.rules_for(project.as_deref()).await?;
        Ok(rules.check())
    }
}

impl<I, S, N, C, D, G> WatchService for Service<I, S, N, C, D, G>
where
    I: ProjectIdentity + Send,
    S: WatchedStores + Send,
    N: Send,
    C: Send,
    D: Send,
    G: Send,
{
    async fn watch(&mut self, request: WatchRequest) -> Result<Option<Watch>, AppError> {
        self.watching().watch(request).await
    }

    async fn poll(&mut self, watch: &mut Watch) -> Result<bool, AppError> {
        self.watching().poll(watch).await
    }
}

/// A store's failure, as the kernel reports one: its key and args.
pub(crate) fn store_report(error: StoreError) -> Report {
    Report::of(&KernelError::Store(error))
}

/// A store's failure, as the use cases fail with it.
pub(crate) fn failed(error: StoreError) -> AppError {
    AppError::Failed(store_report(error))
}

pub(crate) fn refused(refusal: &TagRefusal) -> AppError {
    AppError::TagRefused(Report::of(refusal))
}

pub(crate) fn rejected(rejection: &dyn Diagnostic) -> AppError {
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
