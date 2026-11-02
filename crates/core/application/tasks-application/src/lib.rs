//! The tasks use cases, each a unit of its own over only the ports it
//! uses, borrowed from one [`Service`] that owns the ports and composes
//! the units behind the traits a driving adapter calls:
//!
//! - add a task ([`TaskService::add`]);
//! - change one: its status, deferring it on a trigger or reviving it
//!   ([`TaskService::set_status`]), its text ([`TaskService::edit`]), its
//!   tags ([`TaskService::tag`]);
//! - read them: list, show, count, the statuses and tags in use, lint
//!   ([`TaskService::list`] and the rest), and the store's rules
//!   ([`TaskService::tag_schema`], [`TaskService::permits`],
//!   [`SchemaService`]);
//! - prune old finished ones ([`PruneService`]);
//! - grant and revoke tag permits, and declare the store's rules
//!   ([`PermitService`], a person's, apart so a frontend can leave it out);
//! - watch a project's store change ([`WatchService`]);
//! - and, apart from them, import the JSONL task logs ([`Importer`], the
//!   one use case that reads them).
//!
//! Each reaches its project through one collaborator (`project`:
//! [`ProjectIdentity`], [`TaskStores`], the configuration's moved keys
//! refused), and a write runs its commands through angzarr-embedded's
//! kernel through kernel-runner's [`Runner`] (`write`), which retries
//! them ([`KERNEL_RETRY`]: the kernel owns race re-runs, its waits
//! taskloom's ports); a read is the task-list projector caught up from the
//! store (`lists`). The ports are the only way out: this crate does no
//! I/O, keeps no clock and starts no runtime.

mod add;
mod calendar;
mod change;
mod error;
mod import;
mod lint;
mod lists;
mod permit;
mod priority;
mod project;
mod prune;
mod query;
mod read;
mod request;
mod rules;
mod schema;
mod service;
mod tags;
mod task;
mod view;
mod watch;
mod write;

use angzarr_diagnostic::{Diagnostic, Report};
use angzarr_embedded::{KernelError, StoreError};

/// A dead letter, as the kernel answers it and the store keeps it: angzarr's
/// `AngzarrDeadLetter` whole, keyed by the stream it is about. A use case
/// returns every one its kernel calls answered, beside its result or on
/// its error, for its driving adapter to say loudly ([`dead_letter_line`]):
/// each almost certainly marks a bug. A command taskloom waits for is
/// answered, never dead-lettered.
pub use angzarr_embedded::{dead_letter_line, DeadLetter};

/// A dead letter as the kernel answers one, from `component`, about the
/// task swift-amber's stream, its line `DEAD_LETTER <component>:
/// ROUTER_REASON_UNHANDLED_HANDLER_ERROR`: for a test that hands a use
/// case, or a driving adapter, a letter a kernel call answered.
#[cfg(any(test, feature = "test-support"))]
pub fn dead_letter_sample(component: &str) -> DeadLetter {
    use angzarr_embedded::v1::{AngzarrDeadLetter, Diagnostic};
    DeadLetter {
        stream: task_cover("swift-amber"),
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
pub use add::{NewTask, KEYED_PROBES};
pub use change::{StatusChange, TagChange, TextEdit};
#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{AppError, CATALOGS, EN_US};
pub use import::{ImportReport, ImportRequest, ImportService, Importer, Lost, ProjectImport, Why};
pub use lint::{Finding, LintReport, Violation};
pub use permit::{PermitChange, PermitList, PermitService, Permitted};
pub use ports::Timestamp;
pub use ports::{NamespacePermit, Project, TagPermits};
pub use priority::{PriorityNote, PriorityWarning};
pub use prune::{
    Dangling, PruneRequest, PruneService, Pruned, Pruning, Via, DEFAULT_OLDER_THAN,
    DEFAULT_PRUNE_STATUSES,
};
pub use read::{
    Details, Hidden, ListQuery, Listing, Reach, Scope, ShowQuery, Statuses, Summary, TagCount,
    Vocabulary,
};
pub use request::{request_command_id, REQUEST_NAMESPACE};
pub use rules::{
    Code as RuleCode, Finding as RuleFinding, Origin, RuleWarning, RulesRefusal, SchemaSummary,
    TargetSummary, SEEDED_SCHEMA, TASKLOOM_NAMESPACE,
};
pub use schema::{Declaration, Declared, SchemaService};
pub use service::{registry, NotConfigured, Service, TaskService};
/// SemVer 2.0.0, as a `type=semver` target orders by: tagma's.
pub use tagma_core::semver::{Semver, Version, SEMVER};
/// A finding's tag is tagma's own type, as tagma reports it.
pub use tagma_core::Tag as RuleTag;
pub use tags::{
    is_retired as tag_is_retired, parse as parse_tag, schema_free as schema_free_tag, Tag,
    RETIRED_TAG,
};
pub use task::Written;
pub use view::{text_hash, Row, Shown, StatusInfo, Task};
pub use watch::{Watch, WatchRequest, WatchService};

// Used across the crate's modules by these paths; nothing outside uses them.
#[cfg(test)]
pub(crate) use add::HARP_ATTEMPTS;
pub(crate) use tags::TagRefusal;
pub(crate) use task::task_cover;
pub(crate) use view::DONE;

/// A store's failure, as the kernel reports one: its key and args.
pub(crate) fn store_report(error: StoreError) -> Report {
    Report::of(&KernelError::Store(error))
}

/// A store's failure, as the use cases fail with it.
pub(crate) fn failed(error: StoreError) -> AppError {
    AppError::Failed(store_report(error))
}

pub(crate) fn refused(refusal: &TagRefusal) -> AppError {
    AppError::TagRefused(Report::of(refusal))
}

pub(crate) fn rejected(rejection: &dyn Diagnostic) -> AppError {
    AppError::Rejected(Report::of(rejection))
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;

#[cfg(test)]
#[path = "english.test.rs"]
mod english;

#[cfg(test)]
#[path = "fakes.test.rs"]
mod fakes;
