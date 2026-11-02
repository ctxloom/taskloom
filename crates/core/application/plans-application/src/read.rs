//! `plan list` and `plan show`: the plans as the list holds them, each
//! file read now and its new hash recorded where it changed ([`Reading`]).

use plan_lists::PlanRow;
use plans::PlanStatus;
use ports::{Clock, CommandIds, Delay, Jitter, PlanFiles, ProjectIdentity, TaskStores};

use crate::rules::{locate, Located};
use crate::tree::{view_of, Tree};
use crate::{PlanError, PlanView};

/// `plan list`'s filters.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PlanQuery {
    /// Any of these; empty is every status.
    pub statuses: Vec<PlanStatus>,
    /// Only plans governing this task.
    pub governs: Option<String>,
}

/// `plan show`: the plan and its text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlanText {
    pub plan: PlanView,
    /// None when the file is missing.
    pub content: Option<String>,
}

/// The reads over the ports they need: the project, the kernel (a read
/// records a file's new hash) and the work tree ([`Tree`]). Borrowed from
/// the service for one call.
pub(crate) struct Reading<'s, I, S, F, C, D> {
    pub tree: Tree<'s, I, S, F, C, D>,
}

impl<I, S, F, C, D> Reading<'_, I, S, F, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    F: PlanFiles,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    pub(crate) async fn list(&mut self, query: &PlanQuery) -> Result<Vec<PlanView>, PlanError> {
        let Some(project) = self.tree.writes.projects.for_read().await? else {
            return Ok(Vec::new());
        };
        let root = self.tree.work_root()?;
        let list = self.tree.writes.projects.plan_list(&project).await?;
        let selected: Vec<PlanRow> = match &query.governs {
            Some(task) => list.governing(task),
            None => list.plans(),
        }
        .into_iter()
        .filter(|row| query.statuses.is_empty() || query.statuses.contains(&row.status))
        .cloned()
        .collect();
        let mut views = Vec::new();
        for mut row in selected {
            let text = self.tree.tracked(&project, &mut row).await?;
            views.push(view_of(&root, &row, text.as_deref()));
        }
        Ok(views)
    }

    pub(crate) async fn show(&mut self, id_or_path: &str) -> Result<PlanText, PlanError> {
        let missing = || PlanError::NoSuchPlan(id_or_path.to_string());
        let project = self.tree.writes.projects.for_read().await?;
        let project = project.ok_or_else(missing)?;
        let root = self.tree.work_root()?;
        let list = self.tree.writes.projects.plan_list(&project).await?;
        let found = list
            .get(id_or_path)
            .or_else(|| match locate(&root, id_or_path) {
                Located::Inside(path) => list.by_path(&path),
                Located::Outside(_) => None,
            });
        let mut row = found.cloned().ok_or_else(missing)?;
        let text = self.tree.tracked(&project, &mut row).await?;
        Ok(PlanText {
            plan: view_of(&root, &row, text.as_deref()),
            content: text.map(|text| String::from_utf8_lossy(&text).into_owned()),
        })
    }
}
