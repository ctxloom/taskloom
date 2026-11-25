//! The store's schema stream (docs/plans/self-hosted-schema.md): one per
//! store ([`tasks::schema`]), holding the store's tag rules as tagma
//! config tags ([`crate::rules`]), seeded with [`crate::SEEDED_SCHEMA`] when a
//! store is written first (and on the first write to a store with no
//! schema stream, or with its permits from before every store was an
//! allowlist: [`seed_in`]), and changed only by a person
//! ([`crate::PermitService::declare`], `permit`, `revoke`, `schema
//! import-config`). No file layer (home, work root,
//! `TASKLOOM_CONFIG_TAG_SCHEMA`, `--config-set`) is read for them: a layer
//! still setting one of the old keys is refused
//! ([`ports::Configuration::moved`]).
//!
//! A read (a listing, lint, `schema show`) never writes: a store with no
//! schema stream, or with its old permits, is read as if seeded.

use std::future::Future;

use angzarr_embedded::v1::{CommandBook, Cover};
use angzarr_embedded::{CommandBookExt, CoverExt, Handled, Kernel, Registry, Store};
use coded_diagnostic::Report;
use ports::{Clock, CommandIds, Configuration, Delay, Jitter, ProjectIdentity, TaskStores};
use tasks::schema::{
    Declare, SchemaAggregate, SchemaCommand, SchemaRejection, SchemaState, SCHEMA_DOMAIN,
    SCHEMA_STREAM,
};

use crate::rules::{Finding, StoreRules, Template};
use crate::write::Writes;
use crate::{failed, rejected, AppError, Tag};
use kernel_runner::Runner;
use tag_rules::template::{self, Conflict};

/// Schema tags to add and remove, in tagma's grammar: `taskloom schema
/// declare --add/--remove`. Each must be in `tagma.*` or `taskloom.*`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Declaration {
    pub add: Vec<String>,
    pub remove: Vec<String>,
    /// Check everything and answer what it would do, writing nothing.
    pub dry_run: bool,
}

/// `taskloom manage schema init`: a named template, its areas
/// customized, to declare on a store, adding only what the store lacks.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SchemaInit {
    pub template: Template,
    /// The `area:` keys the project has, when it has said: `area:` is then
    /// CLOSED to exactly these; none given, the template's own (open).
    pub areas: Vec<String>,
    /// Check everything and answer what it would do, writing nothing.
    pub dry_run: bool,
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

/// `store`'s rules after seeding its schema stream: where it has no
/// history, with [`crate::SEEDED_SCHEMA`]; where it holds permits from
/// before every store was an allowlist, with the permits of the ctxloom
/// template (pinned: what stores were then seeded with), its old ones made
/// presences ([`seeding`]). A stream that holds what it is read
/// as is loaded and nothing is shaped or sent. The seeding is sent at the
/// head loaded, so two writers seeding at once land it once, the second,
/// shaping again, finding it there.
pub(crate) async fn seed_in<St, C, D>(
    runner: &mut Runner<'_, C, D>,
    store: &St,
    base: Template,
) -> Result<StoreRules, AppError>
where
    St: Store,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    let held = schema_now(store, runner.registry).await?;
    if seeding(&held, base).is_none() {
        return Ok(StoreRules::of(&held));
    }
    let unseeded = |schema: &SchemaAggregate| {
        let held = schema.state().clone();
        let send = seeding(&held, base).map(SchemaCommand::from);
        (send, held)
    };
    let command_id = runner.clock.command_id();
    let (handled, held) = runner
        .shaped::<SchemaAggregate, SchemaRejection, _, _, _>(
            store,
            &schema_stream(),
            &command_id,
            unseeded,
        )
        .await??;
    match handled {
        Some(handled) => rules_after(store, runner.registry, &handled).await,
        None => Ok(StoreRules::of(&held)),
    }
}

/// The declare that leaves `held` holding the rules it is read as
/// ([`StoreRules::of`]): the seed, on a stream with no history; on one
/// holding its permits from before every store was an allowlist, those
/// permits as presences and the seed's beside them; `None` where it holds
/// them already.
fn seeding(held: &SchemaState, base: Template) -> Option<Declare> {
    let read = match held.declared {
        true => StoreRules::of(held),
        false => StoreRules::of_template(base),
    };
    let holds = StoreRules::held(held);
    let missing = |from: &StoreRules, of: &StoreRules| -> Vec<Tag> {
        from.tags()
            .iter()
            .filter(|tag| !of.holds(tag))
            .cloned()
            .map(Tag::from)
            .collect()
    };
    let declare = Declare {
        add: missing(&read, &holds),
        remove: missing(&holds, &read),
    };
    (!declare.add.is_empty() || !declare.remove.is_empty()).then_some(declare)
}

/// The rules `schema init` is held against: the store's as read, where its
/// stream has history; none where it has none (a store with no history is
/// not read as the default seed, because init declares its set itself).
fn init_held(state: &SchemaState) -> StoreRules {
    match state.declared {
        true => StoreRules::of(state),
        false => StoreRules::of_tags(std::iter::empty()),
    }
}

/// `conflict` as the refusal of `template`.
pub(crate) fn conflicting(template: Template, conflict: Conflict) -> AppError {
    AppError::SchemaInitConflict {
        template: template.name().to_string(),
        held: conflict.held.join("; "),
    }
}

/// What `wanted` (a template's tags) would leave on `store`'s schema
/// stream, sent nowhere: the rules after it, and whether they change.
pub(crate) async fn init_preview<St: Store>(
    store: Option<&St>,
    registry: &Registry,
    template: Template,
    wanted: &[tagma_core::Tag],
) -> Result<Declared, AppError> {
    let held = match store {
        Some(store) => init_held(&schema_now(store, registry).await?),
        None => init_held(&SchemaState::default()),
    };
    let missing = template::missing(&held, wanted).map_err(|c| conflicting(template, c))?;
    let changed = !missing.is_empty();
    let after = StoreRules::of_tags(held.tags().iter().cloned().chain(missing));
    Ok(Declared {
        changed,
        tags: after.export(),
    })
}

/// `wanted` (a template's tags) declared on `store`'s schema stream, only
/// the tags it lacks: its rules after, and whether they changed. A store
/// from before every store was an allowlist is migrated first ([`seed_in`]);
/// one with no history is not seeded, since this declares its set. Sent at
/// the head shaped from, so two writers land it once.
pub(crate) async fn init_in<St, C, D>(
    runner: &mut Runner<'_, C, D>,
    store: &St,
    template: Template,
    wanted: &[tagma_core::Tag],
) -> Result<Declared, AppError>
where
    St: Store,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    if schema_now(store, runner.registry).await?.declared {
        seed_in(runner, store, Template::default()).await?;
    }
    let missing = |schema: &SchemaAggregate| {
        let held = init_held(schema.state());
        match template::missing(&held, wanted) {
            Ok(missing) => {
                let add: Vec<Tag> = missing.into_iter().map(Tag::from).collect();
                let declare = Declare {
                    add,
                    remove: Vec::new(),
                };
                let send = (!declare.add.is_empty()).then(|| SchemaCommand::from(declare));
                (send, Ok(held))
            }
            Err(conflict) => (None, Err(conflict)),
        }
    };
    let command_id = runner.clock.command_id();
    let (handled, held) = runner
        .shaped::<SchemaAggregate, SchemaRejection, _, _, _>(
            store,
            &schema_stream(),
            &command_id,
            missing,
        )
        .await??;
    let held = held.map_err(|c| conflicting(template, c))?;
    match handled {
        Some(handled) => {
            let after = rules_after(store, runner.registry, &handled).await?;
            Ok(Declared {
                changed: after != held,
                tags: after.export(),
            })
        }
        None => Ok(Declared {
            changed: false,
            tags: held.export(),
        }),
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
        .run::<SchemaAggregate, SchemaRejection, _>(store, book, &command_id)
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
