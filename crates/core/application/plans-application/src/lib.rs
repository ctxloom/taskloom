//! The plans use cases: `plan add`, `status`, `link`, `unlink`, `list`,
//! `show`, `check` and `import`, over the plan aggregate, the plan list,
//! and three ports: the work tree's files ([`PlanFiles`]), the project's
//! tasks ([`Tasks`]) and the harp mint ([`Mint`]).
//!
//! [`PlanService`] resolves the project through [`ProjectIdentity`], opens
//! its store through [`TaskStores`] (a project's plans live in the store its
//! tasks do), and runs each command through angzarr-embedded's kernel, at
//! the head it read and under the tasks' [`Backoff`] loop
//! ([`KERNEL_RETRY`]: the kernel retries nothing); a list is the plan-list
//! projector caught up from the store. The ports are the only way out: this
//! crate does no I/O, keeps no clock and starts no runtime.
//!
//! The rules the aggregate cannot see are here: a `governs` link names a
//! task that exists, a `supersedes` link a plan that exists and closes no
//! cycle, and a path resolves inside the work root. The text is a file, the
//! store holds its path and hash, and the hash tracks the file: `list`,
//! `show` and `check` read it and record its new hash when it changed
//! (`Rehashed`). `check` reports a recorded file that is missing and a plan
//! file no plan records.

mod error;
mod import;
mod rules;
mod service;
mod tasks;

pub use plans::{tag_is_retired, PlanRejection, PlanStatus, RETIRED_TAG};
pub use ports::{Mint, PlanFiles, Tasks};
pub use tasks_application::{Backoff, DeadLetter, KERNEL_RETRY};

#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{PathError, PlanError, CATALOGS, EN_US};
pub use rules::{content_hash, default_path, frontmatter, plan_name, resolve_path, Frontmatter};
pub use service::{registry, PlanService, HARP_ATTEMPTS};
pub use tasks::ProjectTasks;

use std::future::Future;

/// Where a plan's text goes when `plan add` names no path:
/// `<work-root>/.taskloom/plans/<harp-id>.plan.md`, committed with the work
/// tree. A plan `plan import` finds outside the work root is copied here.
pub const PLANS_DIR: &str = ".taskloom/plans";
/// A plan file's suffix.
pub const PLAN_SUFFIX: &str = ".plan.md";

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
    /// As recorded: work-root-relative.
    pub path: String,
    /// Absolute: the work root joined with `path`.
    pub abs_path: String,
    /// [`plan_name`] of the path.
    pub name: String,
    /// The file's hash as last recorded: as it is now, after a read that
    /// found it changed.
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

/// What `plan check` found: drift between the store and the work tree that
/// tracking the hash cannot mend.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Finding {
    /// The recorded file is not there.
    Missing { plan: String, path: String },
    /// A `.taskloom/plans/*.plan.md` no plan records.
    Unregistered { path: String },
}

/// `plan check`'s answer: the missing files by plan, then the unregistered
/// ones by path. Every finding fails the check.
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

/// The plans use cases, as a driving adapter reaches them. Their futures
/// are not promised `Send` (see [`ports::Tasks`]).
pub trait Plans {
    fn add(&mut self, new: NewPlan) -> impl Future<Output = Result<PlanView, PlanError>>;
    fn set_status(
        &mut self,
        plan: &str,
        status: PlanStatus,
    ) -> impl Future<Output = Result<PlanView, PlanError>>;
    fn link(&mut self, plan: &str, link: Link)
        -> impl Future<Output = Result<PlanView, PlanError>>;
    fn unlink(
        &mut self,
        plan: &str,
        link: Link,
    ) -> impl Future<Output = Result<PlanView, PlanError>>;
    /// The plans `query` selects, by harp id; each file read, and its new
    /// hash recorded where it changed.
    fn list(&mut self, query: &PlanQuery)
        -> impl Future<Output = Result<Vec<PlanView>, PlanError>>;
    /// The plan `id_or_path` names: a harp id, or a path (relative, or
    /// absolute as `plan list` gives it), and its text.
    fn show(&mut self, id_or_path: &str) -> impl Future<Output = Result<PlanText, PlanError>>;
    /// Every recorded file read (its new hash recorded where it changed),
    /// and what is missing or unregistered reported.
    fn check(&mut self) -> impl Future<Output = Result<CheckReport, PlanError>>;
    /// Every task's `ctxloom:plan` tags into plans and governs links, the
    /// migrated tags retired. `dry_run` reports and changes nothing.
    /// Running it again adds nothing new.
    fn import(&mut self, dry_run: bool) -> impl Future<Output = Result<ImportReport, PlanError>>;
    /// The lines the kernel dead-lettered since the last call: almost
    /// certainly bugs, for the driving adapter to say loudly.
    fn dead_letters(&mut self) -> Vec<DeadLetter> {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "fakes.test.rs"]
mod fakes;

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
