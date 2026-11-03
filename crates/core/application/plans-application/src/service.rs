//! [`PlanService`]: the ports the use cases run over, owned once, and the
//! plans use cases behind the trait a driving adapter calls ([`Plans`]).
//!
//! A plan is a `*.plan.md` file under the plan roots: `plan_roots` from the
//! configuration (each relative one the work root's), which replaces the
//! defaults, the work tree and `.taskloom/plans`. Its frontmatter is the
//! one authority for everything about it; the store keeps no record. A
//! command is a read of the files, and for a change one write of one file,
//! through [`crate::front`]'s editor, which touches no byte but the
//! edited item (and a minted `id:` on the file's first write).

use std::collections::BTreeMap;
use std::future::Future;

use angzarr_diagnostic::Report;
use ports::{Configuration, Mint, PlanFiles, Tasks};
use tag_rules::rules::StoreRules;
use tag_rules::tags::{self, Tag, TagRefusal};
use tag_rules::{judged, RulesRefusal};
use tagma_core::semver::{Semver, SEMVER};

use crate::front::{self, Edit};
use crate::plan::{derive, Plan, STATUSES, SUPERSEDED};
use crate::watch::{Snapshot, Watched};
use crate::{
    CheckReport, Finding, Link, NewPlan, PlanError, PlanQuery, PlanText, SetPlan, Stamp, PLANS_DIR,
    PLAN_SUFFIX,
};

/// The plans use cases, as a driving adapter reaches them. Their futures
/// are not promised `Send` (see [`ports::Tasks`]).
pub trait Plans {
    /// The plans `query` selects, in the order their files are found.
    fn list(&mut self, query: &PlanQuery) -> impl Future<Output = Result<Vec<Plan>, PlanError>>;
    /// The plan `plan` names, an id or a path, and its file's text.
    fn show(&mut self, plan: &str) -> impl Future<Output = Result<PlanText, PlanError>>;
    /// A new plan file.
    fn add(&mut self, new: NewPlan) -> impl Future<Output = Result<Plan, PlanError>>;
    /// `plan`'s title or status.
    fn set(&mut self, plan: &str, change: SetPlan)
        -> impl Future<Output = Result<Plan, PlanError>>;
    /// `tag` added to, or (`add` false) removed from, `plan`'s tags.
    fn tag(
        &mut self,
        plan: &str,
        tag: &str,
        add: bool,
    ) -> impl Future<Output = Result<Plan, PlanError>>;
    /// `link` made (`linking`), or taken away, on `plan`.
    fn link(
        &mut self,
        plan: &str,
        link: Link,
        linking: bool,
    ) -> impl Future<Output = Result<Plan, PlanError>>;
    /// Every plan file checked, and what is wrong with each named.
    fn check(&mut self) -> impl Future<Output = Result<CheckReport, PlanError>>;
    /// `session` recorded in the `sessions:` of the plan file at `path`.
    fn stamp(&mut self, path: &str, session: &str) -> Result<Stamp, PlanError>;
    /// Every plan file's metadata and body as they are now, for a watch
    /// to compare.
    fn snapshot(&mut self) -> Result<Snapshot, PlanError>;
}

/// The ports the use cases run over: `files` reach the plan files, `tasks`
/// the project's tasks and rules, `mint` makes plan ids, `config` says
/// where the plan files are.
pub struct PlanService<F, T, M, G> {
    pub files: F,
    pub tasks: T,
    pub mint: M,
    pub config: G,
}

impl<F, T, M, G> PlanService<F, T, M, G> {
    pub fn new(files: F, tasks: T, mint: M, config: G) -> Self {
        PlanService {
            files,
            tasks,
            mint,
            config,
        }
    }
}

/// How many times a freshly minted id is tried against the ids in use.
pub const MINT_ATTEMPTS: u32 = 100;

/// Every plan file found, read, and those found that could not be.
struct Loaded {
    plans: Vec<Plan>,
    /// The text each plan was read as, by path.
    texts: BTreeMap<String, String>,
    unread: Vec<String>,
}

impl Loaded {
    fn empty() -> Loaded {
        Loaded {
            plans: Vec::new(),
            texts: BTreeMap::new(),
            unread: Vec::new(),
        }
    }

    fn ids(&self) -> Vec<&str> {
        self.plans.iter().filter_map(|p| p.id.as_deref()).collect()
    }

    /// `plan` as it is now, replacing the one read at its path.
    fn replaced(&mut self, plan: Plan, text: String) {
        self.texts.insert(plan.path.clone(), text);
        match self.plans.iter_mut().find(|p| p.path == plan.path) {
            Some(slot) => *slot = plan,
            None => self.plans.push(plan),
        }
        derive(&mut self.plans);
    }
}

/// `path` with `.` and `..` resolved and `/` for separators.
fn normalized(path: &str) -> String {
    let path = path.replace('\\', "/");
    let (prefix, rest) = match path.as_bytes() {
        [b'/', ..] => ("/", &path[1..]),
        [_, b':', b'/', ..] => (&path[..3], &path[3..]),
        _ => ("", path.as_str()),
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    format!("{prefix}{}", parts.join("/"))
}

/// Whether `path` is absolute: rooted, or on a drive.
fn is_absolute(path: &str) -> bool {
    matches!(
        path.as_bytes(),
        [b'/' | b'\\', ..] | [_, b':', b'/' | b'\\', ..]
    )
}

/// `path` placed against `root` unless it is absolute.
fn placed(root: Option<&str>, path: &str) -> Option<String> {
    match (is_absolute(path), root) {
        (true, _) => Some(normalized(path)),
        (false, Some(root)) => Some(normalized(&format!("{root}/{path}"))),
        (false, None) => None,
    }
}

impl<F, T, M, G> PlanService<F, T, M, G>
where
    F: PlanFiles,
    T: Tasks,
    M: Mint,
    G: Configuration,
{
    fn work_root(&self) -> Result<String, PlanError> {
        self.files.work_root().map_err(PlanError::NoWorkRoot)
    }

    /// `path` as an absolute path: placed against the work root when it is
    /// relative.
    fn place(&self, path: &str) -> Result<String, PlanError> {
        let root = match is_absolute(path) {
            true => None,
            false => Some(self.work_root()?),
        };
        Ok(placed(root.as_deref(), path).expect("a relative path has a root"))
    }

    /// The configured roots, as written; none when the defaults apply.
    fn configured(&mut self) -> Result<Option<Vec<String>>, PlanError> {
        self.config.plan_roots().map_err(PlanError::Config)
    }

    /// The directories plan files are searched for in: the configured
    /// roots, else the work tree and `.taskloom/plans`.
    fn roots(&mut self) -> Result<Vec<String>, PlanError> {
        match self.configured()? {
            Some(roots) => roots.iter().map(|root| self.place(root)).collect(),
            None => {
                let root = self.work_root()?;
                Ok(vec![
                    normalized(&root),
                    normalized(&format!("{root}/{PLANS_DIR}")),
                ])
            }
        }
    }

    /// Where `plans add` puts a plan: the first configured root, else
    /// `.taskloom/plans`.
    fn home(&mut self) -> Result<String, PlanError> {
        match self
            .configured()?
            .and_then(|roots| roots.into_iter().next())
        {
            Some(root) => self.place(&root),
            None => self.place(PLANS_DIR),
        }
    }

    /// Every plan file under the roots, each once, read.
    fn load(&mut self) -> Result<Loaded, PlanError> {
        let mut paths: Vec<String> = Vec::new();
        for root in self.roots()? {
            paths.extend(self.files.scan(&root));
        }
        paths.sort();
        paths.dedup();
        let mut loaded = Loaded::empty();
        for path in paths {
            match self.files.read(&path) {
                Some(text) => {
                    loaded.plans.push(Plan::parse(&path, &text));
                    loaded.texts.insert(path, text);
                }
                None => loaded.unread.push(path),
            }
        }
        derive(&mut loaded.plans);
        Ok(loaded)
    }

    /// The plan `name` is, an id or a path, and the text of its file.
    fn resolve(&self, loaded: &Loaded, name: &str) -> Result<(Plan, String), PlanError> {
        let by_id: Vec<&Plan> = loaded
            .plans
            .iter()
            .filter(|p| p.id.as_deref() == Some(name))
            .collect();
        match by_id.as_slice() {
            [plan] => {
                let text = loaded.texts[&plan.path].clone();
                return Ok(((*plan).clone(), text));
            }
            [] => {}
            many => {
                return Err(PlanError::Ambiguous {
                    id: name.to_string(),
                    paths: many
                        .iter()
                        .map(|p| p.path.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                })
            }
        }
        let path = self.place(name)?;
        if let Some(plan) = loaded.plans.iter().find(|p| p.path == path) {
            return Ok((plan.clone(), loaded.texts[&path].clone()));
        }
        // A plan file outside the roots, named by its path.
        let text = self
            .files
            .read(&path)
            .ok_or_else(|| PlanError::NoSuchPlan(name.to_string()))?;
        let mut plans = loaded.plans.clone();
        plans.push(Plan::parse(&path, &text));
        derive(&mut plans);
        let plan = plans.pop().expect("just pushed");
        Ok((plan, text))
    }

    /// A harp no plan in `used` has.
    fn fresh_id(&self, used: &[&str]) -> Result<String, PlanError> {
        (0..MINT_ATTEMPTS)
            .map(|_| self.mint.mint())
            .find(|id| !used.contains(&id.as_str()))
            .ok_or(PlanError::HarpsExhausted {
                attempts: MINT_ATTEMPTS,
            })
    }

    /// `plan`'s file with `edits` made, an id minted if it has none:
    /// written only if the text changed. The plan as the file now is.
    fn write(
        &mut self,
        loaded: &mut Loaded,
        plan: &Plan,
        text: &str,
        edits: &[Edit],
    ) -> Result<Plan, PlanError> {
        let id = match plan.id {
            Some(_) => None,
            None => Some(self.fresh_id(&loaded.ids())?),
        };
        let edited =
            front::edited(text, id.as_deref(), edits).map_err(|error| PlanError::Frontmatter {
                path: plan.path.clone(),
                error,
            })?;
        if edited != text {
            self.files
                .replace(&plan.path, &edited)
                .map_err(PlanError::Files)?;
        }
        let mut written = Plan::parse(&plan.path, &edited);
        loaded.replaced(written.clone(), edited);
        written = loaded
            .plans
            .iter()
            .find(|p| p.path == plan.path)
            .cloned()
            .unwrap_or(written);
        Ok(written)
    }

    /// The store's rules, as tagma reads them.
    async fn rules(&mut self) -> Result<StoreRules, PlanError> {
        let spelled = self.tasks.rules().await.map_err(PlanError::Tasks)?;
        Ok(StoreRules::of_tags(
            spelled
                .iter()
                .filter_map(|s| tagma_core::Tag::parse(s).ok()),
        ))
    }

    async fn task_exists(&mut self, task: &str) -> Result<bool, PlanError> {
        self.tasks.exists(task).await.map_err(PlanError::Tasks)
    }
}

/// `spelled`, a tag as the person wrote it, parsed.
fn parse_tag(spelled: &str) -> Result<Tag, PlanError> {
    tags::parse(spelled)
        .unwrap_or(Err(TagRefusal::Blank))
        .map_err(PlanError::Tag)
}

/// The tags in `spelled` that parse, skipping the ones that do not.
fn parsed_tags(spelled: &[String]) -> Vec<Tag> {
    spelled
        .iter()
        .filter_map(|s| tags::parse(s)?.ok())
        .collect()
}

/// `status`, if it is a status a plan may be set to.
fn settable(status: &str) -> Result<&str, PlanError> {
    match status {
        SUPERSEDED => Err(PlanError::HandSuperseded),
        s if STATUSES.contains(&s) => Ok(s),
        s => Err(PlanError::UnknownStatus {
            status: s.to_string(),
        }),
    }
}

/// The ids reached from `from` by `supersedes:`, through `plans`.
fn supersedes_reach(plans: &[Plan], from: &str) -> Vec<String> {
    let mut seen: Vec<String> = vec![from.to_string()];
    let mut at = 0;
    while at < seen.len() {
        let id = seen[at].clone();
        for plan in plans.iter().filter(|p| p.id.as_deref() == Some(&id)) {
            for target in &plan.supersedes {
                if !seen.contains(target) {
                    seen.push(target.clone());
                }
            }
        }
        at += 1;
    }
    seen
}

/// The ids of `plans` a tag query selects under `rules`.
fn select(query: &str, plans: &[&Plan], rules: &StoreRules) -> Result<Vec<String>, PlanError> {
    let mut index = tagma_core::Index::new();
    index.register_type(SEMVER, std::sync::Arc::new(Semver));
    let types = rules.types();
    if !types.is_empty() {
        index.add_item("taskloom.internal:type-config", types);
    }
    let presence = tagma_core::Tag {
        namespace: Some("taskloom.internal".to_string()),
        key: "candidate".to_string(),
        value: None,
    };
    for plan in plans {
        let mut held: Vec<tagma_core::Tag> = plan
            .tags
            .iter()
            .filter_map(|t| tagma_core::Tag::parse(t).ok())
            .collect();
        held.push(presence.clone());
        index.add_item(&plan.path, held);
    }
    let matched = index
        .query_postfix(query)
        .map_err(|postfix| PlanError::TagQuery {
            query: query.to_string(),
            postfix,
        })?;
    Ok(matched.into_iter().collect())
}

impl<F, T, M, G> Plans for PlanService<F, T, M, G>
where
    F: PlanFiles,
    T: Tasks,
    M: Mint,
    G: Configuration,
{
    async fn list(&mut self, query: &PlanQuery) -> Result<Vec<Plan>, PlanError> {
        for status in &query.statuses {
            if status != SUPERSEDED && !STATUSES.contains(&status.as_str()) {
                return Err(PlanError::UnknownStatus {
                    status: status.clone(),
                });
            }
        }
        let rules = match &query.tag_query {
            Some(_) => self.rules().await?,
            None => StoreRules::default(),
        };
        let loaded = self.load()?;
        let mut chosen: Vec<&Plan> = loaded
            .plans
            .iter()
            .filter(|p| query.statuses.is_empty() || query.statuses.contains(&p.status))
            .filter(|p| query.governs.as_ref().is_none_or(|g| p.governs.contains(g)))
            .collect();
        if let Some(tag_query) = &query.tag_query {
            let paths = select(tag_query, &chosen, &rules)?;
            chosen.retain(|p| paths.contains(&p.path));
        }
        Ok(chosen.into_iter().cloned().collect())
    }

    async fn show(&mut self, plan: &str) -> Result<PlanText, PlanError> {
        let loaded = self.load()?;
        let (plan, content) = self.resolve(&loaded, plan)?;
        Ok(PlanText { plan, content })
    }

    async fn add(&mut self, new: NewPlan) -> Result<Plan, PlanError> {
        let title = new.title.trim().to_string();
        if title.is_empty() {
            return Err(PlanError::BlankTitle);
        }
        let status = match new.status.as_deref() {
            Some(status) => settable(status)?.to_string(),
            None => crate::plan::DEFAULT_STATUS.to_string(),
        };
        if let Some(path) = &new.path {
            if !path.ends_with(PLAN_SUFFIX) {
                return Err(PlanError::NotAPlanPath { path: path.clone() });
            }
        }
        for task in &new.governs {
            if !self.task_exists(task).await? {
                return Err(PlanError::NoSuchTask(task.clone()));
            }
        }
        let loaded = self.load()?;
        let id = self.fresh_id(&loaded.ids())?;
        let path = match &new.path {
            Some(path) => self.place(path)?,
            None => format!("{}/{id}{PLAN_SUFFIX}", self.home()?),
        };
        let mut entries = vec![
            ("id", angzarr_yaml::Value::scalar(id)),
            ("title", angzarr_yaml::Value::scalar(title.clone())),
            ("status", angzarr_yaml::Value::scalar(status)),
        ];
        if !new.governs.is_empty() {
            entries.push((
                "governs",
                angzarr_yaml::Value::Seq(
                    new.governs
                        .iter()
                        .map(|g| angzarr_yaml::Value::scalar(g.as_str()))
                        .collect(),
                ),
            ));
        }
        let text = front::fresh(entries, &format!("\n# {title}\n"));
        match self.files.create(&path, &text).map_err(PlanError::Files)? {
            true => Ok(Plan::parse(&path, &text)),
            false => Err(PlanError::FileExists { path }),
        }
    }

    async fn set(&mut self, plan: &str, change: SetPlan) -> Result<Plan, PlanError> {
        let mut edits = Vec::new();
        if let Some(title) = &change.title {
            let title = title.trim();
            if title.is_empty() {
                return Err(PlanError::BlankTitle);
            }
            edits.push(Edit::Set("title", title.to_string()));
        }
        if let Some(status) = &change.status {
            edits.push(Edit::Set("status", settable(status)?.to_string()));
        }
        if edits.is_empty() {
            return Err(PlanError::NoChange);
        }
        let mut loaded = self.load()?;
        let (found, text) = self.resolve(&loaded, plan)?;
        self.write(&mut loaded, &found, &text, &edits)
    }

    async fn tag(&mut self, plan: &str, tag: &str, add: bool) -> Result<Plan, PlanError> {
        let tag = parse_tag(tag)?;
        if add {
            tags::admit(&tag).map_err(PlanError::Tag)?;
        }
        let mut loaded = self.load()?;
        let (found, text) = self.resolve(&loaded, plan)?;
        let spelled = tag.to_string();
        let held = parsed_tags(&found.tags);
        let at = held.iter().position(|t| *t == tag);
        let edit = match (add, at) {
            (true, Some(_)) | (false, None) => return Ok(found),
            (true, None) => {
                let rules = self.rules().await?;
                judged(rules.validate(&held, std::slice::from_ref(&tag), &[]))
                    .map_err(|refusal: RulesRefusal| PlanError::Rules(refusal))?;
                Edit::Push("tags", spelled)
            }
            (false, Some(_)) => {
                // The item as the file spells it, which may not be the
                // canonical spelling.
                let written = found
                    .tags
                    .iter()
                    .find(|t| tags::parse(t).and_then(Result::ok).as_ref() == Some(&tag))
                    .cloned()
                    .unwrap_or(spelled);
                Edit::Remove("tags", written)
            }
        };
        self.write(&mut loaded, &found, &text, &[edit])
    }

    async fn link(&mut self, plan: &str, link: Link, linking: bool) -> Result<Plan, PlanError> {
        let mut loaded = self.load()?;
        let (found, text) = self.resolve(&loaded, plan)?;
        let edit = match (link, linking) {
            (Link::Governs(task), true) => {
                if found.governs.contains(&task) {
                    return Ok(found);
                }
                if !self.task_exists(&task).await? {
                    return Err(PlanError::NoSuchTask(task));
                }
                Edit::Push("governs", task)
            }
            (Link::Governs(task), false) => Edit::Remove("governs", task),
            (Link::Supersedes(target), true) => {
                let (target, _) = self.resolve(&loaded, &target)?;
                let same = target.path == found.path;
                let label = found.id.clone().unwrap_or_else(|| found.path.clone());
                if same {
                    return Err(PlanError::SupersedeSelf { plan: label });
                }
                let target_id = target.id.clone().ok_or(PlanError::NoId {
                    path: target.path.clone(),
                })?;
                if found.supersedes.contains(&target_id) {
                    return Ok(found);
                }
                if let Some(id) = &found.id {
                    if supersedes_reach(&loaded.plans, &target_id).contains(id) {
                        return Err(PlanError::SupersedeCycle {
                            plan: id.clone(),
                            target: target_id,
                        });
                    }
                }
                Edit::Push("supersedes", target_id)
            }
            (Link::Supersedes(target), false) => {
                // The id as given, or as the plan it names has one.
                let named = self
                    .resolve(&loaded, &target)
                    .ok()
                    .and_then(|(p, _)| p.id)
                    .unwrap_or(target);
                Edit::Remove("supersedes", named)
            }
        };
        self.write(&mut loaded, &found, &text, &[edit])
    }

    async fn check(&mut self) -> Result<CheckReport, PlanError> {
        let loaded = self.load()?;
        let rules = match loaded.plans.iter().any(|p| !p.tags.is_empty()) {
            true => Some(self.rules().await?),
            false => None,
        };
        let mut known_tasks: BTreeMap<String, bool> = BTreeMap::new();
        let mut findings = Vec::new();
        for path in &loaded.unread {
            findings.push(Finding::Unreadable { path: path.clone() });
        }
        let ids = loaded.ids();
        for plan in &loaded.plans {
            let path = || plan.path.clone();
            if let Some(error) = &plan.unreadable {
                findings.push(Finding::Frontmatter {
                    path: path(),
                    error: error.clone(),
                });
                continue;
            }
            let Some(id) = &plan.id else {
                findings.push(Finding::NoId { path: path() });
                continue;
            };
            let others: Vec<&str> = loaded
                .plans
                .iter()
                .filter(|p| p.id.as_ref() == Some(id) && p.path != plan.path)
                .map(|p| p.path.as_str())
                .collect();
            if !others.is_empty() {
                findings.push(Finding::DuplicateId {
                    path: path(),
                    id: id.clone(),
                    others: others.join(", "),
                });
            }
            match plan.written_status.as_deref() {
                Some(SUPERSEDED) => findings.push(Finding::HandSuperseded { path: path() }),
                Some(s) if !STATUSES.contains(&s) => findings.push(Finding::Status {
                    path: path(),
                    status: s.to_string(),
                }),
                _ => {}
            }
            if let Some(rules) = &rules {
                if let Some(finding) = tags_finding(plan, rules) {
                    findings.push(finding);
                }
            }
            for task in &plan.governs {
                if !known_tasks.contains_key(task) {
                    let exists = self.task_exists(task).await?;
                    known_tasks.insert(task.clone(), exists);
                }
                if !known_tasks[task] {
                    findings.push(Finding::Governs {
                        path: path(),
                        task: task.clone(),
                    });
                }
            }
            for target in &plan.supersedes {
                if target == id {
                    findings.push(Finding::SupersedesSelf {
                        path: path(),
                        plan: target.clone(),
                    });
                } else if !ids.contains(&target.as_str()) {
                    findings.push(Finding::Supersedes {
                        path: path(),
                        plan: target.clone(),
                    });
                }
            }
        }
        findings.sort_by(|a, b| a.path().cmp(b.path()));
        Ok(CheckReport { findings })
    }

    fn stamp(&mut self, path: &str, session: &str) -> Result<Stamp, PlanError> {
        if session.is_empty() {
            return Ok(Stamp::NoSession);
        }
        if !path.ends_with(PLAN_SUFFIX) {
            return Ok(Stamp::NotAPlan);
        }
        let path = self.place(path)?;
        let Some(text) = self.files.read(&path) else {
            return Ok(Stamp::NotAPlan);
        };
        let plan = Plan::parse(&path, &text);
        if plan.id.is_some() && plan.sessions.iter().any(|s| s == session) {
            return Ok(Stamp::Already);
        }
        // The other plans' ids are looked at only to mint one that is
        // free: a hook runs on every edit, and a scan of the roots is not
        // for the plans that have an id already.
        let mut loaded = Loaded::empty();
        if plan.id.is_none() {
            loaded = self.load().unwrap_or_else(|_| Loaded::empty());
        }
        let edits = match plan.sessions.iter().any(|s| s == session) {
            true => Vec::new(),
            false => vec![Edit::Push("sessions", session.to_string())],
        };
        self.write(&mut loaded, &plan, &text, &edits)?;
        Ok(Stamp::Stamped)
    }

    fn snapshot(&mut self) -> Result<Snapshot, PlanError> {
        let loaded = self.load()?;
        let mut snapshot = Snapshot::new();
        for plan in loaded.plans {
            let text = &loaded.texts[&plan.path];
            let watched = Watched {
                body: front::body(text).to_string(),
                id: plan.id.clone(),
                meta: plan.meta(),
                invalid: plan.unreadable.clone(),
            };
            snapshot.insert(plan.path, watched);
        }
        Ok(snapshot)
    }
}

/// The finding on `plan`'s tags as a set, if the store's rules refuse
/// them.
fn tags_finding(plan: &Plan, rules: &StoreRules) -> Option<Finding> {
    if plan.tags.is_empty() {
        return None;
    }
    let refused = |error: Report| Finding::Tag {
        path: plan.path.clone(),
        tag: plan.tags.join(", "),
        error,
    };
    let mut parsed = Vec::new();
    for spelled in &plan.tags {
        let tag = match tags::parse(spelled) {
            Some(Ok(tag)) => tag,
            Some(Err(refusal)) => return Some(refused(Report::of(&refusal))),
            None => return Some(refused(Report::of(&TagRefusal::Blank))),
        };
        if let Err(refusal) = tags::admit(&tag) {
            return Some(refused(Report::of(&refusal)));
        }
        parsed.push(tag);
    }
    judged(rules.validate(&[], &parsed, &[]))
        .err()
        .map(|refusal| refused(Report::of(&refusal)))
}

#[cfg(test)]
#[path = "service.test.rs"]
mod tests;
