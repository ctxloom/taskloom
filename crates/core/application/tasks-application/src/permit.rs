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

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;

use ports::{
    Clock, CommandIds, Configuration, ConfigurationWriter, NamespacePermit, ProjectIdentity,
    TagPermits, TaskStores,
};

use crate::schema::FACET_NAMESPACE;
use crate::{tags, AppError, Service, Tag};

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
    async fn permit(&mut self, change: PermitChange) -> Result<Permitted, AppError> {
        let permit = Permit::parse(&change.permit)?;
        let held = self.tag_permits()?;
        let declared = held.is_some();
        let mut permits = held.unwrap_or_default();
        let widened = permit.grant(&mut permits);
        // The first permit of a project declaring none starts its permits,
        // whatever it permits.
        let changed = widened || !declared;
        if changed {
            self.configuration
                .write_tag_permits(&permits)
                .map_err(AppError::Configuration)?;
        }
        Ok(Permitted {
            permit: permit.to_string(),
            changed,
        })
    }

    async fn revoke(&mut self, change: PermitChange) -> Result<Permitted, AppError> {
        let permit = Permit::parse(&change.permit)?;
        let not_declared = || AppError::PermitNotDeclared {
            permit: permit.to_string(),
        };
        let mut permits = self.tag_permits()?.ok_or_else(not_declared)?;
        if !permit.withdraw(&mut permits) {
            return Err(not_declared());
        }
        let harp_ids = self.carrying(&permit).await?;
        if !harp_ids.is_empty() {
            return Err(AppError::PermitInUse {
                permit: permit.to_string(),
                harp_ids,
            });
        }
        self.configuration
            .write_tag_permits(&permits)
            .map_err(AppError::Configuration)?;
        Ok(Permitted {
            permit: permit.to_string(),
            changed: true,
        })
    }
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    I: ProjectIdentity,
    S: TaskStores,
    C: Clock,
    G: Configuration,
{
    /// The harp ids, sorted, of the work root's project's tasks (whatever
    /// their status) carrying a tag `permit` permits: none where there is
    /// no project or no store yet.
    async fn carrying(&mut self, permit: &Permit) -> Result<Vec<String>, AppError> {
        let Some(project) = self.identity.for_read(None).map_err(AppError::Identity)? else {
            return Ok(Vec::new());
        };
        let rows = self.rows_of(&project).await?.unwrap_or_default();
        let mut harp_ids: Vec<String> = rows
            .into_iter()
            .filter(|row| row.task.tags.iter().any(|tag| permit.covers(tag)))
            .map(|row| row.task.harp_id)
            .collect();
        harp_ids.sort();
        Ok(harp_ids)
    }
}

/// A permit, parsed: what it is of.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Permit {
    /// `area:`: the namespace, any key.
    Namespace(String),
    /// `area:cli`: one key of a namespace.
    Key { namespace: String, key: String },
    /// `urgent`: a flat tag.
    Flat(String),
}

impl Permit {
    /// `spelled`, trimmed, as a permit: blank refused as given blank, and
    /// anything not spelled as one ([`AppError::PermitMalformed`]).
    fn parse(spelled: &str) -> Result<Permit, AppError> {
        let trimmed = spelled.trim();
        if trimmed.is_empty() {
            return Err(AppError::Blank { field: "permit" });
        }
        let malformed = || AppError::PermitMalformed {
            permit: trimmed.to_string(),
        };
        // A namespace's permit is its tag with the key left out, which
        // tagma does not parse: it is parsed with a key put in, and that
        // key must come back as put, so nothing else was given.
        let (spelling, namespace_only) = match trimmed.strip_suffix(':') {
            Some(namespace) => (format!("{namespace}:{PLACEHOLDER}"), true),
            None => (trimmed.to_string(), false),
        };
        let tag = match tags::parse(&spelling) {
            Some(Ok(tag)) => tag,
            _ => return Err(malformed()),
        };
        if tag.value.is_some() {
            return Err(malformed());
        }
        let bare = tag.namespace.is_none();
        if bare && tags::OPERATOR_WORDS.contains(&tag.key.to_ascii_lowercase().as_str()) {
            return Err(malformed());
        }
        if let Some(namespace) = &tag.namespace {
            let reserved = namespace
                .strip_prefix(FACET_NAMESPACE)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'));
            if reserved || namespace.is_empty() {
                return Err(malformed());
            }
        }
        match (tag.namespace, namespace_only) {
            (Some(namespace), true) if tag.key == PLACEHOLDER => Ok(Permit::Namespace(namespace)),
            (Some(namespace), false) => Ok(Permit::Key {
                namespace,
                key: tag.key,
            }),
            (None, false) => Ok(Permit::Flat(tag.key)),
            _ => Err(malformed()),
        }
    }

    /// Grant this permit in `permits`: whether that widened them. A key of
    /// a namespace not yet permitted closes it to that key; the namespace
    /// itself opens it, its keys dropped.
    fn grant(&self, permits: &mut TagPermits) -> bool {
        match self {
            Permit::Namespace(namespace) => {
                let held = permits
                    .namespaces
                    .insert(namespace.clone(), NamespacePermit::Open);
                held != Some(NamespacePermit::Open)
            }
            Permit::Key { namespace, key } => {
                match permits
                    .namespaces
                    .entry(namespace.clone())
                    .or_insert_with(|| NamespacePermit::Closed(BTreeSet::new()))
                {
                    NamespacePermit::Open => false,
                    NamespacePermit::Closed(keys) => keys.insert(key.clone()),
                }
            }
            Permit::Flat(key) => permits.flat.insert(key.clone()),
        }
    }

    /// Withdraw this permit from `permits`: whether it was among them. A
    /// namespace's goes with its keys; a closed namespace's last key takes
    /// the namespace with it. A key of an open namespace is not among
    /// them: its namespace's permit admits it.
    fn withdraw(&self, permits: &mut TagPermits) -> bool {
        match self {
            Permit::Namespace(namespace) => permits.namespaces.remove(namespace).is_some(),
            Permit::Key { namespace, key } => {
                let Some(NamespacePermit::Closed(keys)) = permits.namespaces.get_mut(namespace)
                else {
                    return false;
                };
                let removed = keys.remove(key);
                if keys.is_empty() {
                    permits.namespaces.remove(namespace);
                }
                removed
            }
            Permit::Flat(key) => permits.flat.remove(key),
        }
    }

    /// Whether `tag` is one this permit permits, whatever its value.
    fn covers(&self, tag: &Tag) -> bool {
        match self {
            Permit::Namespace(namespace) => tag.namespace.as_ref() == Some(namespace),
            Permit::Key { namespace, key } => {
                tag.namespace.as_ref() == Some(namespace) && tag.key == *key
            }
            Permit::Flat(key) => tag.namespace.is_none() && tag.key == *key,
        }
    }
}

/// The key a namespace's permit is parsed with.
const PLACEHOLDER: &str = "x";

/// The canonical spelling, as `taskloom permit` takes it.
impl fmt::Display for Permit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let spelled = match self {
            Permit::Namespace(namespace) => {
                let tag = Tag {
                    namespace: Some(namespace.clone()),
                    key: PLACEHOLDER.to_string(),
                    value: None,
                };
                let spelled = tag.to_string();
                let cut = spelled.len() - PLACEHOLDER.len();
                spelled[..cut].to_string()
            }
            Permit::Key { namespace, key } => Tag {
                namespace: Some(namespace.clone()),
                key: key.clone(),
                value: None,
            }
            .to_string(),
            Permit::Flat(key) => Tag {
                namespace: None,
                key: key.clone(),
                value: None,
            }
            .to_string(),
        };
        f.write_str(&spelled)
    }
}

#[cfg(test)]
#[path = "permit.test.rs"]
mod tests;
