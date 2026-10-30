//! Watching a project's tasks: whether its store has changed since the
//! last poll, and nothing more. A poll asks the store for its change token
//! ([`WatchedStores`]), which reads no stream, and answers whether it
//! moved; a watcher (`taskloom watch`, whose subscriber re-queries the
//! task list on each line) needs no more than that. The token names the
//! store's handle, so a store opened again is always a change. Where the
//! project has no store yet, a poll is the store's look for one (for
//! SQLite, at the filesystem) until it appears.
//!
//! The token moves for any commit to the store, so a commit no task sees
//! (a purge after its removal, a purge of nothing) is a change too: the
//! watcher re-queries and finds the same tasks (the owner's ruling on D7,
//! 2026-10-30: nothing outside the process reads a cursor, so none is
//! kept). Nothing numbers across streams.

use std::future::Future;

use ports::{HeldToken, Project, ProjectIdentity, WatchedStores};

use crate::{failed, AppError, Service};

/// Which project to watch: `project` pins one, overriding resolution.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct WatchRequest {
    pub project: Option<String>,
}

/// A project being watched, and its store's token as of the last poll.
pub struct Watch {
    pub project: Project,
    /// The store's token at the last poll; none before it had a store.
    token: Option<HeldToken>,
}

impl Watch {
    /// A watch of `project` that has seen no token yet: what a fake
    /// [`WatchService`] hands out. Its first poll that finds a store is a
    /// change.
    pub fn of(project: Project) -> Watch {
        Watch {
            project,
            token: None,
        }
    }
}

/// Watching, as a driving adapter calls it. The futures are `Send`, as
/// [`crate::TaskService`]'s are.
pub trait WatchService {
    /// Start watching the project `request` names, as its store is now;
    /// `None` when the work root has no project yet. Resolving never
    /// mints: a watch binds no project.
    fn watch(
        &mut self,
        request: WatchRequest,
    ) -> impl Future<Output = Result<Option<Watch>, AppError>> + Send;

    /// Whether `watch`'s store has changed (its change token moved) since
    /// the last poll, or since the watch began. No stream is read; while
    /// the project has no store, nothing has changed.
    fn poll(&mut self, watch: &mut Watch) -> impl Future<Output = Result<bool, AppError>> + Send;
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
        let Some(project) = self
            .identity
            .for_read(request.project.as_deref())
            .map_err(AppError::Identity)?
        else {
            return Ok(None);
        };
        let mut watch = Watch::of(project);
        self.poll(&mut watch).await?;
        Ok(Some(watch))
    }

    async fn poll(&mut self, watch: &mut Watch) -> Result<bool, AppError> {
        let Some(token) = self
            .stores
            .change_token(&watch.project)
            .await
            .map_err(failed)?
        else {
            return Ok(false);
        };
        let moved = watch.token != Some(token);
        watch.token = Some(token);
        Ok(moved)
    }
}

#[cfg(test)]
#[path = "watch.test.rs"]
mod tests;
