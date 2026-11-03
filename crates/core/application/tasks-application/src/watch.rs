//! Watching a project's tasks. A poll asks the store for its change token
//! ([`WatchedStores`]), which reads no stream, and answers whether it
//! moved. The token names the store's handle, so a store opened again is
//! always a change. Where the project has no store, a poll is the store's
//! look for one (for SQLite, at the filesystem).
//!
//! The token moves for any commit to the store, so it says only that a
//! scan is worth making. The scan ([`WatchService::after`]) reads each
//! task stream from where a cursor says the watcher is on it and answers
//! what is new: an event, with the task as the event leaves it, or the
//! word that a stream is gone ([`Found::Expired`]). The cursor is the
//! owner's ruling of 2026-11-03, which reverses D7's cut (2026-10-30: no
//! cursor): one position PER STREAM, since nothing numbers across
//! streams ([`Positions`]); the watch keeps none itself, the caller
//! holds them and hands them back.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

use angzarr_diagnostic::Report;
use angzarr_embedded::v1::{Cover, EventPage};
use angzarr_embedded::{Domain, PageContext, Payload, ReadStore, ReadUnit, StreamRef};
use ports::{HeldToken, Project, ProjectIdentity, WatchedStores};
use tasks::{TaskAggregate, TaskEvent, TASK_DOMAIN};
use uuid::Uuid;

use crate::{failed, AppError, Task};

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
    /// Whether a poll has looked for the store yet.
    looked: bool,
}

/// A task stream: its edition (none: main) and its root. The order is the
/// store's listing order.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct StreamKey {
    pub edition: Option<String>,
    pub root: Uuid,
}

impl StreamKey {
    fn of(stream: &StreamRef<'_>) -> StreamKey {
        StreamKey {
            edition: stream.edition.map(str::to_string),
            root: stream.root,
        }
    }
}

/// Where a watcher is on each task stream: the next sequence it has yet to
/// see. A stream with no entry is read from its start.
pub type Positions = BTreeMap<StreamKey, u64>;

/// A task as an event left it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Seen {
    pub harp_id: String,
    pub text: String,
    pub status: String,
    /// Each tag in its canonical spelling, sorted.
    pub tags: Vec<String>,
    /// Whether this event (or an earlier one) removed the task.
    pub removed: bool,
}

/// What a scan found new.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Found {
    /// The event at `sequence` of `stream`, and the task as it leaves it.
    Event {
        stream: StreamKey,
        sequence: u64,
        task: Seen,
    },
    /// The watcher was past the end of `stream`, or the stream is gone:
    /// pruned (and maybe minted again). The watcher is back at its start,
    /// and what follows for it in the scan is read from there.
    Expired { stream: StreamKey },
}

impl Watch {
    /// Whether the last poll looked for the project's store and found
    /// none: a watch of such a project names one never written to. A
    /// watch that has not polled yet lacks nothing.
    pub fn lacks_store(&self) -> bool {
        self.looked && self.token.is_none()
    }

    /// A watch of `project` whose store a poll looked for and did not
    /// find: what a fake [`WatchService`] hands out for a project never
    /// written to.
    pub fn without_store(project: Project) -> Watch {
        Watch {
            project,
            token: None,
            looked: true,
        }
    }

    /// A watch of `project` that has seen no token yet: what a fake
    /// [`WatchService`] hands out. Its first poll that finds a store is a
    /// change.
    pub fn of(project: Project) -> Watch {
        Watch {
            project,
            token: None,
            looked: false,
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

    /// Where every task stream of the project's store ends now.
    fn heads(&mut self, watch: &Watch) -> impl Future<Output = Result<Positions, AppError>> + Send;

    /// What is new on the task streams since `positions`, in one moment of
    /// the store: per stream in sequence order, the streams in the
    /// store's order; no order across streams is promised.
    fn after(
        &mut self,
        watch: &Watch,
        positions: &Positions,
    ) -> impl Future<Output = Result<Vec<Found>, AppError>> + Send;
}

/// Watching over the two ports it needs: the project's resolution and its
/// store's change token. Borrowed from the service for one call.
pub(crate) struct Watching<'s, I, S> {
    pub identity: &'s mut I,
    pub stores: &'s mut S,
}

impl<I: ProjectIdentity, S: WatchedStores> Watching<'_, I, S> {
    /// [`WatchService::watch`].
    pub(crate) async fn watch(&mut self, request: WatchRequest) -> Result<Option<Watch>, AppError> {
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

    /// [`WatchService::poll`].
    pub(crate) async fn poll(&mut self, watch: &mut Watch) -> Result<bool, AppError> {
        watch.looked = true;
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

impl<I, S: WatchedStores> Watching<'_, I, S> {
    /// [`WatchService::heads`].
    pub(crate) async fn heads(&mut self, watch: &Watch) -> Result<Positions, AppError> {
        let Some(store) = self
            .stores
            .open_existing(&watch.project)
            .await
            .map_err(failed)?
        else {
            return Ok(Positions::new());
        };
        let mut reads = store.begin_read().await.map_err(failed)?;
        let mut heads = Positions::new();
        for cover in reads.covers(TASK_DOMAIN).await.map_err(failed)? {
            let stream = StreamRef::try_from(&cover).map_err(failed)?;
            let head = reads.head(stream).await.map_err(failed)?;
            heads.insert(StreamKey::of(&stream), head.next_sequence);
        }
        Ok(heads)
    }

    /// [`WatchService::after`].
    pub(crate) async fn after(
        &mut self,
        watch: &Watch,
        positions: &Positions,
    ) -> Result<Vec<Found>, AppError> {
        let Some(store) = self
            .stores
            .open_existing(&watch.project)
            .await
            .map_err(failed)?
        else {
            return Ok(Vec::new());
        };
        let mut reads = store.begin_read().await.map_err(failed)?;
        let mut found = Vec::new();
        let mut listed = BTreeSet::new();
        for cover in reads.covers(TASK_DOMAIN).await.map_err(failed)? {
            let stream = StreamRef::try_from(&cover).map_err(failed)?;
            let key = StreamKey::of(&stream);
            listed.insert(key.clone());
            let mut from = positions.get(&key).copied().unwrap_or(0);
            let mut slice = reads.read(stream, from, None).await.map_err(failed)?;
            if slice.next_sequence < from {
                // Shorter than the watcher had seen: a stream pruned and
                // minted again. The new one is read from its start.
                found.push(Found::Expired {
                    stream: key.clone(),
                });
                from = 0;
                slice = reads.read(stream, 0, None).await.map_err(failed)?;
            }
            if slice.pages.is_empty() {
                continue;
            }
            // The task as it stood before the first new event.
            let mut task = TaskAggregate::default();
            if from > 0 {
                let before = reads.read(stream, 0, Some(from)).await.map_err(failed)?;
                for page in &before.pages {
                    fold(&mut task, &cover, page)?;
                }
            }
            for page in &slice.pages {
                fold(&mut task, &cover, page)?;
                found.push(Found::Event {
                    stream: key.clone(),
                    sequence: page.sequence,
                    task: seen(&task),
                });
            }
        }
        for (key, next) in positions {
            if *next > 0 && !listed.contains(key) {
                found.push(Found::Expired {
                    stream: key.clone(),
                });
            }
        }
        Ok(found)
    }
}

/// `page` folded into `task`.
fn fold(task: &mut TaskAggregate, cover: &Cover, page: &EventPage) -> Result<(), AppError> {
    let payload = page.payload.clone().unwrap_or_default();
    let event = TaskEvent::from_any(&payload).map_err(|e| AppError::Failed(Report::of(&e)))?;
    let at = PageContext {
        cover: Some(cover),
        sequence: page.sequence,
        meta: page.meta.as_ref(),
    };
    TaskAggregate::apply(task, &event, &at);
    Ok(())
}

/// The task `aggregate` holds, as a watcher is told of it.
fn seen(aggregate: &TaskAggregate) -> Seen {
    let state = aggregate.state();
    let task = Task::of(state);
    Seen {
        harp_id: task.harp_id,
        text: task.text,
        status: task.status,
        tags: task.tags.iter().map(ToString::to_string).collect(),
        removed: state.removed,
    }
}

#[cfg(test)]
#[path = "watch.test.rs"]
mod tests;
