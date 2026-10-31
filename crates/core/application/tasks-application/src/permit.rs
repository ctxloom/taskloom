//! Tag permits: the store's closed tag vocabulary, declared on its schema
//! stream as tagma's `tagma.permit` tags and changed only by
//! [`PermitService`], which a person runs from the command line.
//!
//! A permit is spelled as the tag it permits, without a value, and is the
//! tagma pattern it declares (`tagma.permit:<pattern>=true`):
//!
//! - `area:` permits the namespace `area`, any key: `"area:*"`;
//! - `area:cli` permits the key `cli` of the namespace `area`: `"area:cli"`;
//! - `urgent` permits the flat tag `urgent` (no namespace): `urgent`.
//!
//! The first permit closes the store's world (`tagma.permit:"*:*"=false`)
//! and keeps admitted every target the store's rules constrain (a permit
//! for each), so a declared `triage:*` target stays writable. From then on
//! a write adding a tag no permit admits is refused, by tagma's
//! `not-permitted`. Where no permit is declared, tagma's implicit
//! `"*:*"=true` admits every tag.

use std::fmt;
use std::future::Future;

use ports::{Clock, CommandIds, Configuration, ConfigurationWriter, ProjectIdentity, TaskStores};
use tasks::schema::Declare;

use crate::store_schema::{closing, permit_tag, Declared, StoreRules};
use crate::{tags, AppError, Declaration, Service, Tag};

/// A permit to grant or revoke, spelled as above. It is the work root's
/// project's: the store written is the work root's, so there is no
/// `--project`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PermitChange {
    pub permit: String,
}

/// What a permit or revoke did: the permit, spelled canonically, and
/// whether the store's rules changed (a permit already granted changes
/// nothing).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Permitted {
    pub permit: String,
    pub changed: bool,
}

/// What a store permits, as [`crate::TaskService::permits`] reads it for
/// `taskloom permits`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PermitList {
    /// Whether the world is closed (`tagma.permit:"*:*"=false`): only what
    /// a pattern admits is written.
    pub closed: bool,
    /// The patterns permitted (`tagma.permit:<pattern>=true`), sorted.
    pub patterns: Vec<String>,
    /// The exact targets the store's rules constrain, sorted.
    pub schema_targets: Vec<String>,
}

/// Granting and revoking tag permits, and every other change to the
/// store's rules. Apart from [`crate::TaskService`] so that a driving
/// adapter that should not offer it (one an agent drives: an agent
/// permitting its own inventions is what permits exist to stop) is never
/// handed it.
pub trait PermitService {
    /// Permit `change.permit`, declared on the work root's project's
    /// store. Where the store declares no permits yet, this is where it
    /// starts to: the world is closed, every target the rules constrain
    /// permitted, and every tag not permitted is refused from then on.
    fn permit(
        &mut self,
        change: PermitChange,
    ) -> impl Future<Output = Result<Permitted, AppError>> + Send;

    /// Revoke `change.permit`; refused, listing them, while any of the
    /// project's tasks (whatever its status) carries a tag it admits, and
    /// refused when it is not among the store's permits.
    fn revoke(
        &mut self,
        change: PermitChange,
    ) -> impl Future<Output = Result<Permitted, AppError>> + Send;

    /// Add and remove the store's schema tags (`taskloom schema declare`),
    /// recorded on its schema stream. A person's: the loadout tells agents
    /// never to run it unasked.
    fn declare(
        &mut self,
        declaration: Declaration,
    ) -> impl Future<Output = Result<Declared, AppError>> + Send;

    /// `taskloom schema import-config`: the work root's config file's
    /// `tag_schema` and `tag_permits` (the keys that moved) declared in its
    /// project's store, in tagma's facets, then taken out of the file, its
    /// other keys and comments kept. Nothing to import is no error: it
    /// answers unchanged.
    fn import_config(&mut self) -> impl Future<Output = Result<Declared, AppError>> + Send;
}

impl<I, S, N, C, D, G> PermitService for Service<I, S, N, C, D, G>
where
    I: ProjectIdentity + Send,
    S: TaskStores + Send,
    N: Send,
    C: Clock + CommandIds + Send,
    D: ports::Delay + ports::Jitter + Send,
    G: Configuration + ConfigurationWriter + Send,
{
    async fn permit(&mut self, change: PermitChange) -> Result<Permitted, AppError> {
        let permit = Permit::parse(&change.permit)?;
        self.refuse_moved()?;
        let project = self.identity.for_write(None).map_err(AppError::Identity)?;
        let rules = self.seeded(&project).await?;
        let mut add = vec![permit.tag()];
        if !rules.closed() {
            add.extend(first_permits(&rules));
        }
        let declared = self
            .declaring(
                &project,
                Declare {
                    add: add.into_iter().map(Tag::from).collect(),
                    remove: Vec::new(),
                },
            )
            .await?;
        Ok(Permitted {
            permit: permit.to_string(),
            changed: declared.changed,
        })
    }

    async fn revoke(&mut self, change: PermitChange) -> Result<Permitted, AppError> {
        let permit = Permit::parse(&change.permit)?;
        self.refuse_moved()?;
        let not_declared = || AppError::PermitNotDeclared {
            permit: permit.to_string(),
        };
        let Some(project) = self.identity.for_read(None).map_err(AppError::Identity)? else {
            return Err(not_declared());
        };
        let rules = self.rules_of(&project).await?;
        if !rules.holds(&permit.tag()) {
            return Err(not_declared());
        }
        let harp_ids = self.carrying(&project, &permit).await?;
        if !harp_ids.is_empty() {
            return Err(AppError::PermitInUse {
                permit: permit.to_string(),
                harp_ids,
            });
        }
        let declared = self
            .declaring(
                &project,
                Declare {
                    add: Vec::new(),
                    remove: vec![permit.tag().into()],
                },
            )
            .await?;
        Ok(Permitted {
            permit: permit.to_string(),
            changed: declared.changed,
        })
    }

    async fn declare(&mut self, declaration: Declaration) -> Result<Declared, AppError> {
        self.refuse_moved()?;
        self.declared(declaration).await
    }

    async fn import_config(&mut self) -> Result<Declared, AppError> {
        self.importing_config().await
    }
}

/// What a first permit declares beside its own: the world closed, and a
/// permit for every target the rules constrain, so they stay writable.
fn first_permits(rules: &StoreRules) -> Vec<tagma_core::Tag> {
    let mut tags = vec![closing()];
    tags.extend(
        rules
            .targets()
            .iter()
            .map(|target| permit_tag(target, true)),
    );
    tags
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    I: ProjectIdentity,
    S: TaskStores,
    C: Clock + CommandIds,
    D: ports::Delay + ports::Jitter,
    G: Configuration + ConfigurationWriter,
{
    /// [`PermitService::import_config`]. The moved keys are not refused
    /// here, as everywhere else ([`ports::Configuration::moved`]): this is
    /// what moves them.
    async fn importing_config(&mut self) -> Result<Declared, AppError> {
        let schema = self
            .configuration
            .tag_schema()
            .map_err(AppError::Configuration)?;
        let permits = self
            .configuration
            .tag_permits()
            .map_err(AppError::Configuration)?;
        let mut add: Vec<tagma_core::Tag> = Vec::new();
        for declaration in &schema {
            add.extend(imported(declaration)?);
        }
        let project = self.identity.for_write(None).map_err(AppError::Identity)?;
        let rules = self.seeded(&project).await?;
        if let Some(permits) = &permits {
            add.extend(permitted(permits));
            if !rules.closed() {
                // Every target the store's rules, and those imported,
                // constrain stays admitted, as the old permits admitted a
                // declared target without a permit.
                let mut after = rules.clone();
                after.extend(&add);
                add.extend(first_permits(&after));
            }
        }
        let declared = match add.is_empty() {
            true => Declared {
                tags: rules.export(),
                changed: false,
            },
            false => {
                let declare = Declare {
                    add: add.into_iter().map(Tag::from).collect(),
                    remove: Vec::new(),
                };
                self.declaring(&project, declare).await?
            }
        };
        let retired = self
            .configuration
            .retire_moved()
            .map_err(AppError::Configuration)?;
        Ok(Declared {
            changed: declared.changed || retired,
            ..declared
        })
    }
}

/// The old `tag_schema` declaration `spelled`, in tagma's facets: an enum
/// of `a,b` one tag a value, a range of `lo,hi` as `lo..hi`, the formulas
/// in `taskloom.*`; any other facet as written, for `taskloom schema
/// check` to judge. Refused when it is not a tag in tagma's grammar.
fn imported(spelled: &str) -> Result<Vec<tagma_core::Tag>, AppError> {
    let trimmed = spelled.trim();
    let tag = tagma_core::Tag::parse(trimmed).map_err(|detail| {
        crate::refused(&tags::TagRefusal::Unparseable {
            tag: trimmed.to_string(),
            detail,
        })
    })?;
    let with = |namespace: &str, value: Option<String>| tagma_core::Tag {
        namespace: Some(namespace.to_string()),
        key: tag.key.clone(),
        value,
    };
    let value = tag.value.clone();
    Ok(match (tag.namespace.as_deref(), value) {
        (Some("tagma.enum"), Some(members)) => members
            .split(',')
            .map(str::trim)
            .filter(|member| !member.is_empty())
            .map(|member| with("tagma.enum", Some(member.to_string())))
            .collect(),
        (Some("tagma.range"), Some(range)) => match range.split_once(',') {
            Some((lo, hi)) => vec![with(
                "tagma.range",
                Some(format!("{}..{}", lo.trim(), hi.trim())),
            )],
            None => vec![tag],
        },
        (Some("tagma.priority_fn"), value) => vec![with(crate::store_schema::PRIORITY_FN, value)],
        (Some("tagma.decay_fn"), value) => vec![with(crate::store_schema::DECAY_FN, value)],
        _ => vec![tag],
    })
}

/// The old `tag_permits`, as tagma's permits: an open namespace `"ns:*"`,
/// a closed one's keys `"ns:key"` each, a flat tag its key.
fn permitted(permits: &ports::TagPermits) -> Vec<tagma_core::Tag> {
    let mut tags = Vec::new();
    for (namespace, permit) in &permits.namespaces {
        match permit {
            ports::NamespacePermit::Open => tags.push(permit_tag(&format!("{namespace}:*"), true)),
            ports::NamespacePermit::Closed(keys) => tags.extend(
                keys.iter()
                    .map(|key| permit_tag(&format!("{namespace}:{key}"), true)),
            ),
        }
    }
    tags.extend(permits.flat.iter().map(|key| permit_tag(key, true)));
    tags
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    I: ProjectIdentity,
    S: TaskStores,
    C: Clock,
    G: Configuration,
{
    /// The harp ids, sorted, of `project`'s tasks (whatever their status)
    /// carrying a tag `permit` admits, as tagma matches its pattern.
    async fn carrying(
        &mut self,
        project: &crate::Project,
        permit: &Permit,
    ) -> Result<Vec<String>, AppError> {
        let rows = self.rows_of(project).await?.unwrap_or_default();
        let alone = tagma_core::Rules::from_tags(&[closing(), permit.tag()]);
        let mut harp_ids: Vec<String> = rows
            .into_iter()
            .filter(|row| {
                let tags: Vec<tagma_core::Tag> = row.task.tags.iter().map(Tag::to_core).collect();
                let refused = alone.validate(&tags, &tagma_core::Change::default());
                tags.iter().any(|tag| {
                    !refused
                        .iter()
                        .any(|f| f.code == tagma_core::Code::NotPermitted && f.tag == *tag)
                })
            })
            .map(|row| row.task.harp_id)
            .collect();
        harp_ids.sort();
        Ok(harp_ids)
    }
}

/// A permit, parsed: the tagma pattern it is (`area:*`, `area:cli`,
/// `urgent`), and the spelling `taskloom permit` takes.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Permit {
    /// The canonical spelling: `area:`, `area:cli`, `urgent`.
    spelled: String,
    /// tagma's pattern for it.
    pattern: String,
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
        // tagma does not parse: it is parsed with a key put in. Anything
        // after the namespace but that key (`a:b:`, `a:b=c:`) is not a tag
        // tagma parses, or carries a value, and is refused below.
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
            if tags::is_reserved(namespace) || namespace.is_empty() {
                return Err(malformed());
            }
        }
        match (tag.namespace, namespace_only) {
            (Some(namespace), true) => {
                let spelled = Tag {
                    namespace: Some(namespace.clone()),
                    key: PLACEHOLDER.to_string(),
                    value: None,
                }
                .to_string();
                let cut = spelled.len() - PLACEHOLDER.len();
                Ok(Permit {
                    spelled: spelled[..cut].to_string(),
                    pattern: format!("{namespace}:*"),
                })
            }
            (Some(namespace), false) => Ok(Permit {
                spelled: Tag {
                    namespace: Some(namespace.clone()),
                    key: tag.key.clone(),
                    value: None,
                }
                .to_string(),
                pattern: format!("{namespace}:{}", tag.key),
            }),
            (None, false) => Ok(Permit {
                spelled: Tag {
                    namespace: None,
                    key: tag.key.clone(),
                    value: None,
                }
                .to_string(),
                pattern: tag.key,
            }),
            (None, true) => Err(malformed()),
        }
    }

    /// `tagma.permit:<pattern>=true`.
    fn tag(&self) -> tagma_core::Tag {
        permit_tag(&self.pattern, true)
    }
}

/// The key a namespace's permit is parsed with.
const PLACEHOLDER: &str = "x";

/// The canonical spelling, as `taskloom permit` takes it.
impl fmt::Display for Permit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.spelled)
    }
}

#[cfg(test)]
#[path = "permit.test.rs"]
mod tests;
