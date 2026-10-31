//! The store's tag schema as a domain stream
//! (docs/plans/self-hosted-schema.md).
//!
//! One stream per project store, at a fixed root ([`schema_root`]) in its
//! own domain ([`SCHEMA_DOMAIN`]), so it is never listed as a task. It holds
//! the store's tag rules as tagma config tags (`tagma.*`, and taskloom's own
//! `taskloom.*`); [`Declare`] changes them, Retag-shaped, and every change
//! records [`SchemaRetagged`], carrying the whole tag set after it. Ordering
//! is the stream's own: nothing numbers across streams.
//!
//! Nothing here interprets a rule: what a tag means is tagma's. Decide
//! holds a declare only to naming a tag, and to naming config tags alone;
//! a malformed one is recorded, for `taskloom schema check` to report.
//!
//! The commands and events are generated from
//! `proto/ctxloom/tasks/v1/schema.proto`; [`SchemaAggregate`] is the
//! aggregate angzarr-embedded runs, thin handlers over [`declared`] and
//! [`apply_schema`], as [`crate::TaskAggregate`] is over `lib.rs`'s rules.

use std::fmt;

use angzarr_embedded::next::markup::{Context, Failed};
use angzarr_embedded::next::v1::Uuid;
use angzarr_embedded::next::{ids, WireError};
use angzarr_embedded_diagnostic::{Arg, Diagnostic};
use angzarr_embedded_macros::{aggregate, applies, handles_cmd};

use crate::{normalized, Tag};
pub use crate::{Declare, SchemaCommand, SchemaEvent, SchemaRetagged};

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
    ids::compute_root(SCHEMA_DOMAIN, SCHEMA_STREAM)
        .expect("the schema stream's fixed name derives a root")
}

/// Whether `namespace` is one of [`CONFIG_NAMESPACES`] or under one.
pub fn is_config_namespace(namespace: &str) -> bool {
    CONFIG_NAMESPACES.iter().any(|root| {
        namespace
            .strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    })
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

/// A declare adding and removing nothing.
impl Default for Declare {
    fn default() -> Self {
        Declare {
            add: Vec::new(),
            remove: Vec::new(),
        }
    }
}

/// Why a declare was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaRejection {
    /// A declare adding and removing nothing.
    TagRequired,
    /// A tag outside [`CONFIG_NAMESPACES`]: a task's tag, not a rule.
    NotConfig { tag: String },
}

/// Its key and args, never English (angzarr decision 14.37).
impl fmt::Display for SchemaRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for SchemaRejection {
    fn code(&self) -> &str {
        match self {
            SchemaRejection::TagRequired => "tasks-schema-tag-required",
            SchemaRejection::NotConfig { .. } => "tasks-schema-not-config",
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            SchemaRejection::TagRequired => Vec::new(),
            SchemaRejection::NotConfig { tag } => vec![("tag", tag.as_str().into())],
        }
    }
}

/// What a declare against the schema `held` records: `None` when it
/// changes nothing. Refused when it names no tag, or a tag outside
/// [`CONFIG_NAMESPACES`] (an add or a remove: neither is ever a rule).
pub fn declared(
    held: &SchemaState,
    declare: &Declare,
) -> Result<Option<SchemaRetagged>, SchemaRejection> {
    check(declare)?;
    let before = normalized(&held.tags);
    let mut after = before.clone();
    after.extend(normalized(&declare.add));
    for spelled in normalized(&declare.remove).keys() {
        after.remove(spelled);
    }
    Ok((after != before).then(|| SchemaRetagged {
        tags: after.into_values().collect(),
    }))
}

/// Whether `declare` keeps the rules decide holds it to by what it carries
/// alone: it names a tag, and every tag it names is a config tag. A caller
/// runs it first, so a declare it refuses is refused before a store is
/// opened for it.
pub fn check(declare: &Declare) -> Result<(), SchemaRejection> {
    if declare.add.is_empty() && declare.remove.is_empty() {
        return Err(SchemaRejection::TagRequired);
    }
    let outside = declare
        .add
        .iter()
        .chain(&declare.remove)
        .find(|tag| !tag.namespace.as_deref().is_some_and(is_config_namespace));
    match outside {
        Some(tag) => Err(SchemaRejection::NotConfig {
            tag: tag.to_string(),
        }),
        None => Ok(()),
    }
}

/// Fold `event` into `state`: assign what it carries.
pub fn apply_schema(state: &mut SchemaState, event: &SchemaRetagged) {
    state.tags = event.tags.clone();
    state.declared = true;
}

impl From<SchemaState> for tasks_proto::v1::SchemaState {
    fn from(value: SchemaState) -> Self {
        tasks_proto::v1::SchemaState {
            tags: value
                .tags
                .into_iter()
                .map(tasks_proto::v1::Tag::from)
                .collect(),
            declared: value.declared,
        }
    }
}

impl TryFrom<tasks_proto::v1::SchemaState> for SchemaState {
    type Error = WireError;

    fn try_from(value: tasks_proto::v1::SchemaState) -> Result<Self, WireError> {
        Ok(SchemaState {
            tags: value
                .tags
                .into_iter()
                .map(Tag::try_from)
                .collect::<Result<Vec<_>, _>>()?,
            declared: value.declared,
        })
    }
}

/// The schema aggregate: the store's rules, folded from its stream. No
/// snapshot: a declare is a person's, so the stream stays short.
#[derive(Clone, Debug, Default)]
pub struct SchemaAggregate {
    schema: SchemaState,
}

impl SchemaAggregate {
    /// The folded rules.
    pub fn state(&self) -> &SchemaState {
        &self.schema
    }
}

// The domain is SCHEMA_DOMAIN, spelled as the markup takes it, a literal
// (`schema.test.rs` holds them to each other).
#[aggregate(domain = "schema")]
impl SchemaAggregate {
    /// Add and remove schema tags; `None` when the rules do not change.
    #[handles_cmd(Declare)]
    fn declare(
        &self,
        cmd: Declare,
        ctx: &Context,
    ) -> Result<Option<SchemaRetagged>, Failed<SchemaRejection>> {
        let _ = ctx;
        Ok(declared(&self.schema, &cmd)?)
    }

    #[applies(SchemaRetagged)]
    fn schema_retagged(&mut self, event: SchemaRetagged) {
        apply_schema(&mut self.schema, &event);
    }
}

/// One value of every rejection, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(SchemaRejection::TagRequired),
        Box::new(SchemaRejection::NotConfig {
            tag: "area:cli".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "schema.test.rs"]
mod tests;
