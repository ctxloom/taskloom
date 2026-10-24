//! The tasks use cases over taskloom's default adapters, wired in one
//! place: the composition root the `taskloom` binary, its servers (MCP
//! now, gRPC next) and ctxloom (by git dependency) all build from, so no
//! frontend wires an adapter of its own.
//!
//! [`Taskloom`] holds what the wiring reads from the process (HOME, the
//! working directory, CTXLOOM_ROOT, CTXLOOM_PROJECT_ID, the environment's
//! variables and PATH) and the one [`Fs`] every adapter is given. It
//! builds each use case only when asked: [`Taskloom::tasks`] for a
//! command, [`Taskloom::services`] for a server that makes one per call,
//! [`Taskloom::importer`] for `import` alone (no other use case can reach
//! the JSONL task logs), [`Taskloom::projects`] and
//! [`Taskloom::registrations`].
//!
//! What a frontend reads back is the application's own typed values (a
//! listing's reach, hidden counts and priority notes, a write's project,
//! a watch's changes), from the application crates, which a caller
//! depends on directly: this crate re-exports none of them, and adds no
//! shape of its own, and none of any one transport's. Rendering them is
//! each frontend's, with the catalog [`catalog`] assembles.

mod error;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use agent_configs::AgentConfigFiles;
use file_system::Fs;
use harp_names::HarpNames;
use jsonl_task_log::JsonlTaskLogDir;
use project_registry::ProjectRegistry;
use system::System;
use task_store_sqlite::SqliteStores;
use tasks_config::TaskloomConfig;

pub use error::{diagnostic_samples, BuildError, EN_US};
pub use presentation::{Catalog, CatalogError, Source};

/// The tasks use cases (commands, queries, prune and watch) over the
/// default adapters: what a command, or one server call, runs on.
pub type Tasks = tasks_application::Service<
    ProjectRegistry<HarpNames>,
    SqliteStores,
    HarpNames,
    System,
    System,
    TaskloomConfig,
>;

/// The import of the JSONL task logs under HOME into each project's
/// store, the only use case that reads them.
pub type Import =
    tasks_application::Importer<ProjectRegistry<HarpNames>, SqliteStores, System, JsonlTaskLogDir>;

/// The project use cases over the registry and the SQLite store's
/// integrity check.
pub type Projects = project_application::Projects<ProjectRegistry<HarpNames>, SqliteStores>;

/// `taskloom mcp` registered in the agents' configurations under HOME.
pub type Registrations = registration_application::Registrations<AgentConfigFiles>;

/// The en-US catalog of every diagnostic the wiring can raise (its own,
/// annals', the use cases', every driven and platform adapter's), then
/// `frontends'`, each frontend's own (the CLI's, the MCP server's, ...),
/// which only the frontend knows.
pub fn catalog(frontends: &[Source]) -> Result<Catalog, CatalogError> {
    let _ = frontends;
    todo!("EN_US, annals, annals-store-sql, the use cases' and every adapter's, then frontends")
}

/// The wiring: what the default adapters read from the process, and the
/// filesystem they all go through. Cheap to clone; a clone shares the
/// filesystem.
#[derive(Clone, Debug)]
pub struct Taskloom {
    fs: Arc<dyn Fs>,
    home: Option<PathBuf>,
    cwd: Result<PathBuf, String>,
    root: Option<PathBuf>,
    project_id: Option<String>,
    vars: Vec<(String, String)>,
    path: Option<OsString>,
}

impl Taskloom {
    /// The wiring as the process gives it: [`Builder::from_process`],
    /// built.
    pub fn from_process() -> Result<Taskloom, BuildError> {
        Builder::from_process().build()
    }

    /// The filesystem every adapter is given.
    pub fn fs(&self) -> &Arc<dyn Fs> {
        &self.fs
    }

    /// The tasks use cases, configured from the work root's config (where
    /// there is a work root), the home's, and the variables. Resolves no
    /// project: each call does.
    pub fn tasks(&self) -> Tasks {
        let _ = (
            &self.home,
            &self.cwd,
            &self.root,
            &self.project_id,
            &self.vars,
        );
        todo!("Service::new(registry, SqliteStores, HarpNames, System, System).with_configuration")
    }

    /// A maker of [`Taskloom::tasks`], for a server that serves each call
    /// on a service of its own (MCP's `TaskTools::new`, gRPC's). It owns a
    /// clone of the wiring, so it outlives this value and crosses threads.
    pub fn services(&self) -> impl Fn() -> Tasks + Send + Sync + 'static {
        let wiring = self.clone();
        move || wiring.tasks()
    }

    /// The importer over the JSONL task logs under HOME
    /// (`~/.ctxloom/tasks`), into the stores [`Taskloom::tasks`] reads.
    pub fn importer(&self) -> Import {
        todo!("Importer::new(registry, SqliteStores, System, JsonlTaskLogDir::in_home)")
    }

    /// The project use cases.
    pub fn projects(&self) -> Projects {
        todo!("Projects::new(registry, SqliteStores)")
    }

    /// The registration of `taskloom mcp` in the agents' configurations
    /// under HOME, its command resolved on PATH.
    pub fn registrations(&self) -> Registrations {
        let _ = &self.path;
        todo!("Registrations::taskloom(AgentConfigFiles {{ home, path, waits }})")
    }
}

/// What [`Taskloom`] is built from. Nothing is read from the process but
/// by [`Builder::from_process`]; each setter overrides one thing.
#[derive(Clone, Debug)]
pub struct Builder {
    cwd: Result<PathBuf, String>,
    home: Option<PathBuf>,
    root: Option<PathBuf>,
    project_id: Option<String>,
    vars: Vec<(String, String)>,
    path: Option<OsString>,
    fs: Option<Arc<dyn Fs>>,
}

impl Builder {
    /// Wiring for the working directory `cwd`, and nothing else: no home,
    /// no work root or project pinned, no variables, no PATH, and the real
    /// filesystem.
    pub fn new(cwd: impl Into<PathBuf>) -> Builder {
        Builder {
            cwd: Ok(cwd.into()),
            home: None,
            root: None,
            project_id: None,
            vars: Vec::new(),
            path: None,
            fs: None,
        }
    }

    /// Wiring as the process gives it, as the binary runs: HOME, the
    /// working directory (or why it cannot be read: a command that needs
    /// it says so), CTXLOOM_ROOT, CTXLOOM_PROJECT_ID, the variables whose
    /// names and values are UTF-8, and PATH.
    pub fn from_process() -> Builder {
        todo!("read the process once, as main does today")
    }

    /// HOME: where the registry, the stores, the home's config and the
    /// JSONL task logs are.
    pub fn home(self, home: impl Into<PathBuf>) -> Builder {
        let _ = home;
        todo!()
    }

    /// CTXLOOM_ROOT: the work root, overriding the search for `.git`.
    pub fn root(self, root: impl Into<PathBuf>) -> Builder {
        let _ = root;
        todo!()
    }

    /// CTXLOOM_PROJECT_ID: the project, overriding the work root's (a
    /// request's own `project` still wins).
    pub fn project_id(self, id: impl Into<String>) -> Builder {
        let _ = id;
        todo!()
    }

    /// The environment's variables, as the config reads them
    /// (`TASKLOOM_CONFIG_<KEY>`), replacing any given before.
    pub fn vars(self, vars: Vec<(String, String)>) -> Builder {
        let _ = vars;
        todo!()
    }

    /// PATH, where a registration's command is looked for.
    pub fn path(self, path: impl Into<OsString>) -> Builder {
        let _ = path;
        todo!()
    }

    /// The filesystem every adapter goes through, in place of the real
    /// one over the working directory.
    pub fn fs(self, fs: Arc<dyn Fs>) -> Builder {
        let _ = fs;
        todo!()
    }

    /// The wiring. With no filesystem given, the real one, relative paths
    /// resolved against the working directory (against the top when it
    /// cannot be read), refused ([`BuildError::RelativeCwd`]) when that
    /// directory is not absolute.
    pub fn build(self) -> Result<Taskloom, BuildError> {
        let _ = (
            self.cwd,
            self.home,
            self.root,
            self.project_id,
            self.vars,
            self.path,
            self.fs,
        );
        todo!("file_system::Os::new(cwd) unless an fs was given")
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
