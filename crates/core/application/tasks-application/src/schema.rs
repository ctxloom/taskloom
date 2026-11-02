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
use crate::{failed, rejected, AppError, Project, Service, Tag};
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
fn schema_stream() -> Cover {
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

impl<I, S, N, C, D, G: Configuration> Service<I, S, N, C, D, G> {
    /// Refused where a configuration layer still sets a key that moved
    /// into the store ([`ports::Configuration::moved`]).
    pub(crate) fn refuse_moved(&mut self) -> Result<(), AppError> {
        self.configuration.moved().map_err(AppError::Configuration)
    }
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    S: TaskStores,
{
    /// The rules of `project`'s store, read and never written: the seed
    /// where it has no store or no schema stream yet.
    pub(crate) async fn rules_of(&mut self, project: &Project) -> Result<StoreRules, AppError> {
        let Some(store) = self.stores.open_existing(project).await.map_err(failed)? else {
            return Ok(StoreRules::default());
        };
        rules_in(store, &self.registry).await
    }
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    I: ProjectIdentity,
    S: TaskStores,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    /// `project`'s store's rules after seeding its schema stream, where it
    /// has no history ([`seed_in`]).
    pub(crate) async fn seeded(&mut self, project: &Project) -> Result<StoreRules, AppError> {
        let store = self.stores.open(project).await.map_err(failed)?;
        let mut runner = Runner {
            registry: &self.registry,
            clock: &mut self.clock,
            delay: &mut self.delay,
            retry: self.retry,
            dead_letters: &mut self.dead_letters,
        };
        seed_in(&mut runner, store).await
    }

    /// `declare` on `project`'s store's schema stream, seeded first: the
    /// tags after it, and whether they changed.
    pub(crate) async fn declaring(
        &mut self,
        project: &Project,
        declare: Declare,
    ) -> Result<Declared, AppError> {
        tasks::schema::check(&declare).map_err(|r| rejected(&r))?;
        let before = self.seeded(project).await?;
        let store = self.stores.open(project).await.map_err(failed)?;
        let book = CommandBook::command_of(schema_stream(), &SchemaCommand::from(declare));
        let command_id = self.clock.command_id();
        let mut runner = Runner {
            registry: &self.registry,
            clock: &mut self.clock,
            delay: &mut self.delay,
            retry: self.retry,
            dead_letters: &mut self.dead_letters,
        };
        let handled = runner
            .run::<SchemaAggregate, _, _>(store, book, &command_id)
            .await??;
        let after = rules_after(store, &self.registry, &handled).await?;
        Ok(Declared {
            changed: after != before,
            tags: after.export(),
        })
    }

    /// A person's [`Declaration`], parsed, on the work root's project's
    /// store.
    pub(crate) async fn declared(
        &mut self,
        declaration: Declaration,
    ) -> Result<Declared, AppError> {
        let declare = Declare {
            add: parsed(&declaration.add)?,
            remove: parsed(&declaration.remove)?,
        };
        tasks::schema::check(&declare).map_err(|r| rejected(&r))?;
        let project = self.identity.for_write(None).map_err(AppError::Identity)?;
        self.declaring(&project, declare).await
    }
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    I: ProjectIdentity,
    S: TaskStores,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
    G: Configuration,
{
    /// The rules of the store `pinned` (or the work root) names, refused
    /// first where a layer still sets a moved key; the seed where there is
    /// no project or no store.
    pub(crate) async fn rules_for(&mut self, pinned: Option<&str>) -> Result<StoreRules, AppError> {
        self.refuse_moved()?;
        match self.identity.for_read(pinned).map_err(AppError::Identity)? {
            Some(project) => self.rules_of(&project).await,
            None => Ok(StoreRules::default()),
        }
    }
}

impl<I, S, N, C, D, G> SchemaService for Service<I, S, N, C, D, G>
where
    I: ProjectIdentity + Send,
    S: TaskStores + Send,
    N: Send,
    C: Clock + CommandIds + Send,
    D: Delay + Jitter + Send,
    G: Configuration + Send,
{
    async fn schema_export(&mut self, project: Option<String>) -> Result<Vec<String>, AppError> {
        self.refuse_moved()?;
        let Some(found) = self
            .identity
            .for_read(project.as_deref())
            .map_err(AppError::Identity)?
        else {
            return Ok(StoreRules::default().export());
        };
        if self
            .stores
            .open_existing(&found)
            .await
            .map_err(failed)?
            .is_none()
        {
            return Ok(StoreRules::default().export());
        }
        Ok(self.seeded(&found).await?.export())
    }

    async fn schema_check(&mut self, project: Option<String>) -> Result<Vec<Finding>, AppError> {
        Ok(self.rules_for(project.as_deref()).await?.check())
    }
}

#[cfg(test)]
#[path = "schema.test.rs"]
mod tests;
