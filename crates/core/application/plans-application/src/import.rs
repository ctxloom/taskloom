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
use plans::{AddPlan, PlanCommand, PlanRejection, PlanStatus, RETIRED_TAG};
use ports::{
    Clock, CommandIds, Delay, Jitter, Mint, PlanFiles, Project, ProjectIdentity, TaskStores, Tasks,
};

use crate::rules::{locate, title_of, Located};
use crate::{
    content_hash, default_path, plan_name, ImportReport, ImportedPlan, LeftTag, PlanError,
    PlanService, PLANS_DIR, PLAN_SUFFIX,
};

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

impl<I, S, F, T, M, C, D> PlanService<I, S, F, T, M, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    F: PlanFiles,
    T: Tasks,
    M: Mint,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    pub(crate) async fn importing(&mut self, dry_run: bool) -> Result<ImportReport, PlanError> {
        let root = self.work_root()?;
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
        let project = match dry_run {
            true => self.for_read().await?,
            false => Some(self.for_write()?),
        };
        let list = match &project {
            Some(project) => self.plan_list(project).await?,
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
            let Some(text) = self.files.read(&path) else {
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
                    Some(project) => self.fresh_id(project).await?,
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
                match self.commit(project, &harp_id, govern).await {
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
                .files
                .create(&add.path, text)
                .map_err(PlanError::Files)?
        {
            return Err(PlanError::FileExists(add.path));
        }
        let harp_id = add.harp_id.clone();
        self.commit(project, &harp_id, PlanCommand::Add(add)).await
    }
}
