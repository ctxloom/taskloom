//! The ports the tasks application needs from outside, beyond
//! angzarr-ports' store ports: which project a call is about, where its store is, how ids
//! are minted, whether its store can purge a stream or say whether
//! anything changed, what time it is, how to wait, what is configured, the
//! JSONL task logs to import, what a directory's identity rests on, to show
//! and repair it, whether a project's database is sound, and, for plans,
//! the work tree's plan files, the tasks they govern and fresh plan ids. Each is a
//! trait a driven adapter (or the composition root) implements; nothing
//! here does I/O.
//!
//! Every future a port returns is `Send`, as angzarr-ports' are, so a server on a multi-threaded runtime can spawn a use case
//! generic over them (`tokio::spawn`).

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use angzarr_embedded_diagnostic::Report;
pub use angzarr_ports::angzarr_proto::v1::Uuid;
use angzarr_ports::{ChangeToken, Purge, ReadStore, Store, StoreError, Watch};
/// An instant: prost's `Timestamp`, as every recorded event carries one.
pub use prost_types::Timestamp;

mod plans;
mod project;

pub use plans::{Mint, PlanFiles, Tasks};
pub use project::{Change, Checked, IdSource, ProjectAdmin, Registered, StoreCheck, Survey};

/// A resolved project: its id, the work root it was resolved from, and
/// where its store is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    /// The project id ("jolly-mute-magma").
    pub id: String,
    /// The work root the id was resolved for: canonical, symlinks resolved.
    /// A pinned project's (an explicit id's) is the path the registry
    /// first holds for it, `None` when it holds none: the working
    /// directory is not walked for one.
    pub root: Option<String>,
    /// The project's database.
    pub store: PathBuf,
}

/// Which project a call is about.
///
/// Synchronous, and it blocks: resolving reads the filesystem, and a write
/// that binds a new project waits on the registry's file lock (`flock`). An
/// async server calls it through `spawn_blocking` (or its runtime's
/// equivalent), never directly on a runtime thread.
pub trait ProjectIdentity {
    /// The project a write goes to: `pinned` (an explicit id) if given,
    /// else the one already bound to the work root, else a new one, minted
    /// and bound.
    fn for_write(&mut self, pinned: Option<&str>) -> Result<Project, Report>;

    /// The project a read is about, never minting: `None` when the work
    /// root has no project yet.
    fn for_read(&mut self, pinned: Option<&str>) -> Result<Option<Project>, Report>;

    /// Every project with a directory of its own under the projects
    /// directory, by id: what a read across every project (`--global`)
    /// spans. A project's root is the path the registry first holds for
    /// it, canonical, or empty where it holds none.
    fn every_project(&mut self) -> Result<Vec<Project>, Report>;

    /// The working directory, canonical, when it lies in no work root of
    /// its own: no CTXLOOM_ROOT, and no git repository above it. A read
    /// that finds no project there reads every project instead; inside a
    /// work root (`None`) it reads none.
    fn rootless_directory(&mut self) -> Result<Option<String>, Report>;
}

/// A project's store, opened where its project says.
pub trait TaskStores {
    /// A project's store: every base port, one-moment reads, a purge and a
    /// change token.
    type Store: Store + ReadStore + Purge + Watch + Send + Sync;

    /// The project's store, to write: opened (and created) on first use and
    /// held after.
    fn open(
        &mut self,
        project: &Project,
    ) -> impl Future<Output = Result<&Self::Store, StoreError>> + Send;

    /// The project's store, to read: `None` if it has none yet, and
    /// nothing is created. A store older than the build is migrated, as
    /// [`TaskStores::open`] migrates one. A store already held for a write
    /// serves it too.
    fn open_existing(
        &mut self,
        project: &Project,
    ) -> impl Future<Output = Result<Option<&Self::Store>, StoreError>> + Send;
}

/// What a project's store answers a watcher: angzarr-ports'
/// [`ChangeToken`], and which handle on the store answered it, since
/// tokens compare only from one handle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct HeldToken {
    /// The handle that answered: a new one each time a store is opened.
    pub handle: u64,
    /// What that handle answered.
    pub change: ChangeToken,
}

/// Task stores whose stores say cheaply whether anything was committed.
pub trait WatchedStores: TaskStores {
    /// The token of `project`'s store, read through the store held for it
    /// or one opened as [`TaskStores::open_existing`] opens one and held
    /// after; `None` while the project has no store.
    fn change_token(
        &mut self,
        project: &Project,
    ) -> impl Future<Output = Result<Option<HeldToken>, StoreError>> + Send;
}

/// Fresh command ids: UUIDv7 (RFC 9562), as
/// angzarr-embedded expects of an application.
pub trait CommandIds {
    fn command_id(&mut self) -> Uuid;
}

/// Fresh, random names, and names derived from a request's key.
/// Uniqueness is not promised: a caller checks.
pub trait Names {
    /// A task's harp id: two words ("swift-amber").
    fn task_id(&mut self) -> String;

    /// The task harp id at `position` of the order `request_id` derives:
    /// two words, as [`Names::task_id`]'s. An add under the key looks for
    /// its own add on each of the key's first `KEYED_PROBES` positions, then
    /// adds on the first free one, so a retry finds the task it added
    /// wherever in that order it landed.
    ///
    /// Compatibility promise: a key's order is the same every time, in
    /// every process and release. In harp-names it is harp's seeded index
    /// (scheme v1) seeded by FNV-1a 64 of the key's UTF-8 bytes; together
    /// with the application's probe bound and order (`KEYED_PROBES`,
    /// positions from 0), changing any of them makes a retry across the
    /// change add its task again. Another key may derive the same harp.
    fn task_id_for(&mut self, request_id: &str, position: u64) -> String;

    /// A project's id: three words ("jolly-mute-magma").
    fn project_id(&mut self) -> String;
}

/// The instant a command is recorded at.
pub trait Clock {
    fn now(&mut self) -> Timestamp;
}

/// Waiting, between retries.
pub trait Delay {
    fn sleep(&mut self, duration: Duration) -> impl Future<Output = ()> + Send;
}

/// Randomness for spreading retries: contenders that wait the same time
/// collide again.
pub trait Jitter {
    /// A duration drawn uniformly from zero to `ceiling`, inclusive.
    fn up_to(&mut self, ceiling: Duration) -> Duration;
}

/// Taskloom's configuration, as far as the use cases read it: whether it
/// still sets a key that moved into the store, and, for `taskloom schema
/// import-config` alone, what those keys held.
///
/// The store's tag rules live on its schema stream
/// (docs/plans/self-hosted-schema.md): no use case reads them from a file,
/// a variable or an override. `tag_schema` and `tag_permits` are the keys
/// that moved, kept here only so the one-shot import can read them; they
/// go when the import is retired.
///
/// Synchronous, and it blocks on the filesystem, as [`ProjectIdentity`]
/// does.
pub trait Configuration {
    /// Refused (`tasks-config-key-moved`, naming `taskloom schema
    /// import-config`) where any layer still sets a key that moved into the
    /// store: a config file's `tag_schema` or `tag_permits`, the
    /// `TASKLOOM_CONFIG_TAG_SCHEMA` variable, a `--config-set` of either.
    /// Every use case but the import asks this first.
    ///
    /// The default answers that nothing is set: a configuration that
    /// knows nothing of those keys (ctxloom's, a test's) sets none.
    fn moved(&mut self) -> Result<(), Report> {
        Ok(())
    }

    /// The work root's config file's `tag_schema`, as written (the old
    /// declarations, `tagma.enum:"k:x"="a,b"` and `tagma.range:"k:n"="1,5"`
    /// among them): read by the import alone. Empty when it sets none.
    fn tag_schema(&mut self) -> Result<Vec<String>, Report>;

    /// The work root's config file's `tag_permits`, read by the import
    /// alone: `None` when it declares none.
    fn tag_permits(&mut self) -> Result<Option<TagPermits>, Report> {
        Ok(None)
    }
}

/// Where the moved keys are taken out of the work root's config file once
/// the import has declared them in the store.
///
/// Synchronous, and it blocks on the filesystem, as [`Configuration`]
/// does.
pub trait ConfigurationWriter {
    /// Replace the work root's config file's `tag_permits` with
    /// `permits`, keeping the rest of the file (its other keys, comments
    /// and layout) as written; the file and its directory made when there
    /// is none. Refused where there is no work root. Nothing calls it any
    /// more (`permit` and `revoke` declare in the store): it goes with the
    /// import.
    fn write_tag_permits(&mut self, permits: &TagPermits) -> Result<(), Report>;

    /// Take `tag_schema` and `tag_permits` out of the work root's config
    /// file, keeping its other keys, comments and layout as written:
    /// whether it held either. Nothing is written where it holds neither.
    ///
    /// The default holds neither and writes nothing: a configuration that
    /// knows nothing of those keys has none to take out.
    fn retire_moved(&mut self) -> Result<bool, Report> {
        Ok(false)
    }
}

/// A project's closed tag vocabulary: the namespaces its tasks' tags may
/// use, each open (any key) or closed (only its keys), and the tags with
/// no namespace it permits. A permit is of a target (`namespace:key`, or
/// a flat tag's key), never of a value: the schema governs values.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TagPermits {
    pub namespaces: BTreeMap<String, NamespacePermit>,
    /// The flat tags (no namespace) permitted, by key.
    pub flat: BTreeSet<String>,
}

/// What a permitted namespace admits.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NamespacePermit {
    /// Any key (`relates:<harp>`, `sig:<function>`).
    Open,
    /// Only these keys (`area:` with `cli`, `mcp`, ...).
    Closed(BTreeSet<String>),
}

/// One record of a JSONL task log, `~/.ctxloom/tasks/<project-id>.jsonl`:
/// one JSON line, with absent and null fields empty.
#[derive(Clone, PartialEq, Debug)]
pub struct JsonlRecord {
    /// Its line in the log, from 1.
    pub line: u32,
    /// The line, trimmed: the fold skips a line repeating an earlier one
    /// byte for byte, and the importer derives the record's command id
    /// from it.
    pub raw: String,
    pub op: JsonlOp,
    /// The task's harp id.
    pub task: String,
    pub text: String,
    pub status: String,
    pub trigger: String,
    /// The session that added the task (an add), or acted on it.
    pub session: String,
    /// On a repair add: the key of the displaced add it re-adds.
    pub repair_of: String,
    pub tags: Vec<String>,
    /// When it was recorded; 0001-01-01T00:00:00Z when the line has none.
    pub ts: Timestamp,
}

/// What a record does.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum JsonlOp {
    Add,
    Status,
    Text,
    Remove,
    Tag,
    Untag,
}

impl JsonlOp {
    /// The op as the log spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            JsonlOp::Add => "add",
            JsonlOp::Status => "status",
            JsonlOp::Text => "text",
            JsonlOp::Remove => "remove",
            JsonlOp::Tag => "tag",
            JsonlOp::Untag => "untag",
        }
    }
}

/// The per-project JSONL task logs, read and never written.
///
/// Synchronous, and it blocks on the filesystem, as [`ProjectIdentity`]
/// does.
pub trait JsonlTaskLogs {
    /// The ids of the projects that have a log, sorted.
    fn projects(&mut self) -> Result<Vec<String>, Report>;

    /// The project's records in the log's order, blank lines left out;
    /// `None` when it has no log. A line that is malformed JSON, has no op
    /// or has one the format does not know refuses the whole log.
    fn records(&mut self, project_id: &str) -> Result<Option<Vec<JsonlRecord>>, Report>;
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
