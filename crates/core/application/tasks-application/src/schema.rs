//! The store's schema stream (docs/plans/self-hosted-schema.md): one per
//! store ([`tasks::schema`]), holding the store's tag rules as tagma
//! config tags ([`crate::rules`]), seeded with [`crate::SEEDED_SCHEMA`] when a
//! store is written first (and on the first write to a store with no
//! schema stream), and changed only by a person
//! ([`crate::PermitService::declare`], `permit`, `revoke`, `schema
//! import-config`). No file layer (home, work root,
//! `TASKLOOM_CONFIG_TAG_SCHEMA`, `--config-set`) is read for them: a layer
//! still setting one of the old keys is refused
//! ([`ports::Configuration::moved`]).
//!
//! A read (a listing, lint, `schema show`) never writes: a store with no
//! schema stream is read as if seeded.

use std::future::Future;

use angzarr_diagnostic::Report;
use angzarr_embedded::v1::{CommandBook, Cover};
use angzarr_embedded::{CommandBookExt, CoverExt, Handled, Kernel, Registry, Store};
use ports::{Clock, CommandIds, Configuration, Delay, Jitter, ProjectIdentity, TaskStores};
use tasks::schema::{
    Declare, SchemaAggregate, SchemaCommand, SchemaState, SCHEMA_DOMAIN, SCHEMA_STREAM,
};

use crate::rules::{Finding, StoreRules};
use crate::write::Writes;
use crate::{failed, rejected, AppError, Tag};
use kernel_runner::Runner;

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

/// The schema stream of a store: one per store, its fixed name its
/// natural key.
pub(crate) fn schema_stream() -> Cover {
    Cover::natural(SCHEMA_DOMAIN, SCHEMA_STREAM)
}

/// The schema `store` holds now, as the kernel loads its stream: no
/// history, an undeclared one.
async fn schema_now<St: Store>(store: &St, registry: &Registry) -> Result<SchemaState, AppError> {
    let (schema, _) = Kernel::new(registry)
        .load::<SchemaAggregate, _>(store, &schema_stream())
        .await
        .map_err(|e| AppError::Failed(Report::of(&e)))?;
    Ok(schema.state().state().clone())
}

/// The rules `store` holds now ([`schema_now`]): an undeclared schema's,
/// the seed's.
pub(crate) async fn rules_in<St: Store>(
    store: &St,
    registry: &Registry,
) -> Result<StoreRules, AppError> {
    Ok(StoreRules::of(&schema_now(store, registry).await?))
}

/// The rules `handled`, a command to the schema stream, left: from its
/// commit itself; a duplicate's, which recorded nothing, as `store` holds
/// them now.
async fn rules_after<St: Store>(
    store: &St,
    registry: &Registry,
    handled: &Handled,
) -> Result<StoreRules, AppError> {
    let committed = handled
        .state::<SchemaAggregate>(&schema_stream(), registry)
        .map_err(|e| AppError::Failed(Report::of(&e)))?;
    match committed {
        Some(schema) => Ok(StoreRules::of(schema.state())),
        None => rules_in(store, registry).await,
    }
}

/// `store`'s rules after seeding its schema stream, where it has no
/// history, with [`crate::SEEDED_SCHEMA`]: a stream already declared is loaded
/// and nothing is shaped or sent. The seed is sent at the head loaded (empty), so two
/// writers seeding at once land it once, the second, shaping again, finding
/// it there.
pub(crate) async fn seed_in<St, C, D>(
    runner: &mut Runner<'_, C, D>,
    store: &St,
) -> Result<StoreRules, AppError>
where
    St: Store,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    let held = schema_now(store, runner.registry).await?;
    if held.declared {
        return Ok(StoreRules::of(&held));
    }
    let seed = Declare {
        add: StoreRules::default()
            .tags()
            .iter()
            .cloned()
            .map(Tag::from)
            .collect(),
        remove: Vec::new(),
    };
    let unseeded = |schema: &SchemaAggregate| {
        let held = schema.state().clone();
        let send = (!held.declared).then(|| SchemaCommand::from(seed.clone()));
        (send, held)
    };
    let command_id = runner.clock.command_id();
    let (handled, held) = runner
        .shaped::<SchemaAggregate, _, _, _, _>(store, &schema_stream(), &command_id, unseeded)
        .await??;
    match handled {
        Some(handled) => rules_after(store, runner.registry, &handled).await,
        None => Ok(StoreRules::of(&held)),
    }
}

/// Each of `spelled` parsed as a schema tag: refused at the first that is
/// not a tag in tagma's grammar, or is blank.
fn parsed(spelled: &[String]) -> Result<Vec<Tag>, AppError> {
    crate::tags::parse_all(spelled).map_err(|r| crate::refused(&r))
}

/// `declare` sent to `store`'s schema stream, whose rules were `before`:
/// the tags after it, and whether they changed (a duplicate's, which
/// recorded nothing, as `store` holds them now).
pub(crate) async fn declare_in<St, C, D>(
    runner: &mut Runner<'_, C, D>,
    store: &St,
    declare: Declare,
    before: StoreRules,
) -> Result<Declared, AppError>
where
    St: Store,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    let book = CommandBook::command_of(schema_stream(), &SchemaCommand::from(declare));
    let command_id = runner.clock.command_id();
    let handled = runner
        .run::<SchemaAggregate, _, _>(store, book, &command_id)
        .await??;
    let after = rules_after(store, runner.registry, &handled).await?;
    Ok(Declared {
        changed: after != before,
        tags: after.export(),
    })
}

/// A person's [`Declaration`], parsed: what it adds and removes, held to
/// the schema stream's rules by what it carries alone.
pub(crate) fn declaration(declaration: Declaration) -> Result<Declare, AppError> {
    let declare = Declare {
        add: parsed(&declaration.add)?,
        remove: parsed(&declaration.remove)?,
    };
    tasks::schema::check(&declare).map_err(|r| rejected(&r))?;
    Ok(declare)
}

/// [`SchemaService::schema_export`]: the store's schema tags, seeded first
/// where it is there but has no schema stream; the seed's where there is
/// no project or no store, written nowhere.
pub(crate) async fn export<I, S, G, C, D>(
    writes: &mut Writes<'_, I, S, G, C, D>,
    pinned: Option<&str>,
) -> Result<Vec<String>, AppError>
where
    I: ProjectIdentity,
    S: TaskStores,
    G: Configuration,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    writes.projects.refuse_moved()?;
    let found = writes
        .projects
        .identity
        .for_read(pinned)
        .map_err(AppError::Identity)?;
    let Some(found) = found else {
        return Ok(StoreRules::default().export());
    };
    let opened = writes.projects.stores.open_existing(&found).await;
    if opened.map_err(failed)?.is_none() {
        return Ok(StoreRules::default().export());
    }
    Ok(writes.seeded(&found).await?.export())
}

#[cfg(test)]
#[path = "schema.test.rs"]
mod tests;
