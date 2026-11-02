//! How a use case reaches its project: resolving it ([`ProjectIdentity`]),
//! opening its store ([`TaskStores`]), and reading the store's rules,
//! refused first where a configuration layer still sets a key that moved
//! into the store ([`Configuration::moved`]). Every use case but the
//! watch and the import reaches its project this way ([`Projects`]); a
//! write goes on to run its commands through the kernel
//! ([`crate::write::Writes`]).

use angzarr_embedded::Registry;
use ports::{Configuration, Project, ProjectIdentity, TaskStores};
use tasks::TaskRejection;

use crate::rules::StoreRules;
use crate::{failed, rejected, AppError};

/// The ports a project is reached through, and the registry its store's
/// streams are read under: borrowed from the service for one call.
pub(crate) struct Projects<'s, I, S, G> {
    pub identity: &'s mut I,
    pub stores: &'s mut S,
    pub configuration: &'s mut G,
    pub registry: &'s Registry,
}

impl<I, S, G: Configuration> Projects<'_, I, S, G> {
    /// Refused where a configuration layer still sets a key that moved
    /// into the store ([`ports::Configuration::moved`]).
    pub(crate) fn refuse_moved(&mut self) -> Result<(), AppError> {
        self.configuration.moved().map_err(AppError::Configuration)
    }
}

impl<I, S: TaskStores, G> Projects<'_, I, S, G> {
    /// The rules of `project`'s store, read and never written: the seed
    /// where it has no store or no schema stream yet.
    pub(crate) async fn rules_of(&mut self, project: &Project) -> Result<StoreRules, AppError> {
        let Some(store) = self.stores.open_existing(project).await.map_err(failed)? else {
            return Ok(StoreRules::default());
        };
        crate::schema::rules_in(store, self.registry).await
    }
}

impl<I: ProjectIdentity, S: TaskStores, G> Projects<'_, I, S, G> {
    /// The project of a change to an existing task, resolved as a read
    /// resolves it: where there is none, or no store yet, there is no task
    /// to change, and nothing is minted or created to say so.
    pub(crate) async fn existing(&mut self, pinned: Option<&str>) -> Result<Project, AppError> {
        let project = self
            .identity
            .for_read(pinned)
            .map_err(AppError::Identity)?
            .ok_or(AppError::NoProject)?;
        match self.stores.open_existing(&project).await.map_err(failed)? {
            Some(_) => Ok(project),
            None => Err(rejected(&TaskRejection::NoTask)),
        }
    }
}

impl<I: ProjectIdentity, S: TaskStores, G: Configuration> Projects<'_, I, S, G> {
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
