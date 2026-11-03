//! The plans use cases: `plan add`, `status`, `link`, `unlink`, `list`,
//! `show`, `check` and `import`, over the plan aggregate, the plan list,
//! and three ports: the work tree's files ([`PlanFiles`]), the project's
//! tasks ([`Tasks`]) and the harp mint ([`Mint`]).
//!
//! [`PlanService`] resolves the project through [`ProjectIdentity`], opens
//! its store through [`TaskStores`] (a project's plans live in the store its
//! tasks do), and runs each command through angzarr-embedded's kernel, at
//! the head it read, the kernel retrying it as every taskloom command is
//! retried (kernel-runner's `KERNEL_RETRY`); a list is the plan-list
//! projector caught up from the store. The ports are the only way out: this
//! crate does no I/O, keeps no clock and starts no runtime.
//!
//! The crate is laid out as tasks-application is (CLAUDE.md, "A context's
//! application crate"): a unit per use case over only the ports it uses
//! (`add.rs` `Adding`, `change.rs` `Changing`: status, link and unlink,
//! `read.rs` `Reading`: list and show, `check.rs` `Checking`, `import.rs`
//! `Importing`), shared collaborators (`project.rs` `Projects`: resolve,
//! open, a plan's stream, the plan list caught up; `write.rs` `Writes`:
//! `Projects` and kernel-runner's `Runner`; `tree.rs` `Tree`: `Writes`
//! and the work tree's files, a plan viewed and its file's hash tracked),
//! and `service.rs`, the [`PlanService`] owning the ports once and lending
//! each call's unit its share.
//!
//! The rules the aggregate cannot see are here: a `governs` link names a
//! task that exists, a `supersedes` link a plan that exists and closes no
//! cycle, and a path resolves inside the work root. The text is a file, the
//! store holds its path and hash, and the hash tracks the file: `list`,
//! `show` and `check` read it and record its new hash when it changed
//! (`Rehashed`). `check` reports a recorded file that is missing, a plan
//! file no plan records, and a governed task the project no longer holds.

mod add;
mod change;
mod check;
mod digest;
mod error;
mod import;
mod project;
mod read;
mod rules;
mod service;
mod tasks;
mod tree;
mod write;

/// A dead letter, as the kernel answers it: angzarr-embedded's.
pub use angzarr_embedded::{dead_letter_line, DeadLetter};
pub use plans::{PlanRejection, PlanStatus};
pub use ports::{Mint, PlanFiles, Project, Tasks};

/// A dead letter as the kernel answers one, from `component`, about the
/// plan brisk-cedar's stream, its line `DEAD_LETTER <component>:
/// ROUTER_REASON_UNHANDLED_HANDLER_ERROR`: for a test that hands a use
/// case, or a driving adapter, a letter a kernel call answered.
#[cfg(any(test, feature = "test-support"))]
pub fn dead_letter_sample(component: &str) -> DeadLetter {
    use angzarr_embedded::v1::{AngzarrDeadLetter, Diagnostic};
    use angzarr_embedded::CoverExt;
    DeadLetter {
        stream: angzarr_embedded::v1::Cover::natural(plans::PLAN_DOMAIN, "brisk-cedar"),
        at: angzarr_embedded::DeadLetterAt::Sequence(0),
        letter: AngzarrDeadLetter {
            source_component: component.to_string(),
            reason: Some(Diagnostic {
                code: "ROUTER_REASON_UNHANDLED_HANDLER_ERROR".to_string(),
                args: Default::default(),
            }),
            ..AngzarrDeadLetter::default()
        },
    }
}

pub use add::NewPlan;
pub use change::Link;
pub use check::{CheckReport, Finding};
pub use digest::{DigestPlans, Folded};
#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{PathError, PlanError, CATALOGS, EN_US};
pub use import::{ImportReport, ImportedPlan, LeftTag};
pub use project::HARP_ATTEMPTS;
pub use read::{PlanQuery, PlanText};
pub use rules::{content_hash, default_path, frontmatter, plan_name, resolve_path, Frontmatter};
pub use service::{registry, Governing, PlanService, Plans};
pub use tasks::ProjectTasks;
pub use tree::PlanView;

/// Where a plan's text goes when `plan add` names no path:
/// `<work-root>/.taskloom/plans/<harp-id>.plan.md`, committed with the work
/// tree. A plan `plan import` finds outside the work root is copied here.
pub const PLANS_DIR: &str = ".taskloom/plans";
/// A plan file's suffix.
pub const PLAN_SUFFIX: &str = ".plan.md";

#[cfg(test)]
#[path = "fakes.test.rs"]
mod fakes;

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
