//! [`PlanService`]: the use cases over the ports and angzarr-embedded's
//! kernel.
//!
//! **Each command is decided at the head it read.** A command's stream is
//! read and replayed, the command decided against it (so a refusal comes
//! back typed, before anything is sent), and sent with that head as its
//! expected sequence (STRICT): a write landing between is the kernel's
//! conflict, and the retry reads and decides again. A retry re-sends the
//! same command id, so an attempt that landed after all is answered
//! DUPLICATE. The waits between attempts are the tasks' [`Backoff`], over
//! the [`Delay`] and [`Jitter`] ports, and the kernel retries nothing
//! ([`KERNEL_RETRY`]).
//!
//! **A supersede is two writes**, one on each plan's stream: `Supersede` on
//! the superseding plan, then `MarkSuperseded` on its target (an unlink,
//! `Unsupersede` then `Reinstate`). No order crosses streams. Each half is
//! checked against what the other stream holds, so a link or unlink left
//! half done is finished by running it again. Two supersedes of one target
//! racing can both pass their check (the design's flag): the second
//! target's write is refused, and its first half is left for an unlink.

use std::sync::{Mutex, PoisonError};

use angzarr_embedded::next::v1::{
    command_page, Command, CommandBook, CommandPage, Cover, Handling, Uuid,
};
use angzarr_embedded::next::{
    ids, DeadLetterLog, EventStore, Failure, Kernel, KernelError, Payload, Recovery, Registry,
    Sampled, Store, StoreError, Stream, StreamRef,
};
use angzarr_embedded_diagnostic::Report;
use plan_lists::{PlanList, PlanRow};
use plans::{decide, AddPlan, PlanAggregate, PlanCommand, PlanRejection, PlanState, PLAN_DOMAIN};
use ports::{
    Clock, CommandIds, Delay, Jitter, Mint, PlanFiles, Project, ProjectIdentity, TaskStores, Tasks,
};

use crate::rules::{joined, locate, new_plan_text, Located};
use crate::{
    content_hash, default_path, frontmatter, plan_name, resolve_path, Backoff, CheckReport,
    DeadLetter, Finding, ImportReport, Link, NewPlan, PlanError, PlanQuery, PlanStatus, PlanText,
    PlanView, Plans, KERNEL_RETRY, PLANS_DIR,
};

/// How many plan ids an add mints before giving up, each refused because a
/// plan already holds it.
pub const HARP_ATTEMPTS: u32 = 100;

/// The registry the plans domain runs under: the plan aggregate.
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.aggregate::<PlanAggregate>(PlanAggregate::default);
    registry
}

/// The use cases over their ports: `identity` resolves the project (the
/// one `project` pins, if any), `stores` opens its store, `files` reaches
/// the work tree, `tasks` the project's tasks and `mint` new ids; `clock`
/// stamps commands and mints their ids; `delay` waits, with jitter, between
/// retries.
pub struct PlanService<I, S, F, T, M, C, D> {
    pub identity: I,
    pub stores: S,
    pub files: F,
    pub tasks: T,
    pub mint: M,
    pub clock: C,
    pub delay: D,
    pub backoff: Backoff,
    /// `--project`: the project to act on, overriding the work root's.
    pub project: Option<String>,
    registry: Registry,
    dead_letters: DeadLetters,
}

impl<I, S, F, T, M, C, D> PlanService<I, S, F, T, M, C, D> {
    pub fn new(identity: I, stores: S, files: F, tasks: T, mint: M, clock: C, delay: D) -> Self {
        PlanService {
            identity,
            stores,
            files,
            tasks,
            mint,
            clock,
            delay,
            backoff: Backoff::DEFAULT,
            project: None,
            registry: registry(),
            dead_letters: DeadLetters::default(),
        }
    }

    /// The same service, acting on `project` when one is given.
    pub fn pinned(self, project: Option<String>) -> Self {
        PlanService { project, ..self }
    }
}

/// The log the kernel writes dead letters to, kept until taken.
#[derive(Default)]
struct DeadLetters(Mutex<Vec<String>>);

impl DeadLetters {
    fn take(&self) -> Vec<DeadLetter> {
        let mut lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut *lines)
            .into_iter()
            .map(|line| DeadLetter { line })
            .collect()
    }
}

impl DeadLetterLog for DeadLetters {
    fn write(&self, line: &str) {
        let mut lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        lines.push(line.to_string());
    }
}

/// The stream of the plan `harp_id` names.
fn cover(harp_id: &str) -> Cover {
    Cover {
        domain: PLAN_DOMAIN.to_string(),
        root: ids::compute_root(PLAN_DOMAIN, harp_id).ok(),
        edition: None,
    }
}

/// `command` for the plan `harp_id`, sent at `head`.
fn book(harp_id: &str, command: &PlanCommand, head: u64) -> CommandBook {
    CommandBook {
        cover: Some(cover(harp_id)),
        page: Some(CommandPage {
            body: Some(command_page::Body::Command(Command {
                payload: Some(command.to_any()),
            })),
            ..CommandPage::default()
        }),
        handling: Some(Handling {
            expected_sequence: Some(head),
            ..Handling::default()
        }),
    }
}

/// A store's failure, as the kernel reports one, with what to do.
fn store_failed(error: StoreError) -> (Report, Recovery) {
    let error = KernelError::Store(error);
    let recovery = error.recovery();
    (Report::of(&error), recovery)
}

fn failed(error: StoreError) -> PlanError {
    PlanError::Failed(Report::of(&KernelError::Store(error)))
}

/// The plan `harp_id`'s stream replayed, and its head: an empty plan at
/// head 0 for a stream with no events.
async fn plan_at<St: EventStore>(
    registry: &Registry,
    store: &St,
    harp_id: &str,
) -> Result<(PlanState, u64), (Report, Recovery)> {
    let cover = cover(harp_id);
    let at = StreamRef::try_from(&cover).map_err(store_failed)?;
    let slice = store.read(at, 0, None).await.map_err(store_failed)?;
    if slice.pages.is_empty() {
        return Ok((PlanState::default(), slice.next_sequence));
    }
    let replayed = Stream::<PlanAggregate>::replay(cover, &slice.pages, registry)
        .map_err(|e| (Report::of(&e), Recovery::Bug))?;
    Ok((replayed.state().state().clone(), slice.next_sequence))
}

/// What one run of a command needs besides the store and the command.
struct Runner<'a, C, D> {
    registry: &'a Registry,
    dead_letters: &'a DeadLetters,
    clock: &'a mut C,
    delay: &'a mut D,
    backoff: Backoff,
}

impl<C: Clock + CommandIds, D: Delay + Jitter> Runner<'_, C, D> {
    /// Run `command` on the plan `harp_id` in `store` until it commits, is
    /// refused, fails in a way a retry will not mend, or the attempts run
    /// out: decided against the stream as each attempt reads it, sent at
    /// that head, under one command id.
    async fn run<St: Store>(
        &mut self,
        store: &St,
        harp_id: &str,
        command: &PlanCommand,
    ) -> Result<(), PlanError> {
        let command_id: Uuid = self.clock.command_id();
        let mut failed = 0;
        loop {
            let (state, head) = match plan_at(self.registry, store, harp_id).await {
                Ok(read) => read,
                Err((failure, recovery)) => {
                    self.again(&mut failed, failure, recovery).await?;
                    continue;
                }
            };
            let decided = decide(&state, command);
            // A retry that finds its own command landed is the kernel's to
            // answer DUPLICATE: it is sent whatever this decides of it now.
            if let (Err(rejection), 0) = (&decided, failed) {
                return Err(PlanError::Rejected(rejection.clone()));
            }
            let sampled = Sampled {
                now: self.clock.now(),
                command_id: command_id.clone(),
            };
            let handled = Kernel::new(self.registry, self.dead_letters)
                .on_retryable(KERNEL_RETRY)
                .handle::<PlanAggregate, _, _>(
                    store,
                    book(harp_id, command, head),
                    &sampled,
                    &mut (),
                )
                .await;
            let (failure, recovery) = match handled {
                Ok(_) => return Ok(()),
                Err(Failure::Rejected(rejected)) => {
                    return Err(match decided {
                        Err(rejection) => PlanError::Rejected(rejection),
                        // Decided here at the head the kernel refused at:
                        // the two disagreeing is a bug.
                        Ok(_) => PlanError::Failed(rejected.report().clone()),
                    });
                }
                Err(failure) => (Report::of(&failure), failure.recovery()),
            };
            self.again(&mut failed, failure, recovery).await?;
        }
    }

    /// After the `failed`th failure, wait to try again; or the error to
    /// end with, when a retry will not mend it or the attempts are spent.
    async fn again(
        &mut self,
        failed: &mut u32,
        failure: Report,
        recovery: Recovery,
    ) -> Result<(), PlanError> {
        if recovery != Recovery::Retry {
            return Err(PlanError::Failed(failure));
        }
        *failed += 1;
        if *failed >= self.backoff.attempts {
            return Err(PlanError::GaveUp {
                attempts: *failed,
                error: failure,
            });
        }
        let ceiling = self.backoff.ceiling(*failed);
        let wait = self.delay.up_to(ceiling).min(ceiling);
        self.delay.sleep(wait).await;
        Ok(())
    }
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

/// The view of `row`, its file `text` as read now (None: missing).
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

impl<I, S, F, T, M, C, D> PlanService<I, S, F, T, M, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    F: PlanFiles,
    T: Tasks,
    M: Mint,
    C: Clock + CommandIds,
    D: Delay + Jitter,
{
    /// The project a write goes to: pinned, bound, or minted and bound.
    pub(crate) fn for_write(&mut self) -> Result<Project, PlanError> {
        self.identity
            .for_write(self.project.as_deref())
            .map_err(PlanError::Identity)
    }

    /// The project a read is about: `None` where there is none yet, or it
    /// has no store, so no plan.
    pub(crate) async fn for_read(&mut self) -> Result<Option<Project>, PlanError> {
        let Some(project) = self
            .identity
            .for_read(self.project.as_deref())
            .map_err(PlanError::Identity)?
        else {
            return Ok(None);
        };
        let store = self.stores.open_existing(&project).await.map_err(failed)?;
        Ok(store.is_some().then_some(project))
    }

    /// The project holding the plan `plan`, which must exist.
    async fn holding(&mut self, plan: &str) -> Result<Project, PlanError> {
        let missing = || PlanError::NoSuchPlan(plan.to_string());
        let project = self.for_read().await?.ok_or_else(missing)?;
        match self.state(&project, plan).await? {
            Some(_) => Ok(project),
            None => Err(missing()),
        }
    }

    pub(crate) fn work_root(&self) -> Result<String, PlanError> {
        self.files.work_root().map_err(PlanError::NoWorkRoot)
    }

    /// Every plan of `project`'s store, caught up now.
    pub(crate) async fn plan_list(&mut self, project: &Project) -> Result<PlanList, PlanError> {
        let mut list = PlanList::new();
        if let Some(store) = self.stores.open_existing(project).await.map_err(failed)? {
            list.catch_up(store, &self.registry)
                .await
                .map_err(|e| PlanError::Failed(Report::of(&*e)))?;
        }
        Ok(list)
    }

    /// The plan `harp_id` as its stream holds it now; `None` when it holds
    /// none.
    pub(crate) async fn state(
        &mut self,
        project: &Project,
        harp_id: &str,
    ) -> Result<Option<PlanState>, PlanError> {
        let Some(store) = self.stores.open_existing(project).await.map_err(failed)? else {
            return Ok(None);
        };
        let (state, _) = plan_at(&self.registry, store, harp_id)
            .await
            .map_err(|(report, _)| PlanError::Failed(report))?;
        Ok(state.added.then_some(state))
    }

    /// Run `command` on the plan `harp_id` in `project`'s store.
    pub(crate) async fn commit(
        &mut self,
        project: &Project,
        harp_id: &str,
        command: PlanCommand,
    ) -> Result<(), PlanError> {
        let store = self.stores.open(project).await.map_err(failed)?;
        Runner {
            registry: &self.registry,
            dead_letters: &self.dead_letters,
            clock: &mut self.clock,
            delay: &mut self.delay,
            backoff: self.backoff,
        }
        .run(store, harp_id, &command)
        .await
    }

    /// A fresh plan id: one no plan stream of `project` holds.
    pub(crate) async fn fresh_id(&mut self, project: &Project) -> Result<String, PlanError> {
        for _ in 0..HARP_ATTEMPTS {
            let harp_id = self.mint.mint();
            if self.state(project, &harp_id).await?.is_none() {
                return Ok(harp_id);
            }
        }
        Err(PlanError::HarpsExhausted {
            attempts: HARP_ATTEMPTS,
        })
    }

    /// The plan `harp_id`, viewed as its stream and file are now.
    async fn view(&mut self, project: &Project, harp_id: &str) -> Result<PlanView, PlanError> {
        let state = self
            .state(project, harp_id)
            .await?
            .ok_or_else(|| PlanError::NoSuchPlan(harp_id.to_string()))?;
        let root = self.work_root()?;
        let text = self.files.read(&state.path);
        Ok(view_of(&root, &row_of(&state), text.as_deref()))
    }

    /// `row`'s file read now, and its hash recorded where it changed (the
    /// row then carrying it): the file's text, or None when it is missing.
    async fn tracked(
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
            match self.commit(project, &row.harp_id, rehash).await {
                // Recorded meanwhile by another reader: the same hash.
                Ok(()) | Err(PlanError::Rejected(PlanRejection::HashUnchanged)) => {}
                Err(error) => return Err(error),
            }
            row.content_hash = hash;
        }
        Ok(Some(text))
    }

    async fn adding(&mut self, new: NewPlan) -> Result<PlanView, PlanError> {
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
        let root = self.work_root()?;
        let (harp_id, path, hash, project) = match &new.path {
            Some(given) => {
                let path = resolve_path(&root, given).map_err(PlanError::Path)?;
                let text = self
                    .files
                    .read(&path)
                    .ok_or_else(|| PlanError::FileMissing(path.clone()))?;
                let project = self.for_write()?;
                if let Some(row) = self.plan_list(&project).await?.by_path(&path) {
                    return Err(PlanError::AlreadyRegistered {
                        path,
                        plan: row.harp_id.clone(),
                    });
                }
                let harp_id = self.fresh_id(&project).await?;
                (harp_id, path, content_hash(&text), project)
            }
            None => {
                let project = self.for_write()?;
                let harp_id = self.fresh_id(&project).await?;
                let path = default_path(&harp_id);
                let text = new_plan_text(&template.title, &new.session);
                if !self
                    .files
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
        self.commit(&project, &harp_id, PlanCommand::Add(add))
            .await?;
        for task in governs {
            self.commit(&project, &harp_id, PlanCommand::Govern(task))
                .await?;
        }
        self.view(&project, &harp_id).await
    }

    async fn linking(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        let project = self.holding(plan).await?;
        match link {
            Link::Governs(task) => {
                if !self.tasks.exists(&task).await.map_err(PlanError::Tasks)? {
                    return Err(PlanError::NoSuchTask(task));
                }
                self.commit(&project, plan, PlanCommand::Govern(task))
                    .await?;
            }
            Link::Supersedes(target) => self.superseding(&project, plan, target).await?,
        }
        self.view(&project, plan).await
    }

    /// `plan` supersedes `target`: both exist, the link closes no cycle,
    /// and no other plan supersedes `target`. Two writes, the second on the
    /// target; a link left half done (the first landed) is finished.
    async fn superseding(
        &mut self,
        project: &Project,
        plan: &str,
        target: String,
    ) -> Result<(), PlanError> {
        if target == plan {
            return Err(PlanError::Rejected(PlanRejection::SupersedesItself));
        }
        let list = self.plan_list(project).await?;
        let (Some(superseding), Some(superseded)) = (list.get(plan), list.get(&target)) else {
            return Err(PlanError::NoSuchPlan(target));
        };
        if list.supersedes_reaches(&target, plan) {
            return Err(PlanError::SupersedeCycle {
                plan: plan.to_string(),
                target,
            });
        }
        let linked = superseding.supersedes.contains(&target);
        let marked_by = superseded.superseded_by.clone();
        match (linked, marked_by.as_str()) {
            (true, by) if by == plan => {
                return Err(PlanError::Rejected(PlanRejection::AlreadySupersedes(
                    target,
                )))
            }
            (_, "") => {}
            (_, by) if by != plan => {
                return Err(PlanError::AlreadySuperseded {
                    plan: target,
                    by: by.to_string(),
                })
            }
            _ => {}
        }
        if !linked {
            self.commit(project, plan, PlanCommand::Supersede(target.clone()))
                .await?;
        }
        if marked_by.is_empty() {
            self.commit(
                project,
                &target,
                PlanCommand::MarkSuperseded(plan.to_string()),
            )
            .await?;
        }
        Ok(())
    }

    async fn unlinking(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        let project = self.holding(plan).await?;
        match link {
            Link::Governs(task) => {
                self.commit(&project, plan, PlanCommand::Ungovern(task))
                    .await?;
            }
            Link::Supersedes(target) => {
                let linked = self
                    .state(&project, plan)
                    .await?
                    .is_some_and(|p| p.supersedes.contains(&target));
                let marked = self
                    .state(&project, &target)
                    .await?
                    .is_some_and(|t| t.superseded_by == plan);
                if !linked && !marked {
                    return Err(PlanError::Rejected(PlanRejection::NotSuperseded(target)));
                }
                if linked {
                    self.commit(&project, plan, PlanCommand::Unsupersede(target.clone()))
                        .await?;
                }
                if marked {
                    self.commit(&project, &target, PlanCommand::Reinstate)
                        .await?;
                }
            }
        }
        self.view(&project, plan).await
    }

    async fn listing(&mut self, query: &PlanQuery) -> Result<Vec<PlanView>, PlanError> {
        let Some(project) = self.for_read().await? else {
            return Ok(Vec::new());
        };
        let root = self.work_root()?;
        let list = self.plan_list(&project).await?;
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
            let text = self.tracked(&project, &mut row).await?;
            views.push(view_of(&root, &row, text.as_deref()));
        }
        Ok(views)
    }

    async fn showing(&mut self, id_or_path: &str) -> Result<PlanText, PlanError> {
        let missing = || PlanError::NoSuchPlan(id_or_path.to_string());
        let project = self.for_read().await?.ok_or_else(missing)?;
        let root = self.work_root()?;
        let list = self.plan_list(&project).await?;
        let found = list
            .get(id_or_path)
            .or_else(|| match locate(&root, id_or_path) {
                Located::Inside(path) => list.by_path(&path),
                Located::Outside(_) => None,
            });
        let mut row = found.cloned().ok_or_else(missing)?;
        let text = self.tracked(&project, &mut row).await?;
        Ok(PlanText {
            plan: view_of(&root, &row, text.as_deref()),
            content: text.map(|text| String::from_utf8_lossy(&text).into_owned()),
        })
    }

    async fn checking(&mut self) -> Result<CheckReport, PlanError> {
        let mut findings = Vec::new();
        let mut recorded = Vec::new();
        if let Some(project) = self.for_read().await? {
            let list = self.plan_list(&project).await?;
            for row in list.plans() {
                let mut row = row.clone();
                recorded.push(row.path.clone());
                if self.tracked(&project, &mut row).await?.is_none() {
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
        for path in self.files.plan_files(PLANS_DIR) {
            if !recorded.contains(&path) {
                findings.push(Finding::Unregistered { path });
            }
        }
        Ok(CheckReport { findings })
    }
}

impl<I, S, F, T, M, C, D> Plans for PlanService<I, S, F, T, M, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    F: PlanFiles,
    T: Tasks,
    M: Mint,
    C: Clock + CommandIds,
    D: Delay + Jitter,
{
    async fn add(&mut self, new: NewPlan) -> Result<PlanView, PlanError> {
        self.adding(new).await
    }

    async fn set_status(&mut self, plan: &str, status: PlanStatus) -> Result<PlanView, PlanError> {
        let project = self.holding(plan).await?;
        self.commit(&project, plan, PlanCommand::SetStatus(status))
            .await?;
        self.view(&project, plan).await
    }

    async fn link(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.linking(plan, link).await
    }

    async fn unlink(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        self.unlinking(plan, link).await
    }

    async fn list(&mut self, query: &PlanQuery) -> Result<Vec<PlanView>, PlanError> {
        self.listing(query).await
    }

    async fn show(&mut self, id_or_path: &str) -> Result<PlanText, PlanError> {
        self.showing(id_or_path).await
    }

    async fn check(&mut self) -> Result<CheckReport, PlanError> {
        self.checking().await
    }

    async fn import(&mut self, dry_run: bool) -> Result<ImportReport, PlanError> {
        self.importing(dry_run).await
    }

    fn dead_letters(&mut self) -> Vec<DeadLetter> {
        let mut letters = self.dead_letters.take();
        letters.extend(
            self.tasks
                .dead_letters()
                .into_iter()
                .map(|line| DeadLetter { line }),
        );
        letters
    }
}

#[cfg(test)]
#[path = "service.test.rs"]
mod tests;
