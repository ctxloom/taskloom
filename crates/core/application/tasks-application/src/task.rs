//! A task's stream, as the use cases address and read it: its cover
//! (its harp is its natural key), the task it holds now, the task a
//! write left, and whether a command's events are on it; and
//! [`Written`], what a write answers with.

use angzarr_diagnostic::Report;
use angzarr_embedded::v1::event_meta::Causation;
use angzarr_embedded::v1::{Cover, Uuid};
use angzarr_embedded::{
    CoverExt, DeadLetter, Handled, Kernel, ReadStore, ReadUnit, Registry, Store, StreamRef,
};
use ports::Project;
use tasks::{TaskAggregate, TASK_DOMAIN};

use crate::rules::Finding;
use crate::{failed, AppError, Shown, Task};

/// A task written, the project it was written in, which of its tags a
/// person is shown, and the dead letters its kernel calls answered
/// (almost certainly bugs, for the caller to say loudly).
#[derive(Clone, PartialEq, Debug)]
pub struct Written {
    pub project: Project,
    pub task: Task,
    pub shown: Shown,
    pub dead_letters: Vec<DeadLetter>,
    /// tagma's `Existing` findings on the task written ([`Written::warnings`]).
    pub warnings: Vec<Finding>,
}

impl Written {
    /// tagma's `Existing` findings on the task written: what was wrong
    /// before the write and still is. A write is never refused for these
    /// (an `Introduced` finding refuses it, `tasks-application-tag-rules`),
    /// and the edge prints them on stderr, or in the JSON document.
    pub fn warnings(&self) -> &[Finding] {
        &self.warnings
    }
}

/// The stream of the task `harp_id` names: its harp is its natural key.
pub(crate) fn task_cover(harp_id: &str) -> Cover {
    Cover::natural(TASK_DOMAIN, harp_id)
}

/// Whether the stream of the task `harp_id` names holds an event
/// `command_id` caused, read in a read-only unit.
pub(crate) async fn caused<S: ReadStore>(
    store: &S,
    harp_id: &str,
    command_id: &Uuid,
) -> Result<bool, AppError> {
    let cover = task_cover(harp_id);
    let stream = StreamRef::try_from(&cover).map_err(failed)?;
    let mut reads = store.begin_read().await.map_err(failed)?;
    let slice = reads.read(stream, 0, None).await.map_err(failed)?;
    Ok(slice.pages.iter().any(|page| {
        matches!(
            page.meta.as_ref().and_then(|meta| meta.causation.as_ref()),
            Some(Causation::CommandId(id)) if id == command_id
        )
    }))
}

/// The task `harp_id` names, as the store holds it now ([`Kernel::load`]);
/// `None` when its stream holds no task.
pub(crate) async fn task_now<S: Store>(
    store: &S,
    registry: &Registry,
    harp_id: &str,
) -> Result<Option<Task>, AppError> {
    let (task, _) = Kernel::new(registry)
        .load::<TaskAggregate, _>(store, &task_cover(harp_id))
        .await
        .map_err(|e| AppError::Failed(Report::of(&e)))?;
    Ok(Task::existing(task.state().state()))
}

/// The task `harp_id` names as `handled` left it: from its commit itself,
/// never read after it; a duplicate's, which recorded nothing, as the
/// store holds it now ([`task_now`]). A stream holding no task after all
/// (one emptied since) is [`AppError::Vanished`].
pub(crate) async fn task_after<S: Store>(
    store: &S,
    registry: &Registry,
    harp_id: &str,
    handled: &Handled,
) -> Result<Task, AppError> {
    let committed = handled
        .state::<TaskAggregate>(&task_cover(harp_id), registry)
        .map_err(|e| AppError::Failed(Report::of(&e)))?;
    let task = match committed {
        Some(task) => Task::existing(task.state()),
        None => task_now(store, registry, harp_id).await?,
    };
    task.ok_or_else(|| AppError::Vanished {
        harp_id: harp_id.to_string(),
    })
}
