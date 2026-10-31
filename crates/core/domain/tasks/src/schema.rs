//! RED STUB (red/self-hosted-schema, for the owner's review): the store's
//! tag schema as a domain stream (docs/plans/self-hosted-schema.md).
//!
//! One stream per project store, at a fixed root ([`schema_root`]) in its
//! own domain ([`SCHEMA_DOMAIN`]), so it is never listed as a task. It holds
//! the store's tag rules as tagma config tags (`tagma.*`, and taskloom's own
//! `taskloom.*`); [`Declare`] changes them, Retag-shaped, and every change
//! records [`SchemaRetagged`], carrying the whole tag set after it. Ordering
//! is the stream's own: nothing numbers across streams.
//!
//! Every body here is `todo!()`: the tests in `schema.test.rs` are the
//! proposal. The markup aggregate (`SchemaAggregate`, its proto messages in
//! `proto/ctxloom/tasks/v1/schema.proto`) is the implementation's, wired
//! over these same rules, as `TaskAggregate` is over `lib.rs`'s.

use std::fmt;

use angzarr_embedded::next::v1::Uuid;
use angzarr_embedded_diagnostic::{Arg, Diagnostic};

use crate::Tag;

/// The schema stream's domain: its `Cover.domain`. Not [`crate::TASK_DOMAIN`],
/// so a task list, which projects the tasks domain, never lists it.
pub const SCHEMA_DOMAIN: &str = "schema";

/// The name the schema stream's root derives from
/// (`ids::compute_root(SCHEMA_DOMAIN, SCHEMA_STREAM)`): one per store.
pub const SCHEMA_STREAM: &str = "store";

/// The namespaces a schema tag may be in: tagma's facets, and taskloom's
/// own (`taskloom.priority_fn`, `taskloom.decay_fn`), each with its
/// dot-descendants.
pub const CONFIG_NAMESPACES: [&str; 2] = ["tagma", "taskloom"];

/// The schema stream's fixed root.
pub fn schema_root() -> Uuid {
    todo!("red: the schema stream's root, derived from SCHEMA_DOMAIN and SCHEMA_STREAM")
}

/// The store's tag rules, folded from the schema stream.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SchemaState {
    /// Normalized: unique, sorted by their canonical spelling.
    pub tags: Vec<Tag>,
    /// Whether the stream has any history: a store whose schema stream has
    /// none is seeded on open.
    pub declared: bool,
}

/// Add and remove schema tags in one change, as a task's `Retag`: the adds
/// first, then the removes, so a tag named in both ends up removed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Declare {
    pub add: Vec<Tag>,
    pub remove: Vec<Tag>,
}

/// Recorded only when a declare changes the rules: the whole tag set after
/// it, normalized, so a reader folds it with no rule and no prior state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SchemaRetagged {
    pub tags: Vec<Tag>,
}

/// Why a declare was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaRejection {
    /// A declare adding and removing nothing.
    TagRequired,
    /// A tag outside [`CONFIG_NAMESPACES`]: a task's tag, not a rule.
    NotConfig { tag: String },
}

impl fmt::Display for SchemaRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for SchemaRejection {
    fn code(&self) -> &'static str {
        todo!("red: tasks-schema-tag-required, tasks-schema-not-config")
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        todo!("red: NotConfig's `tag`")
    }
}

/// What a declare against the schema `held` records: `None` when it
/// changes nothing.
pub fn declared(
    held: &SchemaState,
    declare: &Declare,
) -> Result<Option<SchemaRetagged>, SchemaRejection> {
    let _ = (held, declare);
    todo!("red: decide a Declare")
}

/// Fold `event` into `state`.
pub fn apply_schema(state: &mut SchemaState, event: &SchemaRetagged) {
    let _ = (state, event);
    todo!("red: apply SchemaRetagged")
}

#[cfg(test)]
#[path = "schema.test.rs"]
mod tests;
