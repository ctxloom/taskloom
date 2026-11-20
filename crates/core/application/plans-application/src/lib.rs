//! The plans use cases over plan files: `list`, `show`, `add`, `set`,
//! `tag`, `link`, `unlink`, `check` and `stamp`, and the snapshots a watch
//! compares ([`Snapshot`], [`changes`]).
//!
//! Plans are files and nothing else: no aggregate, no projection, nothing
//! in the store. A plan is any `*.plan.md` under the plan roots; its YAML
//! frontmatter is the one authority for `id`, `title`, `status`, `tags`,
//! `governs`, `supersedes` and `sessions` ([`front`]); taskloom edits the
//! frontmatter through angzarr-yaml's editor and touches no other byte,
//! minting the `id:` (a harp) the first time it writes a file. `superseded`
//! is never written: it is derived from another plan's `supersedes:`.
//!
//! [`PlanService`] runs the use cases over four ports: the plan files
//! ([`PlanFiles`]), the project's tasks and rules ([`Tasks`]: whether a
//! task exists, and the store's tag rules a plan's tags are held to, as a
//! task's are, through `tag-rules`), the harp mint ([`Mint`]) and the
//! configuration ([`Configuration`]: `plan_roots`). The ports are the only
//! way out: this crate does no I/O, keeps no clock and starts no runtime.

mod error;
pub mod front;
mod plan;
mod rules;
mod service;
mod tasks;
mod watch;

pub use front::{Edit, FrontError, Meta};
pub use ports::{Configuration, Mint, PlanFiles, Tasks};

#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{Finding, PlanError, CATALOGS, EN_US};
pub use plan::{Plan, DEFAULT_STATUS, STATUSES, SUPERSEDED};
pub use rules::{frontmatter, plan_name, Frontmatter};
pub use service::{PlanService, Plans, MINT_ATTEMPTS};
pub use tasks::ProjectTasks;
pub use watch::{changes, Change, Delta, Event, Snapshot, Watched};

/// Where a plan file goes when `plans add` names no path and no root is
/// configured: `<work-root>/.taskloom/plans/<harp-id>.plan.md`, committed
/// with the work tree.
pub const PLANS_DIR: &str = ".taskloom/plans";
/// A plan file's suffix.
pub const PLAN_SUFFIX: &str = ".plan.md";

/// The plans a listing selects.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PlanQuery {
    /// Only plans in one of these statuses (derived `superseded` too);
    /// every plan when empty.
    pub statuses: Vec<String>,
    /// Only plans whose `governs:` names this task.
    pub governs: Option<String>,
    /// Only plans whose tags a postfix tag query selects.
    pub tag_query: Option<String>,
}

/// A plan to write.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct NewPlan {
    pub title: String,
    /// Where to write it: absolute, or the work root's; `.plan.md`.
    pub path: Option<String>,
    /// draft (the default), active, done or abandoned.
    pub status: Option<String>,
    /// Tasks it governs; each must exist.
    pub governs: Vec<String>,
}

/// Fields of a plan to set; at least one.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SetPlan {
    pub title: Option<String>,
    pub status: Option<String>,
}

/// A link a plan makes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Link {
    /// To a task it governs.
    Governs(String),
    /// To a plan it supersedes (an id, or a path).
    Supersedes(String),
}

/// A plan with its file's text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlanText {
    pub plan: Plan,
    pub content: String,
}

/// What `check` found.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CheckReport {
    pub findings: Vec<Finding>,
}

impl CheckReport {
    /// Whether anything was found.
    pub fn failed(&self) -> bool {
        !self.findings.is_empty()
    }
}

/// What `stamp` did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stamp {
    /// The session was written to the file's `sessions:` (and an id, if
    /// the file had none).
    Stamped,
    /// The file already held the session and an id.
    Already,
    /// There is no session to stamp.
    NoSession,
    /// The path is no plan file, or is not there.
    NotAPlan,
}

#[cfg(test)]
#[path = "fakes.test.rs"]
mod fakes;
