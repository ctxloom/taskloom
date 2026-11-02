//! `plan import`: the tasks' `ctxloom:plan="<path>"` tags turned into plans
//! and governs links, and retired.
//!
//! Each value is placed against the work root and grouped by the file it
//! names, however spelled. A readable `.plan.md` becomes one Active plan
//! governing every task tagged with it: inside the work root it is
//! registered where it is; outside it, it is copied to
//! `.taskloom/plans/<id>.plan.md` and the copy registered, the original
//! left as it was. Its title is its frontmatter's, else its heading, else
//! its name. The tags it migrates are removed, each after the links are
//! made, so an import stopped part way is finished by running it again. A
//! value naming no readable file, or a file that is no `.plan.md`, is
//! reported and its tags kept.
//!
//! Running it again adds nothing: a path already recorded is that plan's
//! (and an outside file whose copy is recorded, by its hash), so its tasks
//! join the links it has.

use std::collections::{BTreeMap, BTreeSet};

use plan_lists::PlanList;
use plans::{AddPlan, PlanCommand, PlanRejection, PlanStatus};
use ports::{
    Clock, CommandIds, Delay, Jitter, Mint, PlanFiles, Project, ProjectIdentity, TaskStores, Tasks,
};
use tasks_application::RETIRED_TAG;

use crate::rules::{locate, title_of, Located};
use crate::tree::Tree;
use crate::{content_hash, default_path, plan_name, PlanError, PLANS_DIR, PLAN_SUFFIX};

/// One `ctxloom:plan` value `plan import` registered (or found already
/// registered) and the tasks it now governs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImportedPlan {
    pub harp_id: String,
    /// The path the plan records: the value's, made work-root-relative, or
    /// for a plan outside the work root its copy's under [`PLANS_DIR`].
    pub path: String,
    /// The value named a file outside the work root, copied in (the
    /// original is left as it was).
    pub external: bool,
    /// Added by this import (false: a plan already recording the path).
    pub new: bool,
    /// Every task the plan governs after the import, by harp id.
    pub governs: Vec<String>,
}

/// A `ctxloom:plan` value `plan import` left as it was, and the tasks
/// carrying it (their tag kept, so nothing is lost).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LeftTag {
    pub value: String,
    pub tasks: Vec<String>,
}

/// `plan import`'s report.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ImportReport {
    pub plans: Vec<ImportedPlan>,
    /// Values naming no readable file.
    pub dead: Vec<LeftTag>,
    /// Values naming a file that is not a `.plan.md`.
    pub not_plans: Vec<LeftTag>,
    /// (task, value): the tags removed, each now a governs link.
    pub retired: Vec<(String, String)>,
}

impl ImportReport {
    /// Whether a value was left behind (dead, or not a plan): `plan
    /// import` exits 1, so a script sees what was left.
    pub fn left_any(&self) -> bool {
        !self.dead.is_empty() || !self.not_plans.is_empty()
    }
}

/// An import over the ports it needs: the project, the kernel and the work
/// tree ([`Tree`]), the tasks whose tags it reads and retires, and the
/// mint a new plan's id is drawn from. Borrowed from the service for one
/// call.
pub(crate) struct Importing<'s, I, S, F, T, M, C, D> {
    pub tree: Tree<'s, I, S, F, C, D>,
    pub tasks: &'s mut T,
    pub mint: &'s M,
}

/// The values naming one readable plan file, and its text.
struct Group {
    text: Vec<u8>,
    /// (task, value).
    tags: BTreeSet<(String, String)>,
}

/// Values left as they were, each with the tasks carrying it.
type Left = BTreeMap<String, BTreeSet<String>>;

/// The values of the retired tag, by what they name.
#[derive(Default)]
struct Sorted {
    /// By (outside the work root, the file's path): inside ones first.
    groups: BTreeMap<(bool, String), Group>,
    dead: Left,
    not_plans: Left,
}

fn left(left: Left) -> Vec<LeftTag> {
    left.into_iter()
        .map(|(value, tasks)| LeftTag {
            value,
            tasks: tasks.into_iter().collect(),
        })
        .collect()
}

impl<I, S, F, T, M, C, D> Importing<'_, I, S, F, T, M, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    F: PlanFiles,
    T: Tasks,
    M: Mint,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    pub(crate) async fn import(&mut self, dry_run: bool) -> Result<ImportReport, PlanError> {
        let root = self.tree.work_root()?;
        let tagged = self
            .tasks
            .tagged(RETIRED_TAG)
            .await
            .map_err(PlanError::Tasks)?;
        let Sorted {
            groups,
            dead,
            not_plans,
        } = self.sorted(&root, tagged);
        let projects = &mut self.tree.writes.projects;
        let project = match dry_run {
            true => projects.for_read().await?,
            false => Some(projects.for_write()?),
        };
        let list = match &project {
            Some(project) => projects.plan_list(project).await?,
            None => PlanList::new(),
        };
        let mut report = ImportReport {
            dead: left(dead),
            not_plans: left(not_plans),
            ..ImportReport::default()
        };
        for ((external, path), group) in groups {
            let imported = self
                .imported(&list, project.as_ref(), dry_run, external, &path, &group)
                .await?;
            report.plans.push(imported);
            report.retired.extend(group.tags);
        }
        report.retired.sort();
        Ok(report)
    }

    /// The values `tagged` by the files they name: each readable plan's,
    /// and those left, naming no readable file or none that is a plan.
    fn sorted(&self, root: &str, tagged: Vec<(String, String)>) -> Sorted {
        let mut sorted = Sorted::default();
        for (task, value) in tagged {
            let (external, path) = match locate(root, &value) {
                Located::Inside(path) => (false, path),
                Located::Outside(path) => (true, path),
            };
            let Some(text) = self.tree.files.read(&path) else {
                sorted.dead.entry(value).or_default().insert(task);
                continue;
            };
            if !path.ends_with(PLAN_SUFFIX) {
                sorted.not_plans.entry(value).or_default().insert(task);
                continue;
            }
            sorted
                .groups
                .entry((external, path))
                .or_insert_with(|| Group {
                    text,
                    tags: BTreeSet::new(),
                })
                .tags
                .insert((task, value));
        }
        sorted
    }

    /// The plan the file at `path` (outside the work root if `external`)
    /// is, registered unless it is already, governing `group`'s tasks, its
    /// tags removed; on a dry run (or with no `project`), only reported.
    async fn imported(
        &mut self,
        list: &PlanList,
        project: Option<&Project>,
        dry_run: bool,
        external: bool,
        path: &str,
        group: &Group,
    ) -> Result<ImportedPlan, PlanError> {
        let hash = content_hash(&group.text);
        let copies = format!("{PLANS_DIR}/");
        let recorded = match external {
            false => list.by_path(path),
            true => list
                .plans()
                .into_iter()
                .find(|row| row.content_hash == hash && row.path.starts_with(&copies)),
        };
        let tasks: BTreeSet<String> = group.tags.iter().map(|(t, _)| t.clone()).collect();
        let (harp_id, registered, new, mut governs) = match recorded {
            Some(row) => (
                row.harp_id.clone(),
                row.path.clone(),
                false,
                row.governs.iter().cloned().collect::<BTreeSet<_>>(),
            ),
            None => {
                // A dry run's ids are not reserved: a run mints its own.
                let harp_id = match project {
                    Some(project) => {
                        let projects = &mut self.tree.writes.projects;
                        projects.fresh_id(self.mint, project).await?
                    }
                    None => self.mint.mint(),
                };
                let registered = match external {
                    true => default_path(&harp_id),
                    false => path.to_string(),
                };
                (harp_id, registered, true, BTreeSet::new())
            }
        };
        if let (false, Some(project)) = (dry_run, project) {
            if new {
                let text = String::from_utf8_lossy(&group.text);
                let add = AddPlan {
                    harp_id: harp_id.clone(),
                    title: title_of(&text, &plan_name(path)),
                    status: PlanStatus::Active,
                    path: registered.clone(),
                    content_hash: hash,
                    origin_session: String::new(),
                };
                self.register(project, external, add, &group.text).await?;
            }
            for task in tasks.iter().filter(|task| !governs.contains(*task)) {
                let govern = PlanCommand::Govern(task.clone());
                match self.tree.writes.commit(project, &harp_id, govern).await {
                    Ok(()) | Err(PlanError::Rejected(PlanRejection::AlreadyGoverns(_))) => {}
                    Err(error) => return Err(error),
                }
            }
            for (task, value) in &group.tags {
                self.tasks
                    .untag(task, RETIRED_TAG, value)
                    .await
                    .map_err(PlanError::Tasks)?;
            }
        }
        governs.extend(tasks);
        Ok(ImportedPlan {
            harp_id,
            path: registered,
            external,
            new,
            governs: governs.into_iter().collect(),
        })
    }

    /// Add the plan `add` names, its file copied in first when it is
    /// `external` (never over a file already there).
    async fn register(
        &mut self,
        project: &Project,
        external: bool,
        add: AddPlan,
        text: &[u8],
    ) -> Result<(), PlanError> {
        if external
            && !self
                .tree
                .files
                .create(&add.path, text)
                .map_err(PlanError::Files)?
        {
            return Err(PlanError::FileExists(add.path));
        }
        let harp_id = add.harp_id.clone();
        let add = PlanCommand::Add(add);
        self.tree.writes.commit(project, &harp_id, add).await
    }
}
