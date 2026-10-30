//! Importing the JSONL task logs into each project's store.
//!
//! Each record becomes the command that does what it did, run at the
//! instant it was recorded, so a task's creation, its status's last change
//! and its last deferral keep the log's times. The records are replayed in
//! fold order: a line repeating an earlier one byte for byte is skipped,
//! and the rest are sorted by time, stably, so the file breaks ties. The
//! log's fold decides which records count:
//!
//! - an add for a harp the log already issued is a displaced duplicate
//!   (two tasks minted the same harp). The first keeps it. A displaced add
//!   that a later repair add names (`repair_of`) was re-added under a
//!   fresh harp, which imports as any add does; one no repair names is not
//!   imported, and is reported.
//! - a record to a harp with no live task (never added, or removed) is
//!   ignored.
//! - a tag or untag that changes no tag records nothing.
//!
//! Go's empty string is a value left out. Go keeps a trigger given with any
//! status, and re-defers a task on the one it holds; here a trigger is
//! held only while a task is Deferred, and refused with another status. So
//! a trigger logged with another status is left out of its record's
//! command, and kept, as Go keeps it, for a later deferral that names
//! none, which is given it.
//!
//! What the tasks domain refuses that the log's fold accepts (empty text,
//! a Deferred status with no trigger, a status starting with `@`) is not
//! imported, and is reported; so is every later record to a task whose add
//! was not imported. A logged tag that is not a tag in tagma's grammar, or
//! fails what every task's tag must pass whatever the schema
//! ([`tags::schema_free`]: a bare operator word, which could never be
//! queried, or a tag in tagma's reserved namespaces, which tagma would read
//! as its config), is left out of its record's command, and reported; the
//! rest of the record is imported. The project's schema is not applied. The fold still counts it, so a later record is judged
//! against the log's tags as written.
//!
//! Re-running an import duplicates nothing: a record's command id derives
//! from its line ([`record_command_id`]), so a record already imported is
//! found in its task's stream and skipped, and an import that stopped
//! part way resumes. A log appended to since is imported up to its new
//! end.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;

use angzarr_embedded::next::v1::Uuid;
use angzarr_embedded::next::{ids, EventStore, Registry, StreamRef};
use angzarr_embedded_diagnostic::{Arg, Diagnostic, Report};
use ports::{
    Clock, Delay, Jitter, JsonlOp, JsonlRecord, JsonlTaskLogs, ProjectIdentity, TaskStores,
    Timestamp,
};
use tasks::{AddTask, EditText, RemovalReason, RemoveTask, Retag, SetStatus, TaskCommand};

use crate::dead_letter::{DeadLetter, DeadLetters};
use crate::retry::Backoff;
use crate::tags::{self, Tag, TagRefusal};
use crate::view::{DEFERS, INITIAL};
use crate::{book, cover, registry, retry, store_report, text_hash, AppError};

/// What to import: every project with a log, or the one named.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ImportRequest {
    pub project: Option<String>,
}

/// What an import did, project by project, and the dead letters the
/// kernel wrote while it ran (almost certainly bugs, for the caller to say
/// loudly).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ImportReport {
    pub projects: Vec<ProjectImport>,
    pub dead_letters: Vec<DeadLetter>,
}

impl ImportReport {
    /// Whether everything was brought over: no project failed and nothing
    /// was left behind.
    pub fn complete(&self) -> bool {
        self.projects
            .iter()
            .all(|p| p.failed.is_none() && p.lost.is_empty())
    }
}

/// What importing one project's log did.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ProjectImport {
    pub project_id: String,
    /// Why the import stopped: the log could not be read, the project id
    /// is not valid, or its store failed. What was imported before stays,
    /// and a re-run resumes from it.
    pub failed: Option<Report>,
    /// The log's records (its non-blank lines).
    pub records: usize,
    /// Lines repeating an earlier one byte for byte, which the fold skips.
    pub repeated: usize,
    /// Tasks this run added.
    pub tasks: usize,
    /// Records this run imported, adds included.
    pub applied: usize,
    /// Records an earlier run imported.
    pub already: usize,
    /// Tags and untags that change no tag, recording nothing in the fold
    /// or here.
    pub unchanged: usize,
    /// Records the fold ignores: to a harp with no live task.
    pub ignored: usize,
    /// Displaced duplicate adds that a repair re-added under a fresh harp:
    /// the re-add is imported, under that harp.
    pub repaired: usize,
    /// What could not be brought over.
    pub lost: Vec<Lost>,
}

/// A record, or a tag of one, that could not be brought over.
#[derive(Clone, PartialEq, Debug)]
pub struct Lost {
    /// Its line in the log.
    pub line: u32,
    /// What kind of record it is, as the log names it (`add`, `status`,
    /// `text`, `remove`, `tag`, `untag`).
    pub op: &'static str,
    pub harp_id: String,
    /// Its text, if it carries any: an add's or an edit's.
    pub text: String,
    pub why: Why,
}

/// Why a record was not brought over.
#[derive(Clone, PartialEq, Debug)]
pub enum Why {
    /// An add for a harp another task already holds in the log, which no
    /// repair re-added: the fold keeps the first task. Run `taskloom
    /// repair` on the log, then import again.
    Displaced,
    /// The tasks domain refused it.
    Refused(Report),
    /// The store already holds a task under this harp, one not imported
    /// from this record.
    HarpTaken,
    /// A record to a task whose add was not imported.
    TaskNotImported,
    /// One of the record's tags, not a tag in tagma's grammar, or refused
    /// whatever the schema ([`tags::schema_free`]); the rest of the record
    /// was imported.
    Tag(TagRefusal),
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for Why {
    fn code(&self) -> &'static str {
        match self {
            Why::Displaced => "tasks-application-import-displaced",
            Why::Refused(rejection) => rejection.code(),
            Why::HarpTaken => "tasks-application-import-harp-taken",
            Why::TaskNotImported => "tasks-application-import-task-not-imported",
            Why::Tag(refusal) => refusal.code(),
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        match self {
            Why::Refused(rejection) => rejection.args(),
            Why::Tag(refusal) => refusal.args(),
            _ => Vec::new(),
        }
    }
}

/// Why a project's import stopped before it began: Go taskloom keeps no
/// task log for the project.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NoJsonlLog {
    pub project: String,
}

impl fmt::Display for NoJsonlLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for NoJsonlLog {
    fn code(&self) -> &'static str {
        "tasks-application-no-jsonl-log"
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        vec![("project", self.project.as_str().into())]
    }
}

/// The import use case, as a driving adapter calls it. Its future is
/// `Send`, so a server can spawn it.
pub trait ImportService {
    fn import(
        &mut self,
        request: ImportRequest,
    ) -> impl Future<Output = Result<ImportReport, AppError>> + Send;
}

/// The import over its ports: `jsonl_logs` are the logs it reads, and only
/// it holds them, so no other use case can reach them; `identity` and
/// `stores` resolve and open each project it writes; `delay` waits, with
/// jitter, between retries.
pub struct Importer<I, S, D, L> {
    pub identity: I,
    pub stores: S,
    pub delay: D,
    pub jsonl_logs: L,
    pub backoff: Backoff,
    registry: Registry,
    /// The log the kernel writes dead letters to, taken into the
    /// report.
    pub(crate) dead_letters: DeadLetters,
}

impl<I, S, D, L> Importer<I, S, D, L> {
    pub fn new(identity: I, stores: S, delay: D, jsonl_logs: L) -> Self {
        Importer {
            identity,
            stores,
            delay,
            jsonl_logs,
            backoff: Backoff::DEFAULT,
            registry: registry(),
            dead_letters: DeadLetters::default(),
        }
    }
}

impl<I, S, D, L> ImportService for Importer<I, S, D, L>
where
    I: ProjectIdentity + Send,
    S: TaskStores + Send,
    D: Delay + Jitter + Send,
    L: JsonlTaskLogs + Send,
{
    async fn import(&mut self, request: ImportRequest) -> Result<ImportReport, AppError> {
        let projects = match request.project {
            Some(project) => vec![project],
            None => self.jsonl_logs.projects().map_err(AppError::Failed)?,
        };
        let mut report = ImportReport::default();
        for project_id in projects {
            let mut project = ProjectImport {
                project_id,
                ..ProjectImport::default()
            };
            if let Err(why) = self.import_project(&mut project).await {
                project.failed = Some(why);
            }
            report.projects.push(project);
        }
        report.dead_letters = self.dead_letters.take();
        Ok(report)
    }
}

impl<I, S, D, L> Importer<I, S, D, L>
where
    I: ProjectIdentity + Send,
    S: TaskStores + Send,
    D: Delay + Jitter + Send,
    L: JsonlTaskLogs + Send,
{
    /// Import one project's log, counting into `report` as it goes.
    async fn import_project(&mut self, report: &mut ProjectImport) -> Result<(), Report> {
        let id = report.project_id.clone();
        let records = self.jsonl_logs.records(&id)?.ok_or_else(|| {
            Report::of(&NoJsonlLog {
                project: id.clone(),
            })
        })?;
        report.records = records.len();
        let (records, repeated) = replay_order(records);
        report.repeated = repeated;
        if records.is_empty() {
            return Ok(());
        }
        // An explicit id resolves without binding any directory to it.
        let project = self.identity.for_write(Some(&id))?;
        let store = self.stores.open(&project).await.map_err(store_report)?;
        let mut fold = Fold::new(&records);
        for record in &records {
            let step = fold.step(record);
            for refusal in fold.refused.drain(..) {
                report.lost.push(lost(record, Why::Tag(refusal)));
            }
            let command = match step {
                Step::Run(command) => command,
                Step::NothingLeft => continue,
                Step::Unchanged => {
                    report.unchanged += 1;
                    continue;
                }
                Step::Ignored => {
                    report.ignored += 1;
                    continue;
                }
                Step::Repaired => {
                    report.repaired += 1;
                    continue;
                }
                Step::Lost(why) => {
                    report.lost.push(lost(record, why));
                    continue;
                }
            };
            let cover = cover(&record.task);
            let command_id = record_command_id(&record.raw);
            let stream = StreamRef::try_from(&cover).map_err(store_report)?;
            let id = angzarr_embedded::next::angzarr_ports::Uuid::try_from(&command_id)
                .expect("a derived command id is 16 bytes");
            let found = store.find_command(stream, id).await.map_err(store_report)?;
            if !found.is_empty() {
                report.already += 1;
                continue;
            }
            let mut at = At(record.ts);
            let mut runner = retry::Runner {
                registry: &self.registry,
                clock: &mut at,
                delay: &mut self.delay,
                backoff: self.backoff,
                dead_letters: &self.dead_letters,
            };
            let book = book(&record.task, command);
            match runner
                .run(store, &book, &command_id)
                .await
                .map_err(|e| Report::of(&e))?
            {
                retry::Outcome::Committed => {
                    report.applied += 1;
                    if record.op == JsonlOp::Add {
                        report.tasks += 1;
                    }
                }
                retry::Outcome::Refused(rejection) => {
                    if record.op == JsonlOp::Add {
                        fold.not_imported(&record.task);
                    }
                    let why = match rejection.code() {
                        "tasks-task-exists" => Why::HarpTaken,
                        _ => Why::Refused(rejection),
                    };
                    report.lost.push(lost(record, why));
                }
            }
        }
        Ok(())
    }
}

fn lost(record: &JsonlRecord, why: Why) -> Lost {
    Lost {
        line: record.line,
        op: record.op.as_str(),
        harp_id: record.task.clone(),
        text: record.text.clone(),
        why,
    }
}

/// A clock stopped at the instant a record was recorded.
struct At(Timestamp);

impl Clock for At {
    fn now(&mut self) -> Timestamp {
        self.0
    }
}

/// The records in replay order, each line once, and how many lines
/// repeated an earlier one.
fn replay_order(records: Vec<JsonlRecord>) -> (Vec<JsonlRecord>, usize) {
    let total = records.len();
    let mut seen = BTreeSet::new();
    let mut once: Vec<JsonlRecord> = records
        .into_iter()
        .filter(|r| seen.insert(r.raw.clone()))
        .collect();
    // Stable: records at one instant keep the log's order.
    once.sort_by_key(|r| (r.ts.seconds, r.ts.nanos));
    let repeated = total - once.len();
    (once, repeated)
}

/// What one record comes to.
#[derive(Debug, PartialEq)]
enum Step {
    /// Send this command to the record's task.
    Run(TaskCommand),
    /// A retag that changes nothing.
    Unchanged,
    /// The fold ignores it.
    Ignored,
    /// A displaced add that a repair re-added.
    Repaired,
    /// It cannot be brought over.
    Lost(Why),
    /// A retag that changed the logged tags only by tags left out,
    /// refused.
    NothingLeft,
}

/// The log's fold, as far as it decides which records count: the harps
/// the log has issued, the live tasks' tags, and the repairs made.
struct Fold {
    issued: BTreeSet<String>,
    live: BTreeMap<String, BTreeSet<String>>,
    /// The trigger Go holds for each live task, whatever its status.
    triggers: BTreeMap<String, String>,
    repaired: BTreeSet<String>,
    /// Harps whose add was not imported.
    missing: BTreeSet<String>,
    /// The tags of the last record stepped that are refused: not tags in
    /// tagma's grammar, or refused whatever the schema; left out of its
    /// command.
    refused: Vec<TagRefusal>,
}

impl Fold {
    /// The fold of `records`, which must be in replay order; the repairs
    /// are read ahead, since a repair counts wherever it falls.
    fn new(records: &[JsonlRecord]) -> Fold {
        Fold {
            issued: BTreeSet::new(),
            live: BTreeMap::new(),
            triggers: BTreeMap::new(),
            repaired: records
                .iter()
                .filter(|r| r.op == JsonlOp::Add && !r.repair_of.is_empty())
                .map(|r| r.repair_of.clone())
                .collect(),
            missing: BTreeSet::new(),
            refused: Vec::new(),
        }
    }

    /// `spelled` as tags, those refused left out and kept in
    /// [`Fold::refused`].
    fn parsed<'a>(&mut self, spelled: impl IntoIterator<Item = &'a String>) -> Vec<Tag> {
        let mut parsed = Vec::new();
        for result in spelled.into_iter().filter_map(|s| importable(s)) {
            match result {
                Ok(tag) => parsed.push(tag),
                Err(refusal) => self.refused.push(refusal),
            }
        }
        parsed
    }

    /// Keep in [`Fold::refused`] those of `spelled` that are refused;
    /// whether there were any.
    fn refused_of(&mut self, spelled: &BTreeSet<String>) -> bool {
        let before = self.refused.len();
        self.parsed(spelled);
        self.refused.len() > before
    }

    /// The task `harp_id`'s add was not imported.
    fn not_imported(&mut self, harp_id: &str) {
        self.missing.insert(harp_id.to_string());
    }

    fn step(&mut self, record: &JsonlRecord) -> Step {
        if record.op == JsonlOp::Add {
            return self.add(record);
        }
        let harp = &record.task;
        let Some(held) = self.live.get_mut(harp) else {
            return Step::Ignored;
        };
        // The fold changes whether or not the task was imported, so a
        // later record is judged against the log as written.
        let command: Option<TaskCommand> = match record.op {
            JsonlOp::Tag | JsonlOp::Untag => {
                let given = normalized(&record.tags);
                let after: BTreeSet<String> = if record.op == JsonlOp::Tag {
                    held.union(&given).cloned().collect()
                } else {
                    held.difference(&given).cloned().collect()
                };
                // The task's tags are the logged ones, parsed: two
                // spellings of one tag (`k=v`, `k="v"`) are one tag here, so
                // the retag takes the task from the logged tags before to
                // the logged tags after, as tags, whatever the record named.
                let before = parsed_set(held);
                let now = parsed_set(&after);
                *held = after;
                let refused = self.refused_of(&given);
                let add: Vec<Tag> = now
                    .iter()
                    .filter(|(spelled, _)| !before.contains_key(*spelled))
                    .map(|(_, tag)| tag.clone())
                    .collect();
                let remove: Vec<Tag> = before
                    .iter()
                    .filter(|(spelled, _)| !now.contains_key(*spelled))
                    .map(|(_, tag)| tag.clone())
                    .collect();
                if add.is_empty() && remove.is_empty() {
                    let step = if refused {
                        Step::NothingLeft
                    } else {
                        Step::Unchanged
                    };
                    return self.unless_missing(harp, step);
                }
                Some(Retag { add, remove }.into())
            }
            JsonlOp::Status => {
                // Go's empty trigger keeps the one held, never clears it.
                let go_held = self.triggers.entry(harp.clone()).or_default();
                if let Some(trigger) = left_out(&record.trigger) {
                    *go_held = trigger.trim().to_string();
                }
                Some(
                    SetStatus {
                        status: left_out(&record.status),
                        trigger: deferring_on(&record.status, go_held),
                        defers: DEFERS.to_string(),
                        initial: INITIAL.to_string(),
                    }
                    .into(),
                )
            }
            JsonlOp::Text => Some(
                EditText {
                    text: record.text.clone(),
                }
                .into(),
            ),
            // An add was taken above.
            JsonlOp::Remove | JsonlOp::Add => {
                self.live.remove(harp);
                self.triggers.remove(harp);
                Some(
                    RemoveTask {
                        reason: RemovalReason::Removed,
                    }
                    .into(),
                )
            }
        };
        self.unless_missing(harp, command.map_or(Step::Unchanged, Step::Run))
    }

    /// `step`, unless `harp`'s add was not imported.
    fn unless_missing(&mut self, harp: &str, step: Step) -> Step {
        if self.missing.contains(harp) {
            self.refused.clear();
            return Step::Lost(Why::TaskNotImported);
        }
        step
    }

    /// An add: the task, unless the harp is already issued.
    fn add(&mut self, record: &JsonlRecord) -> Step {
        let harp = &record.task;
        if !self.issued.insert(harp.clone()) {
            return if self.repaired.contains(&anomaly_key(record)) {
                Step::Repaired
            } else {
                Step::Lost(Why::Displaced)
            };
        }
        self.live.insert(harp.clone(), normalized(&record.tags));
        let go_held = record.trigger.trim().to_string();
        let trigger = deferring_on(&record.status, &go_held);
        self.triggers.insert(harp.clone(), go_held);
        let tags = self.parsed(&record.tags);
        Step::Run(
            AddTask {
                harp_id: harp.clone(),
                text: record.text.clone(),
                status: left_out(&record.status),
                trigger,
                tags,
                origin_session: left_out(&record.session),
                defers: DEFERS.to_string(),
                initial: INITIAL.to_string(),
            }
            .into(),
        )
    }
}

/// Go's `value`, its empty string read as left out.
fn left_out(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

/// The trigger a record's command gives: Go's held one, `go_held`, when the
/// record's `status` defers the task (none, if Go holds none); otherwise
/// none, since a trigger is refused with another status.
fn deferring_on(status: &str, go_held: &str) -> Option<String> {
    match status == DEFERS {
        true => left_out(go_held),
        false => None,
    }
}

/// `spelled` as a tag an import may bring over: one in tagma's grammar
/// that passes [`tags::schema_free`]; `None` when it is blank.
fn importable(spelled: &str) -> Option<Result<Tag, TagRefusal>> {
    Some(tags::parse(spelled)?.and_then(|tag| tags::schema_free(&tag).map(|()| tag)))
}

/// The tags of `spelled` an import may bring over, by their canonical
/// spelling.
fn parsed_set(spelled: &BTreeSet<String>) -> BTreeMap<String, Tag> {
    spelled
        .iter()
        .filter_map(|s| importable(s)?.ok())
        .map(|tag| (tag.to_string(), tag))
        .collect()
}

/// Tags trimmed, empties dropped, deduplicated and sorted.
fn normalized(tags: &[String]) -> BTreeSet<String> {
    tags.iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// What a command id derived from a log record's line is prefixed with, so
/// it can equal no other id derived the same way. It keeps the crate's old
/// name: the ids of records already imported derive from it, and a new
/// prefix would import each of them again.
const RECORD_ID_PREFIX: &str = "ctxloom-go-task-log:";

/// The command id a record imports under: a UUIDv5 of its line. A record
/// is an external fact, and this is the id angzarr gives a fact
/// (`ids::fact_command_id`) whose external id is the line, prefixed. The
/// fold counts a line once, so within a log the line is the record's
/// identity.
pub fn record_command_id(raw: &str) -> Uuid {
    ids::fact_command_id(&format!("{RECORD_ID_PREFIX}{raw}"))
}

/// The key a repair add names a displaced add by: the text hash of its
/// harp, text, session and instant, NUL-separated.
pub fn anomaly_key(record: &JsonlRecord) -> String {
    text_hash(&format!(
        "{}\0{}\0{}\0{}",
        record.task,
        record.text,
        record.session,
        rfc3339_nano(record.ts)
    ))
}

/// `at` in UTC in RFC 3339 with nanoseconds: the fraction's trailing zeros
/// dropped, and the fraction too when it is zero.
pub fn rfc3339_nano(at: Timestamp) -> String {
    let (year, month, day) = civil(at.seconds.div_euclid(86_400));
    let second = at.seconds.rem_euclid(86_400);
    let mut text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        second / 3600,
        second % 3600 / 60,
        second % 60
    );
    if at.nanos != 0 {
        let fraction = format!("{:09}", at.nanos);
        text.push('.');
        text.push_str(fraction.trim_end_matches('0'));
    }
    text.push('Z');
    text
}

/// The proleptic Gregorian date `days` after 1970-01-01: Howard Hinnant's
/// `civil_from_days`.
fn civil(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
#[path = "import.test.rs"]
mod tests;
