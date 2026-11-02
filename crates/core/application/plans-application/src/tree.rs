//! The work tree a project's plan files are in, beside the writes: a plan
//! viewed as its stream and its file are now, and a file's hash tracked
//! (recorded where it changed), which every use case that reads a plan's
//! file shares ([`Tree`]).

use plan_lists::PlanRow;
use plans::{PlanCommand, PlanRejection, PlanState, PlanStatus};
use ports::{Clock, CommandIds, Delay, Jitter, PlanFiles, Project, TaskStores};

use crate::rules::joined;
use crate::write::Writes;
use crate::{content_hash, frontmatter, plan_name, PlanError};

/// A plan as the use cases answer it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PlanView {
    pub harp_id: String,
    pub title: String,
    pub status: PlanStatus,
    /// As recorded: work-root-relative.
    pub path: String,
    /// Absolute: the work root joined with `path`.
    pub abs_path: String,
    /// [`plan_name`] of the path.
    pub name: String,
    /// The file's hash as last recorded: as it is now, after a read that
    /// found it changed.
    pub content_hash: String,
    /// The session it was added in.
    pub origin_session: String,
    /// The file's frontmatter `sessions:`, read now; empty when it has none
    /// or the file is missing.
    pub sessions: Vec<String>,
    pub governs: Vec<String>,
    pub supersedes: Vec<String>,
    pub superseded_by: String,
}

/// A change's way to its project and the kernel, and the work tree's
/// files: borrowed from the service for one call.
pub(crate) struct Tree<'s, I, S, F, C, D> {
    pub writes: Writes<'s, I, S, C, D>,
    pub files: &'s F,
}

/// A plan as the list holds one, from its stream's state.
fn row_of(state: &PlanState) -> PlanRow {
    PlanRow {
        harp_id: state.harp_id.clone(),
        title: state.title.clone(),
        status: state.status,
        path: state.path.clone(),
        content_hash: state.content_hash.clone(),
        origin_session: state.origin_session.clone(),
        governs: state.governs.iter().cloned().collect(),
        supersedes: state.supersedes.iter().cloned().collect(),
        superseded_by: state.superseded_by.clone(),
    }
}

/// The view of `row`, its file `text` as read now (None: missing), under
/// the work root `root`.
pub(crate) fn view_of(root: &str, row: &PlanRow, text: Option<&[u8]>) -> PlanView {
    let sessions = text
        .map(|text| frontmatter(&String::from_utf8_lossy(text)).sessions)
        .unwrap_or_default();
    PlanView {
        harp_id: row.harp_id.clone(),
        title: row.title.clone(),
        status: row.status,
        path: row.path.clone(),
        abs_path: joined(root, &row.path),
        name: plan_name(&row.path),
        content_hash: row.content_hash.clone(),
        origin_session: row.origin_session.clone(),
        sessions,
        governs: row.governs.clone(),
        supersedes: row.supersedes.clone(),
        superseded_by: row.superseded_by.clone(),
    }
}

impl<I, S, F: PlanFiles, C, D> Tree<'_, I, S, F, C, D> {
    /// The work root, which every plan path is relative to.
    pub(crate) fn work_root(&self) -> Result<String, PlanError> {
        self.files.work_root().map_err(PlanError::NoWorkRoot)
    }
}

impl<I, S: TaskStores, F: PlanFiles, C, D> Tree<'_, I, S, F, C, D> {
    /// The plan `harp_id`, viewed as its stream and file are now.
    pub(crate) async fn view(
        &mut self,
        project: &Project,
        harp_id: &str,
    ) -> Result<PlanView, PlanError> {
        let state = self
            .writes
            .projects
            .state(project, harp_id)
            .await?
            .ok_or_else(|| PlanError::NoSuchPlan(harp_id.to_string()))?;
        let root = self.work_root()?;
        let text = self.files.read(&state.path);
        Ok(view_of(&root, &row_of(&state), text.as_deref()))
    }
}

impl<I, S, F, C, D> Tree<'_, I, S, F, C, D>
where
    S: TaskStores,
    F: PlanFiles,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    /// `row`'s file read now, and its hash recorded where it changed (the
    /// row then carrying it): the file's text, or None when it is missing.
    pub(crate) async fn tracked(
        &mut self,
        project: &Project,
        row: &mut PlanRow,
    ) -> Result<Option<Vec<u8>>, PlanError> {
        let Some(text) = self.files.read(&row.path) else {
            return Ok(None);
        };
        let hash = content_hash(&text);
        if hash != row.content_hash {
            let rehash = PlanCommand::Rehash(hash.clone());
            match self.writes.commit(project, &row.harp_id, rehash).await {
                // Recorded meanwhile by another reader: the same hash.
                Ok(()) | Err(PlanError::Rejected(PlanRejection::HashUnchanged)) => {}
                Err(error) => return Err(error),
            }
            row.content_hash = hash;
        }
        Ok(Some(text))
    }
}
