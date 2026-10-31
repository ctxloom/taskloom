//! The plans use cases: `plan add`, `status`, `link`, `unlink`, `list`,
//! `show`, `check` and `import`, over the plan aggregate, the plan list,
//! and three ports: the work tree's files ([`PlanFiles`]), the project's
//! tasks ([`Tasks`]) and the harp mint ([`Mint`]).
//!
//! RED STUB (docs/plans/plans-aggregate.md): every body `todo!()`.
//! Implemented, the ports move to `core/ports`, the service runs over
//! angzarr-embedded's kernel (async, at the head it read, under the
//! `Backoff` loop) as `TaskService` does, and the errors get codes and an
//! en-US catalog. Here it is synchronous, so the interface reads plainly.
//!
//! The rules the aggregate cannot see are here: a `governs` link names a
//! task that exists, a `supersedes` link a plan that exists and closes no
//! cycle, and a path resolves inside the work root. The text is a file, the
//! store holds its path and hash, and `check` reports where they differ.

// RED STUB: the bodies are `todo!()`, so their arguments are unused.
#![allow(unused_variables, dead_code)]

pub use plans::{PlanRejection, PlanStatus};

/// Where a plan's text goes when `plan add` names no path:
/// `<work-root>/.taskloom/plans/<harp-id>.plan.md`.
pub const PLANS_DIR: &str = ".taskloom/plans";
/// A plan file's suffix.
pub const PLAN_SUFFIX: &str = ".plan.md";
/// The task tag `plan import` retires: `ctxloom:plan="<path>"`.
pub const RETIRED_TAG: &str = "ctxloom:plan";

// ---- ports (move to core/ports when implemented) ----

/// The work tree's files, as the file-system seam reaches them. A path is
/// work-root-relative, `/`-separated, unless it is absolute.
pub trait PlanFiles {
    /// The work root, absolute, `/`-separated.
    fn work_root(&self) -> String;
    /// The file's bytes, or None when it is missing or unreadable.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// Create the file (and its directories) holding `text`; false, writing
    /// nothing, when it already exists.
    fn create(&self, path: &str, text: &[u8]) -> bool;
    /// The `*.plan.md` files directly under `dir`, as work-root-relative
    /// paths, sorted.
    fn plan_files(&self, dir: &str) -> Vec<String>;
}

/// The project's tasks, as far as plans need them.
pub trait Tasks {
    fn exists(&self, harp_id: &str) -> bool;
    /// Every task carrying a tag keyed `key` (namespace:key), with each of
    /// its values: (harp id, value).
    fn tagged(&self, key: &str) -> Vec<(String, String)>;
    /// Remove the tag `key="value"` from the task.
    fn untag(&self, harp_id: &str, key: &str, value: &str);
}

/// Harp ids for new plans, from the same mint as tasks'.
pub trait Mint {
    fn mint(&self) -> String;
}

// ---- pure rules ----

/// Why a path cannot be a plan's.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PathError {
    /// It leaves the work root (`..` past it, or absolute elsewhere).
    OutsideWorkRoot(String),
    /// It does not end `.plan.md`.
    NotAPlanFile(String),
    Empty,
}

/// `given` (from `plan add --path`) as the work-root-relative path the
/// store records: normalized (`.` and `..` resolved, `/`-separated), an
/// absolute path inside `work_root` made relative.
pub fn resolve_path(work_root: &str, given: &str) -> Result<String, PathError> {
    todo!("RED: resolve_path")
}

/// The path a plan's file has when `plan add` names none.
pub fn default_path(harp_id: &str) -> String {
    todo!("RED: default_path")
}

/// sha-256 of `text`, 64 lowercase hex digits.
pub fn content_hash(text: &[u8]) -> String {
    todo!("RED: content_hash")
}

/// What a plan file's YAML frontmatter says, as far as plans read it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Frontmatter {
    pub title: String,
    pub sessions: Vec<String>,
}

/// The frontmatter of `text`: `title:` and the `sessions:` list, empty
/// when it has none.
pub fn frontmatter(text: &str) -> Frontmatter {
    todo!("RED: frontmatter")
}

/// A plan's name: its file's base name without `.plan.md` (or `.md`).
pub fn plan_name(path: &str) -> String {
    todo!("RED: plan_name")
}

/// Whether a task tag keyed `key` is retired, so a write adding it is
/// refused (wired into the tasks' tag admission when implemented).
pub fn tag_is_retired(key: &str) -> bool {
    todo!("RED: tag_is_retired")
}

// ---- the use cases ----

/// Why a plan use case failed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PlanError {
    /// The aggregate refused.
    Rejected(PlanRejection),
    NoSuchPlan(String),
    /// A `governs` link to a task the project does not hold.
    NoSuchTask(String),
    /// `plan` superseding `target` would close a cycle.
    SupersedeCycle {
        plan: String,
        target: String,
    },
    Path(PathError),
    /// `plan add --path` naming a file that is not there.
    FileMissing(String),
    /// `plan add` with no path, and the default path's file already exists.
    FileExists(String),
    /// A path another plan's text already is.
    AlreadyRegistered {
        path: String,
        plan: String,
    },
}

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

/// A typed link, as `plan link` and `plan unlink` name it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Link {
    Governs(String),
    Supersedes(String),
}

/// `plan list`'s filters.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PlanQuery {
    /// Any of these; empty is every status.
    pub statuses: Vec<PlanStatus>,
    /// Only plans governing this task.
    pub governs: Option<String>,
}

/// A plan as the use cases answer it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PlanView {
    pub harp_id: String,
    pub title: String,
    pub status: PlanStatus,
    /// As recorded: work-root-relative, or absolute for an external one.
    pub path: String,
    /// Absolute: the work root joined with `path`.
    pub abs_path: String,
    /// [`plan_name`] of the path.
    pub name: String,
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

/// `plan show`: the plan and its text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlanText {
    pub plan: PlanView,
    /// None when the file is missing.
    pub content: Option<String>,
}

/// What `plan check` found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Finding {
    /// The recorded file is not there.
    Missing { plan: String, path: String },
    /// The file's hash is not the one recorded.
    HashMismatch {
        plan: String,
        path: String,
        recorded: String,
        actual: String,
    },
    /// The text is outside the work root, so not committed with it.
    External { plan: String, path: String },
    /// A `.taskloom/plans/*.plan.md` no plan records.
    Unregistered { path: String },
    /// A governed task the project no longer holds (pruned).
    MissingTask { plan: String, task: String },
}

impl Finding {
    /// Whether it fails `plan check` (exit 1): all but External.
    pub fn fails(&self) -> bool {
        todo!("RED: Finding::fails")
    }
}

/// `plan check`'s answer, findings sorted by plan then path.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CheckReport {
    pub findings: Vec<Finding>,
}

impl CheckReport {
    pub fn failed(&self) -> bool {
        todo!("RED: CheckReport::failed")
    }
}

/// One `ctxloom:plan` value `plan import` registered (or found already
/// registered) and the tasks it now governs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImportedPlan {
    pub harp_id: String,
    pub path: String,
    /// Outside the work root, registered by absolute path.
    pub external: bool,
    /// Added by this import (false: a plan already recording the path).
    pub new: bool,
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

/// The plans use cases, as a driving adapter reaches them.
pub trait Plans {
    fn add(&mut self, new: NewPlan) -> Result<PlanView, PlanError>;
    fn set_status(&mut self, plan: &str, status: PlanStatus) -> Result<PlanView, PlanError>;
    fn link(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError>;
    fn unlink(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError>;
    fn list(&self, query: &PlanQuery) -> Vec<PlanView>;
    fn show(&self, id_or_path: &str) -> Result<PlanText, PlanError>;
    fn check(&self) -> CheckReport;
    fn accept(&mut self, plan: &str) -> Result<PlanView, PlanError>;
    fn import(&mut self, dry_run: bool) -> ImportReport;
}

/// The plans use cases.
pub struct PlanService<'a> {
    files: &'a dyn PlanFiles,
    tasks: &'a dyn Tasks,
    mint: &'a dyn Mint,
}

impl<'a> PlanService<'a> {
    /// The use cases over an empty plan store (the stub's: implemented, it
    /// takes the project's store and kernel as `TaskService` does).
    pub fn new(files: &'a dyn PlanFiles, tasks: &'a dyn Tasks, mint: &'a dyn Mint) -> Self {
        PlanService { files, tasks, mint }
    }
}

impl Plans for PlanService<'_> {
    fn add(&mut self, new: NewPlan) -> Result<PlanView, PlanError> {
        todo!("RED: PlanService::add")
    }

    fn set_status(&mut self, plan: &str, status: PlanStatus) -> Result<PlanView, PlanError> {
        todo!("RED: PlanService::set_status")
    }

    fn link(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        todo!("RED: PlanService::link")
    }

    fn unlink(&mut self, plan: &str, link: Link) -> Result<PlanView, PlanError> {
        todo!("RED: PlanService::unlink")
    }

    /// The plans `query` selects, by harp id.
    fn list(&self, query: &PlanQuery) -> Vec<PlanView> {
        todo!("RED: PlanService::list")
    }

    /// The plan `id_or_path` names: a harp id, or a path (relative,
    /// or absolute as `plan list` gives it).
    fn show(&self, id_or_path: &str) -> Result<PlanText, PlanError> {
        todo!("RED: PlanService::show")
    }

    fn check(&self) -> CheckReport {
        todo!("RED: PlanService::check")
    }

    /// `plan check --accept <plan>`: record the file's hash as it now is.
    fn accept(&mut self, plan: &str) -> Result<PlanView, PlanError> {
        todo!("RED: PlanService::accept")
    }

    /// `plan import`: every task's `ctxloom:plan` tags into plans and
    /// governs links, the migrated tags retired. `dry_run` reports and
    /// changes nothing. Running it again adds nothing new.
    fn import(&mut self, dry_run: bool) -> ImportReport {
        todo!("RED: PlanService::import")
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
