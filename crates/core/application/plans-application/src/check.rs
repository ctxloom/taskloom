//! `plan check`: drift between the store and the work tree (or the
//! project's tasks) that tracking a file's hash cannot mend
//! ([`Checking`]).

use ports::{Clock, CommandIds, Delay, Jitter, PlanFiles, ProjectIdentity, TaskStores, Tasks};

use crate::tree::Tree;
use crate::{PlanError, PLANS_DIR};

/// What `plan check` found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Finding {
    /// The recorded file is not there.
    Missing { plan: String, path: String },
    /// A governed task the project no longer holds (pruned).
    MissingTask { plan: String, task: String },
    /// A `.taskloom/plans/*.plan.md` no plan records.
    Unregistered { path: String },
}

/// `plan check`'s answer: by plan, its missing file then its governed
/// tasks gone; then the unregistered files by path. Every finding fails the
/// check.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CheckReport {
    pub findings: Vec<Finding>,
}

impl CheckReport {
    /// Whether anything was found: `plan check` exits 1.
    pub fn failed(&self) -> bool {
        !self.findings.is_empty()
    }
}

/// A check over the ports it needs: the project, the kernel (it records a
/// file's new hash, as a read does) and the work tree ([`Tree`]), and the
/// tasks a plan governs. Borrowed from the service for one call.
pub(crate) struct Checking<'s, I, S, F, T, C, D> {
    pub tree: Tree<'s, I, S, F, C, D>,
    pub tasks: &'s mut T,
}

impl<I, S, F, T, C, D> Checking<'_, I, S, F, T, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    F: PlanFiles,
    T: Tasks,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    pub(crate) async fn check(&mut self) -> Result<CheckReport, PlanError> {
        let mut findings = Vec::new();
        let mut recorded = Vec::new();
        if let Some(project) = self.tree.writes.projects.for_read().await? {
            let list = self.tree.writes.projects.plan_list(&project).await?;
            for row in list.plans() {
                let mut row = row.clone();
                recorded.push(row.path.clone());
                if self.tree.tracked(&project, &mut row).await?.is_none() {
                    findings.push(Finding::Missing {
                        plan: row.harp_id.clone(),
                        path: row.path.clone(),
                    });
                }
                for task in &row.governs {
                    if !self.tasks.exists(task).await.map_err(PlanError::Tasks)? {
                        findings.push(Finding::MissingTask {
                            plan: row.harp_id.clone(),
                            task: task.clone(),
                        });
                    }
                }
            }
        }
        for path in self.tree.files.plan_files(PLANS_DIR) {
            if !recorded.contains(&path) {
                findings.push(Finding::Unregistered { path });
            }
        }
        Ok(CheckReport { findings })
    }
}
