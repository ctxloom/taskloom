//! Watching a project's tasks: one task list per watched project, of an
//! instance minted for it, caught up from the project's store when its
//! change token moves; each watcher resumes from its own cursor.
//!
//! A poll first asks the store for its change token ([`WatchedStores`]),
//! which reads no stream; only when it differs from the last poll's does
//! the poll re-read the task streams, and the list skips what it has
//! already projected. The token is read before the streams, so a commit
//! landing between the two is read again at the next poll rather than
//! missed; and it names the store's handle, so a store opened again is
//! always read. Where the project has no store yet, a poll is the store's
//! look for one (for SQLite, at the filesystem) until it appears.
//!
//! Nothing numbers across streams (the owner's ruling D7). A watcher's
//! [`WatchCursor`] is the list's cursor on the wire: the list's instance
//! and each task stream's last sequence seen, by harp id. [`Watch::since`]
//! answers [`Polled::Changes`] for a cursor of the list's instance, and
//! [`Polled::Snapshot`] (every task, and the cursor to resume from) for no
//! cursor, one of another instance (another process's list), or one from
//! before a purge, which bumps the list's instance.

use std::collections::BTreeMap;
use std::future::Future;

use angzarr_embedded_diagnostic::Report;
use ports::{CommandIds, HeldToken, Project, ProjectIdentity, Uuid, WatchedStores};
use task_lists::{Change, ListCursor, TaskList};

use crate::{failed, AppError, Service, Task};

/// Which project to watch: `project` pins one, overriding resolution.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct WatchRequest {
    pub project: Option<String>,
}

/// Where a watcher is, on the wire: the list's cursor
/// ([`task_lists::ListCursor`]) as plain data, for a driving adapter to
/// write and read back. `instance` is opaque, compared only; `streams` is
/// each task stream's last sequence seen, by harp id. As JSON:
/// `{"instance": "<id>/<purges>", "streams": {"swift-amber": 2}}`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct WatchCursor {
    pub instance: Option<String>,
    pub streams: BTreeMap<String, u64>,
}

impl WatchCursor {
    fn of(cursor: &ListCursor) -> WatchCursor {
        WatchCursor {
            instance: cursor.instance(),
            streams: cursor
                .streams()
                .into_iter()
                .map(|(harp, sequence)| (harp.to_string(), sequence))
                .collect(),
        }
    }

    fn list_cursor(&self) -> ListCursor {
        ListCursor::of(
            self.instance.as_deref(),
            self.streams.iter().map(|(h, s)| (h.clone(), *s)),
        )
    }
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
    /// Every task, by harp id, and the cursor to resume from: for a
    /// watcher with no cursor, or one of another instance (another
    /// process's list, or this one's before a purge).
    Snapshot {
        tasks: Vec<Task>,
        cursor: WatchCursor,
    },
}

/// A project being watched: its task list as of the last poll, and the
/// store's token then.
pub struct Watch {
    pub project: Project,
    list: TaskList,
    /// The store's token when the list was last caught up; none before.
    token: Option<HeldToken>,
}

impl Watch {
    /// A watch of `project` that has read nothing yet, of no instance in
    /// particular: what a fake [`WatchService`] hands out. The service's
    /// own ([`WatchService::watch`]) is of an instance it mints.
    pub fn of(project: Project) -> Watch {
        Watch {
            project,
            list: TaskList::new(Uuid::from_bytes([0; 16])),
            token: None,
        }
    }

    /// What a watcher at `cursor` (`None`: none yet) is answered, from the
    /// list as of the last poll.
    pub fn since(&self, cursor: Option<&WatchCursor>) -> Polled {
        let after = WatchCursor::of(&self.list.cursor());
        let changes = cursor.and_then(|c| self.list.since(&c.list_cursor()));
        match changes {
            Some(changes) => Polled::Changes {
                changes: changes
                    .into_iter()
                    .map(|change| match change {
                        Change::Upsert(state) => Changed::Task(Task::of(&state)),
                        Change::Remove { harp_id } => Changed::Removed { harp_id },
                    })
                    .collect(),
                cursor: after,
            },
            None => Polled::Snapshot {
                tasks: self.list.entries().into_iter().map(Task::of).collect(),
                cursor: after,
            },
        }
    }
}

/// Watching, as a driving adapter calls it. The futures are `Send`, as
/// [`crate::TaskService`]'s are.
pub trait WatchService {
    /// Start watching the project `request` names, caught up with its
    /// store as it is now; `None` when the work root has no project yet.
    /// Resolving never mints: a watch binds no project.
    fn watch(
        &mut self,
        request: WatchRequest,
    ) -> impl Future<Output = Result<Option<Watch>, AppError>> + Send;

    /// Catch `watch` up with its store, if its change token moved, and
    /// answer the watcher at `cursor` ([`Watch::since`]). No stream is
    /// read while the token holds, nor while the project has no store.
    fn poll(
        &mut self,
        watch: &mut Watch,
        cursor: Option<&WatchCursor>,
    ) -> impl Future<Output = Result<Polled, AppError>> + Send;
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
        let Some(project) = self
            .identity
            .for_read(request.project.as_deref())
            .map_err(AppError::Identity)?
        else {
            return Ok(None);
        };
        let mut watch = Watch {
            project,
            list: TaskList::new(self.clock.command_id()),
            token: None,
        };
        self.catch_up(&mut watch).await?;
        Ok(Some(watch))
    }

    async fn poll(
        &mut self,
        watch: &mut Watch,
        cursor: Option<&WatchCursor>,
    ) -> Result<Polled, AppError> {
        self.catch_up(watch).await?;
        Ok(watch.since(cursor))
    }
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    S: WatchedStores,
{
    /// Catch `watch` up with its store, if its change token moved.
    async fn catch_up(&mut self, watch: &mut Watch) -> Result<(), AppError> {
        let Some(token) = self
            .stores
            .change_token(&watch.project)
            .await
            .map_err(failed)?
        else {
            return Ok(());
        };
        if watch.token == Some(token) {
            return Ok(());
        }
        let Some(store) = self
            .stores
            .open_existing(&watch.project)
            .await
            .map_err(failed)?
        else {
            return Ok(());
        };
        watch
            .list
            .catch_up(store, &self.registry)
            .await
            .map_err(|e| AppError::Failed(Report::of(&*e)))?;
        watch.token = Some(token);
        Ok(())
    }
}

#[cfg(test)]
#[path = "watch.test.rs"]
mod tests;
