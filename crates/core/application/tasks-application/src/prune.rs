//! Pruning: taking old finished tasks out of a project's store.
//!
//! A task is selected when its status is one of those asked for, its
//! status last changed before the cut (an age back from now, or a date),
//! and it matches the tag query, if one is given. Without `apply` the
//! prune only reports what it would take. With it, each selected task is
//! removed (`TaskRemoved{PRUNED}`), so every list drops it at once, and
//! then its stream is purged, which frees its harp: a removal by name
//! keeps both. A surviving task that still names a pruned one (a tag
//! carrying its harp, such as `relates:<harp>`, or the harp as a word of
//! its text or trigger) is reported, and left as written.
//!
//! Each removal is decided in a unit of work that first reads the task
//! and declines when it no longer qualifies (moved, retagged or pruned
//! since the selection), and each purge in a unit that first checks the
//! stream still holds the removed task, so a task minted on the freed harp
//! meanwhile is never purged.
//!
//! A purge that failed for good after its removal landed leaves a prune
//! half done: the task removed, its stream and harp still held. Every
//! prune finishes one, whatever it selects by: it is reported, and purged
//! when applied.

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;

use crate::DeadLetter;
use angzarr_diagnostic::{code, Arg, Diagnostic, Report};
use angzarr_embedded::v1::Cover;
use angzarr_embedded::{Kernel, KernelError, Purge, Recovery, Registry, Store, StreamRef};
use ports::{
    Clock, CommandIds, Configuration, Delay, Jitter, ProjectIdentity, TaskStores, Timestamp,
};
use tasks::{
    RemovalReason, RemoveTask, TaskAggregate, TaskCommand, TaskRejection, TaskState,
    STATUS_CLASS_SIGIL,
};

use crate::calendar::date;
use crate::lists::Lists;
use crate::request::given;
use crate::rules::StoreRulesExt;
use crate::task::task_cover;
use crate::write::Writes;
use crate::{failed, query, rejected, AppError, Project, Shown, Task, DONE};
use kernel_runner::{Refusal, Runner};

/// The age a prune takes by default: `--older-than 7d`.
pub const DEFAULT_OLDER_THAN: &str = "7d";
/// The statuses a prune takes by default: `--status Done`. Archived is
/// left out, so archiving is how a finished task is kept.
pub const DEFAULT_PRUNE_STATUSES: [&str; 1] = [DONE];

/// Which tasks to prune, and whether to.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct PruneRequest {
    pub project: Option<String>,
    /// The statuses that qualify a task; at least one, none blank
    /// ([`AppError::Blank`]) or starting with `@`.
    pub statuses: Vec<String>,
    /// How long ago a qualifying task's status last changed, at the
    /// least: an offset back from now (`30d`, `12w`, `720h`: durations
    /// with days and weeks), or a date (`2026-01-01`, midnight UTC).
    pub older_than: String,
    /// A postfix tag query; given blank, it is refused
    /// ([`AppError::Blank`]), never read as none.
    pub tag_query: Option<String>,
    /// Remove and purge what is selected; without it, only report.
    pub apply: bool,
}

/// One task a prune took, or without `apply` would take.
#[derive(Clone, PartialEq, Debug)]
pub struct Pruned {
    pub task: Task,
    /// When its status last changed: what put it past the cut.
    pub status_set_at: Timestamp,
}

/// Where a surviving task names a pruned one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Via {
    /// A tag carrying the harp, in its canonical spelling.
    Tag(String),
    /// The harp as a word of the task's text.
    Text,
    /// The harp as a word of the task's trigger.
    Trigger,
}

/// Spelled `tag <tag>`, `text` or `trigger`.
impl fmt::Display for Via {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Via::Tag(tag) => write!(f, "tag {tag}"),
            Via::Text => f.write_str("text"),
            Via::Trigger => f.write_str("trigger"),
        }
    }
}

/// A surviving task still naming a pruned one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Dangling {
    /// The surviving task holding the reference.
    pub harp_id: String,
    /// The pruned harp it names.
    pub pruned: String,
    pub via: Via,
}

/// What a prune selected, and what taking it costs.
#[derive(Clone, PartialEq, Debug)]
pub struct Pruning {
    /// The project pruned; `None` when the work root has none yet.
    pub project: Option<Project>,
    /// The statuses selected by.
    pub statuses: Vec<String>,
    /// The cut: a selected task's status last changed before it.
    pub before: Timestamp,
    /// The tag query selected by, if one was given.
    pub tag_query: Option<String>,
    /// Whether the tasks were taken, or only reported.
    pub applied: bool,
    /// The tasks taken (or that would be), in the order added.
    pub pruned: Vec<Pruned>,
    /// The events purged with them, the removal each prune records first
    /// included: as the store reports them, or as it would.
    pub events_removed: u64,
    /// The live tasks left.
    pub tasks_remaining: usize,
    /// Every place a task left names a pruned one.
    pub dangling: Vec<Dangling>,
    /// Which of the tasks' tags a person is shown.
    pub shown: Shown,
    /// The dead letters the kernel wrote while the prune ran (almost
    /// certainly bugs, for the caller to say loudly).
    pub dead_letters: Vec<DeadLetter>,
}

/// Why a prune was refused before it read anything.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PruneRefusal {
    /// No status to select by: an empty set is never read as every status.
    NoStatus,
    /// No age to select by.
    AgeRequired,
    /// An offset of zero or less.
    AgeNotPositive { age: String },
    /// Neither an offset nor a date.
    AgeUnreadable { age: String },
}

impl fmt::Display for PruneRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for PruneRefusal {
    fn code(&self) -> &str {
        match self {
            PruneRefusal::NoStatus => code!("prune-no-status"),
            PruneRefusal::AgeRequired => code!("prune-age-required"),
            PruneRefusal::AgeNotPositive { .. } => code!("prune-age-not-positive"),
            PruneRefusal::AgeUnreadable { .. } => code!("prune-age-unreadable"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            PruneRefusal::AgeNotPositive { age } | PruneRefusal::AgeUnreadable { age } => {
                vec![("age", age.as_str().into())]
            }
            PruneRefusal::NoStatus | PruneRefusal::AgeRequired => Vec::new(),
        }
    }
}

/// One of every refusal, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn samples() -> Vec<PruneRefusal> {
    let age = || "0d".to_string();
    vec![
        PruneRefusal::NoStatus,
        PruneRefusal::AgeRequired,
        PruneRefusal::AgeNotPositive { age: age() },
        PruneRefusal::AgeUnreadable { age: age() },
    ]
}

/// The prune use case, as a driving adapter calls it: opt-in, since it
/// needs a store that purges ([`PurgingStore`]). Its future is `Send`, so
/// a server can spawn it.
pub trait PruneService {
    fn prune(
        &mut self,
        request: PruneRequest,
    ) -> impl Future<Output = Result<Pruning, AppError>> + Send;
}

/// The prune over the ports it needs: its project and the kernel
/// ([`Writes`]), and the task lists kept caught up across the service's
/// reads. Borrowed from the service for one call.
pub(crate) struct Pruner<'s, I, S, G, C, D> {
    pub writes: Writes<'s, I, S, G, C, D>,
    pub lists: &'s mut Lists,
}

impl<I, S, G, C, D> Pruner<'_, I, S, G, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    G: Configuration,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    /// The prune `request` asks for; its failure carries the dead letters
    /// written while it ran ([`PruneService`] wraps them).
    pub(crate) async fn prune(&mut self, request: PruneRequest) -> Result<Pruning, AppError> {
        let tag_query = given(request.tag_query.as_deref(), "tag_query")?;
        query::check(tag_query).map_err(|e| AppError::TagQuery(Report::of(&e)))?;
        self.writes.projects.refuse_moved()?;
        let statuses = statuses(&request.statuses)?;
        let now = self.writes.runner.clock.now();
        let before = cut(&request.older_than, now).map_err(|e| refused(&e))?;
        let mut pruning = Pruning {
            project: None,
            statuses,
            before,
            tag_query: request.tag_query.clone(),
            applied: request.apply,
            pruned: Vec::new(),
            events_removed: 0,
            tasks_remaining: 0,
            dangling: Vec::new(),
            shown: Shown::default(),
            dead_letters: Vec::new(),
        };
        // A prune resolves its project as a read does: where there is none,
        // or no store yet, there is nothing to prune, and nothing is minted
        // or created to say so.
        let projects = &mut self.writes.projects;
        let found = projects.identity.for_read(request.project.as_deref());
        let Some(project) = found.map_err(AppError::Identity)? else {
            return Ok(pruning);
        };
        pruning.project = Some(project.clone());
        if projects
            .stores
            .open_existing(&project)
            .await
            .map_err(failed)?
            .is_none()
        {
            return Ok(pruning);
        }
        let rules = projects.rules_of(&project).await?;
        pruning.shown = rules.shown();
        let store = if request.apply {
            projects.stores.open(&project).await.map_err(failed)?
        } else {
            projects
                .stores
                .open_existing(&project)
                .await
                .map_err(failed)?
                .ok_or(AppError::NoProject)?
        };
        let registry = projects.registry;
        let listed = self.lists.tasks(&project, store, registry, live_tasks);
        let (tasks, unpurged) = listed.await?;
        let selection = Selection {
            statuses: &pruning.statuses,
            before,
            tag_query,
            rules: &rules,
        };
        let runner = &mut self.writes.runner;
        for task in selection.select(&tasks, &unpurged)? {
            let Some(events) = take(runner, store, task, &selection, request.apply).await? else {
                continue;
            };
            pruning.events_removed += events;
            pruning.pruned.push(Pruned {
                task: Task::of(task),
                status_set_at: task.status_set_at,
            });
        }
        pruning.left(&tasks);
        pruning.dead_letters = std::mem::take(runner.dead_letters);
        Ok(pruning)
    }
}

impl Pruning {
    /// What the prune left of `tasks` (every live task it read): how many,
    /// and every place one names a task it took.
    fn left(&mut self, tasks: &[TaskState]) {
        let gone: BTreeSet<&str> = self
            .pruned
            .iter()
            .map(|p| p.task.harp_id.as_str())
            .collect();
        let survivors: Vec<&TaskState> = tasks
            .iter()
            .filter(|t| !gone.contains(t.harp_id.as_str()))
            .collect();
        self.tasks_remaining = survivors.len();
        self.dangling = dangling(&survivors, &gone);
    }
}

/// Every live task `list` holds, and every task a prune removed but left
/// unpurged, each in the order added: one moment of the store.
fn live_tasks(list: &task_lists::TaskList) -> (Vec<TaskState>, Vec<TaskState>) {
    let in_order = |tasks: Vec<&TaskState>| {
        let mut tasks: Vec<TaskState> = tasks.into_iter().cloned().collect();
        tasks.sort_by(|a, b| added(a).cmp(&added(b)));
        tasks
    };
    (in_order(list.entries()), in_order(list.unpurged()))
}

/// Where `task` falls in the order added: when it was, then its harp.
fn added(task: &TaskState) -> (i64, i32, &str) {
    (
        task.created_at.seconds,
        task.created_at.nanos,
        task.harp_id.as_str(),
    )
}

/// Take `task`, as selected: remove it and purge its stream when
/// `apply`, else only count what that would delete. A task already
/// removed is one a prune removed and left unpurged: it is only purged.
/// The events deleted, or `None` when the task was not taken after all:
/// it no longer qualifies, or another removed or purged it since it was
/// selected.
async fn take<S: Store + Purge, C: Clock + CommandIds, D: Delay + Jitter + Send>(
    runner: &mut Runner<'_, C, D>,
    store: &S,
    task: &TaskState,
    selection: &Selection<'_>,
    apply: bool,
) -> Result<Option<u64>, AppError> {
    let stream = task_cover(&task.harp_id);
    if !apply {
        let at = StreamRef::try_from(&stream).map_err(failed)?;
        let held = store.read(at, 0, None).await.map_err(failed)?;
        // Its events, and the removal a prune records before it purges,
        // unless it holds that already.
        let removal = u64::from(!task.removed);
        return Ok(Some(held.pages.len() as u64 + removal));
    }
    if task.removed {
        let deleted = purge(runner, store, &stream).await?;
        return Ok((deleted > 0).then_some(deleted));
    }
    let command_id = runner.clock.command_id();
    let removal = TaskCommand::from(RemoveTask {
        reason: RemovalReason::Pruned,
    });
    // A task removed since is let through, for the kernel to answer: this
    // prune's own removal, landed on an attempt whose answer was lost, as
    // a duplicate; anyone else's as refused.
    let holds = |held: &TaskAggregate| {
        let held = held.state();
        let take = held.removed || selection.holds(held);
        (take.then(|| removal.clone()), ())
    };
    match runner
        .shaped::<TaskAggregate, _, _, _, _>(store, &stream, &command_id, holds)
        .await?
    {
        Ok((Some(_), ())) => purge(runner, store, &stream).await.map(Some),
        Ok((None, ())) => Ok(None),
        Err(Refusal {
            rejection: Some(TaskRejection::Removed { .. } | TaskRejection::NoTask),
            ..
        }) => Ok(None),
        Err(refusal) => Err(refusal.into()),
    }
}

/// Purge the stream of the task `stream` names, if it holds a removed
/// task, read first and purged at the head read (spec 14.43), so a live
/// task minted on the harp since is never purged: the store refuses the
/// stale head as a conflict, and the next attempt reads again, after the
/// wait the kernel would wait ([`Runner::again`]). How many events it
/// deleted: 0 when the stream holds no removed task.
async fn purge<S: Store + Purge, C: Clock, D: Delay + Jitter + Send>(
    runner: &mut Runner<'_, C, D>,
    store: &S,
    stream: &Cover,
) -> Result<u64, AppError> {
    let mut failed = 0;
    loop {
        let (failure, recovery) = match purge_removed(runner.registry, store, stream).await {
            Ok(deleted) => return Ok(deleted),
            Err(failed) => failed,
        };
        runner.again(&mut failed, failure, recovery).await?;
    }
}

/// Purge `stream`, if it holds a removed task, at the head it was loaded
/// at: a stream moved since (purged by another prune, a task minted on the
/// freed harp) is the store's conflict, and the retry loads it again.
async fn purge_removed<S: Store + Purge>(
    registry: &Registry,
    store: &S,
    stream: &Cover,
) -> Result<u64, (Report, Recovery)> {
    let kernel_failed = |error: KernelError| {
        let recovery = error.recovery();
        (Report::of(&error), recovery)
    };
    let (task, head) = Kernel::new(registry)
        .load::<TaskAggregate, _>(store, stream)
        .await
        .map_err(kernel_failed)?;
    if !task.state().state().removed || head == 0 {
        return Ok(0);
    }
    let at = StreamRef::try_from(stream).map_err(|e| kernel_failed(KernelError::Store(e)))?;
    store
        .purge(at, head)
        .await
        .map_err(|e| kernel_failed(KernelError::Store(e)))
}

/// What a prune selects by.
struct Selection<'a> {
    statuses: &'a [String],
    before: Timestamp,
    tag_query: Option<&'a str>,
    rules: &'a crate::rules::StoreRules,
}

impl Selection<'_> {
    /// The tasks this prune takes, in the order added: each of the live
    /// `tasks` dated and matching the tag query, and every task a prune
    /// removed but left `unpurged`, which that prune selected: this one
    /// finishes it, whatever it selects by.
    fn select<'t>(
        &self,
        tasks: &'t [TaskState],
        unpurged: &'t [TaskState],
    ) -> Result<Vec<&'t TaskState>, AppError> {
        let matched = query::select(
            self.tag_query,
            tasks
                .iter()
                .map(|t| (t.harp_id.as_str(), t.tags.as_slice())),
            self.rules,
        )
        .map_err(|e| AppError::TagQuery(Report::of(&e)))?;
        let mut selected: Vec<&TaskState> = tasks
            .iter()
            .filter(|t| matched.contains(&t.harp_id) && self.dated(t))
            .chain(unpurged)
            .collect();
        selected.sort_by(|a, b| added(a).cmp(&added(b)));
        Ok(selected)
    }

    /// Whether `task` is live, in one of the statuses, and last changed
    /// status before the cut.
    fn dated(&self, task: &TaskState) -> bool {
        let at = task.status_set_at;
        !task.removed
            && self.statuses.contains(&task.status)
            && (at.seconds, at.nanos) < (self.before.seconds, self.before.nanos)
    }

    /// Whether `task` qualifies whole: dated, and matching the tag query.
    fn holds(&self, task: &TaskState) -> bool {
        self.dated(task)
            && query::select(
                self.tag_query,
                [(task.harp_id.as_str(), task.tags.as_slice())],
                self.rules,
            )
            .is_ok_and(|matched| matched.contains(&task.harp_id))
    }
}

/// The statuses asked for, trimmed and empties dropped: at least one, and
/// none claiming the status-class sigil (`@open` and `@terminal` are not
/// taken yet, as in a list).
fn statuses(asked: &[String]) -> Result<Vec<String>, AppError> {
    let statuses: Vec<String> = asked.iter().map(|s| s.trim().to_string()).collect();
    // A status given blank is refused, never left out of the set.
    if statuses.iter().any(String::is_empty) {
        return Err(AppError::Blank { field: "status" });
    }
    if let Some(class) = statuses.iter().find(|s| s.starts_with(STATUS_CLASS_SIGIL)) {
        return Err(rejected(&TaskRejection::StatusReserved {
            status: class.clone(),
        }));
    }
    if statuses.is_empty() {
        return Err(refused(&PruneRefusal::NoStatus));
    }
    Ok(statuses)
}

fn refused(refusal: &PruneRefusal) -> AppError {
    AppError::Prune(Report::of(refusal))
}

/// Every place one of `survivors` names a harp in `gone`: a tag carrying it
/// as a word (its key or value, `relates:<harp>`), or the harp as a word
/// of the text or the trigger. Each harp once per tag, text or trigger.
fn dangling(survivors: &[&TaskState], gone: &BTreeSet<&str>) -> Vec<Dangling> {
    let mut found = Vec::new();
    for task in survivors {
        let mut note = |text: &str, via: Via| {
            let mut seen = BTreeSet::new();
            for word in harp_words(text) {
                if gone.contains(word) && seen.insert(word) {
                    found.push(Dangling {
                        harp_id: task.harp_id.clone(),
                        pruned: word.to_string(),
                        via: via.clone(),
                    });
                }
            }
        };
        for tag in &task.tags {
            let spelled = tag.to_string();
            note(&spelled, Via::Tag(spelled.clone()));
        }
        note(&task.text, Via::Text);
        note(&task.trigger, Via::Trigger);
    }
    found
}

/// The runs of the characters a harp is spelled with (lowercase letters,
/// digits and hyphens) in `text`, so a harp is matched as a whole word and
/// never as part of a longer hyphenated one.
fn harp_words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        .filter(|word| !word.is_empty())
}

const NANOS_PER_SECOND: i128 = 1_000_000_000;
const SECONDS_PER_DAY: i64 = 86_400;

/// The instant `age` bounds, read at `now`: an offset back from now, or a
/// date's midnight, UTC.
pub fn cut(age: &str, now: Timestamp) -> Result<Timestamp, PruneRefusal> {
    let trimmed = age.trim();
    if trimmed.is_empty() {
        return Err(PruneRefusal::AgeRequired);
    }
    if let Some(nanos) = offset(trimmed) {
        if nanos <= 0 {
            return Err(PruneRefusal::AgeNotPositive {
                age: age.to_string(),
            });
        }
        let at = i128::from(now.seconds) * NANOS_PER_SECOND + i128::from(now.nanos) - nanos;
        return Ok(Timestamp {
            seconds: i64::try_from(at.div_euclid(NANOS_PER_SECOND)).unwrap_or(i64::MIN),
            nanos: i32::try_from(at.rem_euclid(NANOS_PER_SECOND)).unwrap_or_default(),
        });
    }
    date(trimmed).ok_or_else(|| PruneRefusal::AgeUnreadable {
        age: age.to_string(),
    })
}

/// `text` as a span of time, in nanoseconds: a whole number of days (`d`)
/// or weeks (`w`), or a duration (an optional sign, then numbers with
/// units, `1h30m`, `1.5h`, `720h`).
fn offset(text: &str) -> Option<i128> {
    for (suffix, days) in [('d', 1), ('w', 7)] {
        if let Some(count) = text.strip_suffix(suffix) {
            if let Ok(count) = count.parse::<i64>() {
                return Some(
                    i128::from(count) * i128::from(days * SECONDS_PER_DAY) * NANOS_PER_SECOND,
                );
            }
        }
    }
    duration(text)
}

/// A duration (an optional sign, then decimal numbers each with a unit of
/// `ns`, `us`, `µs`, `ms`, `s`, `m` or `h`), in nanoseconds.
fn duration(text: &str) -> Option<i128> {
    let (sign, mut rest) = match text.as_bytes().first()? {
        b'-' => (-1, &text[1..]),
        b'+' => (1, &text[1..]),
        _ => (1, text),
    };
    if rest == "0" {
        return Some(0);
    }
    if rest.is_empty() {
        return None;
    }
    let mut total: i128 = 0;
    while !rest.is_empty() {
        let whole_len = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let (whole, after) = rest.split_at(whole_len);
        let (fraction, after) = match after.strip_prefix('.') {
            Some(after) => after.split_at(
                after
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(after.len()),
            ),
            None => ("", after),
        };
        if whole.is_empty() && fraction.is_empty() {
            return None;
        }
        let unit_len = after
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(after.len());
        let (unit, after) = after.split_at(unit_len);
        let scale: i128 = match unit {
            "ns" => 1,
            "us" | "\u{b5}s" | "\u{3bc}s" => 1_000,
            "ms" => 1_000_000,
            "s" => 1_000_000_000,
            "m" => 60_000_000_000,
            "h" => 3_600_000_000_000,
            _ => return None,
        };
        // Digits past the nanosecond are dropped.
        let whole: i128 = if whole.is_empty() {
            0
        } else {
            whole.parse().ok()?
        };
        let mut part = 0;
        let mut place = scale;
        for digit in fraction.bytes() {
            place /= 10;
            part += i128::from(digit - b'0') * place;
        }
        total = total.checked_add(whole.checked_mul(scale)?.checked_add(part)?)?;
        // A duration its int64 of nanoseconds cannot hold is refused.
        if total > i128::from(i64::MAX) {
            return None;
        }
        rest = after;
    }
    Some(sign * total)
}

#[cfg(test)]
#[path = "prune.test.rs"]
mod tests;
