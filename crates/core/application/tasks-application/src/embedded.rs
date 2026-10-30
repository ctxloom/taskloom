//! RED, step 4 of taskloom-on-angzarr-embedded.plan.md: the use cases on
//! angzarr-embedded, beside annals' ([`crate::Service`]) until green, when
//! they replace them under the same names and annals leaves. Every body is
//! `todo!()`. The proposed shape:
//!
//! - **Commands** go through angzarr-embedded's [`Kernel::handle`] over the
//!   project's store ([`ports::embedded::TaskStores`]), one atomic write a
//!   step, [`registry`]'s `TaskAggregate` deciding. A command depending on
//!   the task as it is (a scalar tag displacing the one held) is shaped
//!   from a read of the stream and sent at that head (the book's expected
//!   sequence, STRICT): there is no unit of work to hold the stream, so a
//!   write landing between is the kernel's conflict, and the retry shapes
//!   it again.
//! - **One retry loop, taskloom's.** The kernel's capped retry is on by
//!   default (three retries of a retryable bug, then a dead letter); the
//!   application turns it off ([`KERNEL_RETRY`]: `max_retries` 0), so a
//!   retryable failure, a lost race (ABORTED: a conflict, a stale command)
//!   or a store that could not say (UNAVAILABLE), comes straight back to
//!   taskloom's loop ([`crate::Backoff`]: the same command id again after
//!   a jittered, capped wait over the [`ports::Delay`] and
//!   [`ports::Jitter`] ports, then `tasks-application-gave-up`). One
//!   failure is one attempt, never the kernel's retries times taskloom's,
//!   and nothing a person's command could not do is dead-lettered behind
//!   them: the error comes back to the person.
//! - **Prune** records the removal through the kernel, then purges the
//!   stream through the store's own [`Purge`](angzarr_embedded::next::Purge),
//!   having read it removed; a task minted on the harp since that read is
//!   never purged (see the open question in the step's report).
//! - **Watch** ([`WatchService`]) keeps one [`task_lists::embedded::TaskList`]
//!   per watched project, of an instance it mints, caught up when the
//!   store's [`Watch`](angzarr_embedded::next::Watch) token moves; each
//!   watcher resumes from its [`WatchCursor`], the list's cursor on the
//!   wire, and is answered [`Polled::Changes`], or [`Polled::Snapshot`]
//!   (every task, and the cursor to resume from) for no cursor, one of
//!   another instance, or one from before a purge.

use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use angzarr_embedded::next::{OnRetryable, Registry};
use ports::embedded::{CommandIds, TaskStores, WatchedStores};
use ports::{Clock, Configuration, Delay, Jitter, Names, Project, ProjectIdentity};

use crate::{
    AppError, Backoff, Details, LintReport, ListQuery, Listing, NewTask, NotConfigured, PermitList,
    PruneRequest, PruneService, Pruning, Scope, ShowQuery, StatusChange, Statuses, Summary,
    TagChange, TagSchema, Task, TaskService, TextEdit, Vocabulary, Written,
};

/// How the application has the kernel retry: not at all. taskloom's loop
/// ([`crate::Backoff`]) is the one that retries, so a failure is never
/// retried by both.
pub const KERNEL_RETRY: OnRetryable = OnRetryable {
    max_retries: 0,
    initial_backoff: Duration::from_millis(100),
    multiplier: 2.0,
    max_backoff: Duration::from_secs(5),
    jitter: 0.2,
};

/// The registry the tasks domain runs under: `TaskAggregate`.
pub fn registry() -> Registry {
    todo!("step 4: the registry of the task aggregate")
}

/// The use cases over their ports, on angzarr-embedded: as
/// [`crate::Service`], its store angzarr-ports' and its command ids
/// angzarr's.
pub struct Service<I, S, N, C, D, G = NotConfigured> {
    pub identity: I,
    pub stores: S,
    pub names: N,
    pub clock: C,
    pub delay: D,
    pub configuration: G,
    pub backoff: Backoff,
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
        }
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
        let _ = task;
        todo!("step 4: add, through the kernel")
    }

    async fn tag(&mut self, change: TagChange) -> Result<Written, AppError> {
        let _ = change;
        todo!("step 4: tag, through the kernel")
    }

    async fn list(&mut self, query: ListQuery) -> Result<Listing, AppError> {
        let _ = query;
        todo!("step 4: list, from the list caught up")
    }

    async fn set_status(&mut self, change: StatusChange) -> Result<Written, AppError> {
        let _ = change;
        todo!("step 4: set a status, through the kernel")
    }

    async fn edit(&mut self, edit: TextEdit) -> Result<Written, AppError> {
        let _ = edit;
        todo!("step 4: edit, through the kernel")
    }

    async fn show(&mut self, query: ShowQuery) -> Result<Details, AppError> {
        let _ = query;
        todo!("step 4: show")
    }

    async fn summary(&mut self, scope: Scope) -> Result<Summary, AppError> {
        let _ = scope;
        todo!("step 4: summary")
    }

    async fn statuses(&mut self, scope: Scope) -> Result<Statuses, AppError> {
        let _ = scope;
        todo!("step 4: statuses")
    }

    async fn tags(&mut self, query: ListQuery) -> Result<Vocabulary, AppError> {
        let _ = query;
        todo!("step 4: tags")
    }

    async fn lint(&mut self, scope: Scope) -> Result<LintReport, AppError> {
        let _ = scope;
        todo!("step 4: lint")
    }

    async fn tag_schema(&mut self) -> Result<TagSchema, AppError> {
        todo!("step 4: the tag schema")
    }

    async fn permits(&mut self) -> Result<PermitList, AppError> {
        todo!("step 4: the permits")
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
        let _ = request;
        todo!("step 4: prune, through the kernel and Purge")
    }
}

/// Which project to watch: `project` pins one, overriding resolution.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct WatchRequest {
    pub project: Option<String>,
}

/// Where a watcher is, on the wire: the list's cursor
/// ([`task_lists::embedded::ListCursor`]) as plain data, for a driving
/// adapter to write and read back. `instance` is opaque, compared only;
/// `streams` is each task stream's last sequence seen, by harp id. The
/// proposed JSON: `{"instance": "<id>/<purges>", "streams": {"swift-amber": 2}}`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct WatchCursor {
    pub instance: Option<String>,
    pub streams: BTreeMap<String, u64>,
}

/// One change to a watched project's tasks.
#[derive(Clone, PartialEq, Debug)]
pub enum Changed {
    /// A task was added, or changed: the task now.
    Task(Task),
    /// A task was removed.
    Removed { harp_id: String },
}

/// What a watcher at a cursor is answered.
#[derive(Clone, PartialEq, Debug)]
pub enum Polled {
    /// What moved since the cursor, by harp id, and the cursor after.
    Changes {
        changes: Vec<Changed>,
        cursor: WatchCursor,
    },
    /// Every task, and the cursor to resume from: for a watcher with no
    /// cursor, or one of another instance (another process's list, or this
    /// one's before a purge).
    Snapshot {
        tasks: Vec<Task>,
        cursor: WatchCursor,
    },
}

/// A project being watched: its task list, of an instance minted for it,
/// as of the last poll, and the store's token then.
pub struct Watch {
    pub project: Project,
}

impl Watch {
    /// What a watcher at `cursor` (`None`: none yet) is answered, from the
    /// list as of the last poll.
    pub fn since(&self, cursor: Option<&WatchCursor>) -> Polled {
        let _ = cursor;
        todo!("step 4: a watcher's answer")
    }
}

/// Watching, as a driving adapter calls it.
pub trait WatchService {
    /// Start watching the project `request` names, caught up with its store
    /// as it is now; `None` when the work root has no project yet.
    fn watch(
        &mut self,
        request: WatchRequest,
    ) -> impl Future<Output = Result<Option<Watch>, AppError>> + Send;

    /// Catch `watch` up with its store, if its change token moved; whether
    /// it did. No stream is read while the token holds.
    fn poll(&mut self, watch: &mut Watch) -> impl Future<Output = Result<bool, AppError>> + Send;
}

impl<I, S, N, C, D, G> WatchService for Service<I, S, N, C, D, G>
where
    I: ProjectIdentity + Send,
    S: WatchedStores + Send,
    N: Send,
    C: CommandIds + Send,
    D: Send,
    G: Send,
{
    async fn watch(&mut self, request: WatchRequest) -> Result<Option<Watch>, AppError> {
        let _ = request;
        todo!("step 4: start a watch")
    }

    async fn poll(&mut self, watch: &mut Watch) -> Result<bool, AppError> {
        let _ = watch;
        todo!("step 4: poll a watch")
    }
}

#[cfg(test)]
#[path = "embedded.test.rs"]
mod tests;
