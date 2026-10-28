//! Tag permits: a project's closed tag vocabulary, declared in its work
//! root's `.taskloom/config.yaml` (`tag_permits`) and changed only by
//! [`PermitService`], which a person runs from the command line.
//!
//! A permit is spelled as the tag it permits, without a value:
//!
//! - `area:` permits the namespace `area`, OPEN: any key;
//! - `area:cli` permits the key `cli` of the namespace `area`, which is
//!   then CLOSED: only its permitted keys;
//! - `urgent` permits the flat tag `urgent` (no namespace).
//!
//! Where a project declares permits, a write adding a tag they do not
//! permit is refused ([`crate::TagRefusal::NotPermitted`]), naming the tag
//! and the permit that would admit it; a tag the schema declares a target
//! for (the built-in `triage:*` among them) is permitted without one.
//! Where it declares none, every tag the schema admits is permitted.
//!
//! RED: this is the proposed interface, its behaviour stubbed: a permit
//! or revoke changes nothing and says so. See the tests beside it.

use std::future::Future;

use ports::{
    Clock, CommandIds, Configuration, ConfigurationWriter, ProjectIdentity, TagPermits, TaskStores,
};

use crate::{AppError, Service};

/// A permit to grant or revoke, spelled as above. It is the work root's
/// project's: the config file written is the work root's, as the one the
/// tag schema is read from, so there is no `--project`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PermitChange {
    pub permit: String,
}

/// What a permit or revoke did: the permit, spelled canonically, and
/// whether the config file changed (a permit already granted changes
/// nothing).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Permitted {
    pub permit: String,
    pub changed: bool,
}

/// What a project permits, as [`crate::TaskService::permits`] reads it
/// for `taskloom permits` and the `taskloom://tag-permits` resource.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PermitList {
    /// The work root's `tag_permits`; `None` where it declares none, and
    /// then every tag the schema admits is permitted.
    pub permits: Option<TagPermits>,
    /// Every target the resolved tag schema declares (any facet), sorted:
    /// permitted as a key without a permit, whatever the permits say of
    /// its namespace.
    pub schema_targets: Vec<String>,
}

/// Granting and revoking tag permits. Apart from [`crate::TaskService`]
/// so that a driving adapter that should not offer it (the MCP server:
/// an agent permitting its own inventions is what permits exist to stop)
/// is never handed it.
pub trait PermitService {
    /// Permit `change.permit`, persisted to the work root's config file.
    /// Permitting a key of a namespace not yet permitted closes it to that
    /// key; permitting a namespace (`area:`) opens it, its keys dropped.
    /// Where the project declares no permits yet, this is where it starts
    /// to: every tag not permitted is refused from then on.
    fn permit(
        &mut self,
        change: PermitChange,
    ) -> impl Future<Output = Result<Permitted, AppError>> + Send;

    /// Revoke `change.permit`, persisted to the work root's config file;
    /// refused, listing them, while any of the project's tasks (whatever
    /// its status) carries a tag it permits, and refused when it is not
    /// among the project's permits.
    fn revoke(
        &mut self,
        change: PermitChange,
    ) -> impl Future<Output = Result<Permitted, AppError>> + Send;
}

impl<I, S, N, C, D, G> PermitService for Service<I, S, N, C, D, G>
where
    I: ProjectIdentity + Send,
    S: TaskStores + Send,
    N: Send,
    C: Clock + CommandIds + Send,
    D: Send,
    G: Configuration + ConfigurationWriter + Send,
{
    // RED stub: changes nothing.
    async fn permit(&mut self, change: PermitChange) -> Result<Permitted, AppError> {
        Ok(Permitted {
            permit: change.permit,
            changed: false,
        })
    }

    // RED stub: changes nothing.
    async fn revoke(&mut self, change: PermitChange) -> Result<Permitted, AppError> {
        Ok(Permitted {
            permit: change.permit,
            changed: false,
        })
    }
}

#[cfg(test)]
#[path = "permit.test.rs"]
mod tests;
