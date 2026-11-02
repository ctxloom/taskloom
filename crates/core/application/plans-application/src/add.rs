//! `plan add`: a plan registered, its file the one named or a new one at
//! [`default_path`], governing the tasks it names ([`Adding`]).

use plans::{AddPlan, PlanCommand, PlanStatus};
use ports::{
    Clock, CommandIds, Delay, Jitter, Mint, PlanFiles, ProjectIdentity, TaskStores, Tasks,
};

use crate::rules::new_plan_text;
use crate::tree::Tree;
use crate::{content_hash, default_path, resolve_path, PlanError, PlanView};

/// `plan add`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct NewPlan {
    pub title: String,
    /// Register this file; None writes a new one at [`default_path`].
    pub path: Option<String>,
    pub status: PlanStatus,
    pub governs: Vec<String>,
    /// CTXLOOM_SESSION_HARP.
    pub session: String,
}

/// An add over the ports it needs: the project, the kernel and the work
/// tree ([`Tree`]), the tasks it may govern and the mint its id is drawn
/// from. Borrowed from the service for one call.
pub(crate) struct Adding<'s, I, S, F, T, M, C, D> {
    pub tree: Tree<'s, I, S, F, C, D>,
    pub tasks: &'s mut T,
    pub mint: &'s M,
}

impl<I, S, F, T, M, C, D> Adding<'_, I, S, F, T, M, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    F: PlanFiles,
    T: Tasks,
    M: Mint,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    pub(crate) async fn add(&mut self, new: NewPlan) -> Result<PlanView, PlanError> {
        let mut governs = new.governs.clone();
        governs.sort();
        governs.dedup();
        // What the add carries alone, refused before anything is written.
        let template = AddPlan {
            harp_id: String::new(),
            title: new.title.trim().to_string(),
            status: new.status,
            path: String::new(),
            content_hash: content_hash(b""),
            origin_session: new.session.clone(),
        };
        plans::check(&PlanCommand::Add(template.clone())).map_err(PlanError::Rejected)?;
        for task in &governs {
            if !self.tasks.exists(task).await.map_err(PlanError::Tasks)? {
                return Err(PlanError::NoSuchTask(task.clone()));
            }
        }
        let root = self.tree.work_root()?;
        let files = self.tree.files;
        let projects = &mut self.tree.writes.projects;
        let (harp_id, path, hash, project) = match &new.path {
            Some(given) => {
                let path = resolve_path(&root, given).map_err(PlanError::Path)?;
                let text = files
                    .read(&path)
                    .ok_or_else(|| PlanError::FileMissing(path.clone()))?;
                let project = projects.for_write()?;
                if let Some(row) = projects.plan_list(&project).await?.by_path(&path) {
                    return Err(PlanError::AlreadyRegistered {
                        path,
                        plan: row.harp_id.clone(),
                    });
                }
                let harp_id = projects.fresh_id(self.mint, &project).await?;
                (harp_id, path, content_hash(&text), project)
            }
            None => {
                let project = projects.for_write()?;
                let harp_id = projects.fresh_id(self.mint, &project).await?;
                let path = default_path(&harp_id);
                let text = new_plan_text(&template.title, &new.session);
                if !files
                    .create(&path, text.as_bytes())
                    .map_err(PlanError::Files)?
                {
                    return Err(PlanError::FileExists(path));
                }
                (harp_id, path, content_hash(text.as_bytes()), project)
            }
        };
        let add = AddPlan {
            harp_id: harp_id.clone(),
            path,
            content_hash: hash,
            ..template
        };
        let writes = &mut self.tree.writes;
        writes
            .commit(&project, &harp_id, PlanCommand::Add(add))
            .await?;
        for task in governs {
            writes
                .commit(&project, &harp_id, PlanCommand::Govern(task))
                .await?;
        }
        self.tree.view(&project, &harp_id).await
    }
}
