//! RED STUB (red/self-hosted-schema, for the owner's review): the store's
//! own tag rules (docs/plans/self-hosted-schema.md).
//!
//! The rules are tagma config tags on the store's schema stream
//! ([`tasks::schema`]), seeded with [`SEEDED_SCHEMA`] when a store is made
//! (and on first open of a store with no schema stream), changed only by a
//! person ([`crate::PermitService::declare`], `permit`, `revoke`), and
//! interpreted ONLY by tagma: every write is judged by
//! `tagma_core::Rules::validate` (refused on an `Introduced` finding,
//! warned on an `Existing` one, [`crate::Written::warnings`]), and
//! `taskloom schema check` is `tagma_core::Rules::check`. No file layer
//! (home, work root, `TASKLOOM_CONFIG_TAG_SCHEMA`, `--config-set`) is read
//! for them.
//!
//! taskloom keeps only what is its own: storing and seeding the tags, the
//! `taskloom.*` namespace (`taskloom.priority_fn`, `taskloom.decay_fn`:
//! ranking, not constraint, so never tagma's), the `semver` comparator it
//! registers with tagma (`Rules::register_type`), and `admit`'s
//! reserved-namespace and operator-word checks, which tagma does not make
//! (its `Tag::parse` takes `and` as an ordinary key).
//!
//! Replaced and deleted by the implementation (the owner's list):
//! `schema::{TagSchema, Range, SchemaError, Why, FACETS, DEFAULT_TAG_SCHEMA}`,
//! `tags::{admit's value checks, permitted, check_scalars}` (`collapse` only if tagma exposes its §8 scalar collapse),
//! `permit::Permit` and its grant/withdraw, `lint::ONE_KEY`,
//! `ports::{Configuration::tag_schema, tag_permits, ConfigurationWriter,
//! TagPermits, NamespacePermit}`, and tasks-config's `permits`/`edit`
//! modules (the `tag_permits` and `tag_schema` keys).

use std::future::Future;

pub use tagma_core::{Finding, Origin};

use crate::AppError;

/// The namespace taskloom's own schema tags are in: ranking formulas,
/// which tagma's `check` would otherwise call an unknown facet. A task may
/// carry no tag in it, as none in `tagma`.
pub const TASKLOOM_NAMESPACE: &str = "taskloom";

/// What a store's schema stream is seeded with when the store is made: the
/// `triage:*` baseline that was `DEFAULT_TAG_SCHEMA`, in tagma's facets
/// (one tag per enum value, `lo..hi` ranges), the formulas in
/// `taskloom.*`, and one `area` and one `repo` a task
/// (`tagma.keys-per-item`, which was lint's `ONE_KEY`). No permit: the
/// world is open (tagma's implicit `"*:*"=true`) until a first permit.
pub const SEEDED_SCHEMA: [&str; 33] = [
    r#"tagma.arity:"triage:level"=scalar"#,
    r#"tagma.arity:"triage:verdict"=scalar"#,
    r#"tagma.arity:"triage:audited"=scalar"#,
    r#"tagma.arity:"triage:kind"=scalar"#,
    r#"tagma.arity:"triage:effort"=scalar"#,
    r#"tagma.arity:"triage:blocks-release"=scalar"#,
    r#"tagma.type:"triage:blocks-release"=semver"#,
    r#"tagma.enum:"triage:kind"=defect"#,
    r#"tagma.enum:"triage:kind"=capability"#,
    r#"tagma.enum:"triage:kind"=chore"#,
    r#"tagma.enum:"triage:exposed"=wire"#,
    r#"tagma.enum:"triage:exposed"=cli"#,
    r#"tagma.enum:"triage:exposed"=config"#,
    r#"tagma.enum:"triage:exposed"=on-disk"#,
    r#"tagma.enum:"triage:exposed"=api"#,
    r#"tagma.enum:"triage:verdict"=holds"#,
    r#"tagma.enum:"triage:verdict"=phantom"#,
    r#"tagma.enum:"triage:verdict"=obsolete"#,
    r#"tagma.enum:"triage:verdict"=partial"#,
    r#"tagma.enum:"triage:verdict"=unclear"#,
    r#"tagma.enum:"triage:blind-gate"=unit"#,
    r#"tagma.enum:"triage:blind-gate"=integration"#,
    r#"tagma.enum:"triage:blind-gate"=acceptance"#,
    r#"tagma.enum:"triage:blind-gate"=mutation"#,
    r#"tagma.enum:"triage:blind-gate"=lint"#,
    r#"tagma.enum:"triage:blind-gate"=ci"#,
    r#"tagma.range:"triage:level"=1..5"#,
    r#"tagma.range:"triage:audited"=20250101..20991231"#,
    r#"tagma.range:"triage:effort"=0..5"#,
    r#"tagma.keys-per-item:area=1"#,
    r#"tagma.keys-per-item:repo=1"#,
    r#"taskloom.decay_fn:"triage:kind"="{{triage:exposed=*}} > 0 ? 1 + {{age_days}}/({{age_days}}+90) : {{triage:blind-gate=*}} > 0 ? 1 + {{age_days}}/({{age_days}}+30) : {{triage:kind=capability}} > 0 ? 0.4 + 0.6 * 0.5 ** ({{age_days}}/120) : {{triage:kind=chore}} > 0 ? 0.5 + 0.5 * 0.5 ** ({{age_days}}/180) : 1""#,
    r#"taskloom.priority_fn:"triage:kind"="({{triage:level}} > 0 ? 2 ** (3 - {{triage:level}}) : 0.1) * {{age_factor}} * (1 + {{triage:blocks-release=*}}) / (1 + {{triage:effort}}/2)""#,
];

/// Schema tags to add and remove, in tagma's grammar: `taskloom schema
/// declare --add/--remove`. Each must be in `tagma.*` or `taskloom.*`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Declaration {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// What a declare (or a permit, or a revoke) left on the schema stream:
/// its tags after, in tagma syntax, sorted, and whether they changed.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Declared {
    pub tags: Vec<String>,
    pub changed: bool,
}

/// Reading the store's rules: an agent may (`taskloom schema
/// show/export/check`). Changing them is [`crate::PermitService`]'s, a
/// person's.
pub trait SchemaService {
    /// The store's schema tags in tagma syntax, one a line, sorted: the
    /// work root's project's store, seeded first where it has no schema
    /// stream yet (a store made before this change).
    fn schema_export(
        &mut self,
        project: Option<String>,
    ) -> impl Future<Output = Result<Vec<String>, AppError>> + Send;

    /// tagma's `check` over the store's schema tags, every finding
    /// `Existing`.
    fn schema_check(
        &mut self,
        project: Option<String>,
    ) -> impl Future<Output = Result<Vec<Finding>, AppError>> + Send;
}

impl crate::Written {
    /// tagma's `Existing` findings on the task written: what was wrong
    /// before the write and still is. A write is never refused for these
    /// (an `Introduced` finding refuses it, `tasks-application-tag-rules`),
    /// and the edge prints them on stderr, or in the JSON document.
    pub fn warnings(&self) -> &[Finding] {
        todo!("red: a write's Existing findings")
    }
}

impl<I, S, N, C, D, G> SchemaService for crate::Service<I, S, N, C, D, G>
where
    I: Send,
    S: Send,
    N: Send,
    C: Send,
    D: Send,
    G: Send,
{
    async fn schema_export(&mut self, project: Option<String>) -> Result<Vec<String>, AppError> {
        let _ = project;
        todo!("red: the store's schema tags, seeding a store with none")
    }

    async fn schema_check(&mut self, project: Option<String>) -> Result<Vec<Finding>, AppError> {
        let _ = project;
        todo!("red: tagma's check over the store's schema")
    }
}

#[cfg(test)]
#[path = "store_schema.test.rs"]
mod tests;
