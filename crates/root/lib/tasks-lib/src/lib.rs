//! The tasks use cases over taskloom's default adapters, wired in one
//! place: the composition root the `taskloom` binary, its servers (MCP
//! now, gRPC next) and ctxloom (by git dependency) all build from, so no
//! frontend wires an adapter of its own.
//!
//! [`Taskloom`] holds what the wiring reads from the process (HOME, the
//! working directory, CTXLOOM_ROOT, CTXLOOM_PROJECT_ID and the
//! environment's variables, PATH among them), the `--config-set`
//! overrides bound once here, and the one [`Fs`] every adapter is given.
//! It builds each use case only when asked, and building one does no I/O:
//! [`Taskloom::tasks`] for a command, [`Taskloom::services`] for a server
//! that makes one per call, [`Taskloom::importer`] for `import` alone (no
//! other use case can reach the JSONL task logs), [`Taskloom::projects`]
//! and [`Taskloom::registrations`].
//!
//! What a frontend reads back is the application's own typed values (a
//! listing's reach, hidden counts and priority notes, a write's project,
//! a watch's changes), from the application crates, which a caller
//! depends on directly: this crate re-exports none of them, nor
//! presentation's catalog types, and adds no shape of its own, and none
//! of any one transport's. Rendering them is each frontend's, with the
//! catalog [`catalog`] assembles.

mod error;

use std::path::PathBuf;
use std::sync::Arc;

use agent_configs::AgentConfigFiles;
use file_system::Fs;
use harp_names::HarpNames;
use jsonl_task_log::JsonlTaskLogDir;
use project_registry::{Environment, ProjectRegistry};
use system::System;
use task_store_sqlite::SqliteStores;
use tasks_config::TaskloomConfig;

pub use error::{diagnostic_samples, BuildError, EN_US};

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

/// The project use cases over the registry (surveying and changing it
/// mints nothing: no names) and the SQLite store's integrity check.
pub type Projects = project_application::Projects<ProjectRegistry<()>, SqliteStores>;

/// `taskloom mcp` registered in the agents' configurations under HOME.
pub type Registrations = registration_application::Registrations<AgentConfigFiles>;

/// The en-US catalog of every diagnostic the wiring can raise (its own,
/// annals', the use cases', every driven and platform adapter's), then
/// `frontends'`, each frontend's own (the CLI's, the MCP server's, ...),
/// which only the frontend knows. The types are presentation's, which a
/// caller depends on directly.
pub fn catalog(
    frontends: &[presentation::Source],
) -> Result<presentation::Catalog, presentation::CatalogError> {
    let _ = frontends;
    todo!("EN_US, annals, annals-store-sql, the use cases' and every adapter's, then frontends")
}

/// The wiring: what the default adapters read from the process, the
/// overrides bound at composition, and the filesystem they all go
/// through. Cheap to clone; a clone shares the filesystem.
#[derive(Clone, Debug)]
pub struct Taskloom {
    fs: Arc<dyn Fs>,
    /// HOME, the working directory (the wiring's only copy of it),
    /// CTXLOOM_ROOT and CTXLOOM_PROJECT_ID, as the registry reads them.
    env: Environment,
    /// The environment's variables: the configuration's layer, and PATH.
    vars: Vec<(String, String)>,
    /// The `--config-set` overrides, applied last.
    config_set: Vec<String>,
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
    /// there is a work root), the home's, the variables and the
    /// overrides. Building them does no I/O: no project is resolved and
    /// no work root searched for until a call needs one, so a command
    /// that needs neither (`version`, `loadout`) touches nothing.
    pub fn tasks(&self) -> Tasks {
        let _ = (&self.env, &self.vars, &self.config_set);
        todo!("Service::new(registry, SqliteStores, HarpNames, System, System).with_configuration, the work root resolved lazily")
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
        todo!("Projects::new(ProjectRegistry::new(fs, env, ()), SqliteStores)")
    }

    /// The registration of `taskloom mcp` in the agents' configurations
    /// under HOME, its command resolved on the PATH among the variables
    /// given (none there, none resolved: the process's is never read).
    pub fn registrations(&self) -> Registrations {
        todo!(
            "Registrations::taskloom(AgentConfigFiles {{ fs, home, path: PATH from vars, waits }})"
        )
    }
}

/// What [`Taskloom`] is built from. Nothing is read from the process but
/// by [`Builder::from_process`]; each setter overrides one thing.
///
/// The working directory has one holder. It is given once, to
/// [`Builder::new`] (or read by [`Builder::from_process`]); with no
/// filesystem given, the real one is made over it (`Os::new(cwd)`), and
/// the wiring keeps it only where the registry's [`Environment`] needs
/// it.
///
/// Interim rule, until the seam can say what a filesystem's working
/// directory is (`Fs::cwd()`, with the OS crate): a filesystem given to
/// [`Builder::fs`] must be one whose working directory is the builder's,
/// made over that same path (`Os::new(cwd)`, `Rooted::new(root, cwd)`).
/// Nothing checks it. Once the seam can answer, the working directory is
/// asked of the filesystem given, and the two cannot disagree.
#[derive(Clone, Debug)]
pub struct Builder {
    /// The working directory, or why the process's could not be read
    /// (refused at [`Builder::build`]).
    cwd: Result<PathBuf, String>,
    home: Option<PathBuf>,
    root: Option<PathBuf>,
    project_id: Option<String>,
    vars: Vec<(String, String)>,
    config_set: Vec<String>,
    fs: Option<Arc<dyn Fs>>,
}

impl Builder {
    /// Wiring for the working directory `cwd`, and nothing else: no home,
    /// no work root or project pinned, no variables (so no PATH), no
    /// overrides, and the real filesystem over `cwd`.
    pub fn new(cwd: impl Into<PathBuf>) -> Builder {
        Builder::at(Ok(cwd.into()))
    }

    /// Wiring for the working directory `cwd`, or why the process's could
    /// not be read: what [`Builder::from_process`] starts from.
    pub(crate) fn at(cwd: Result<PathBuf, String>) -> Builder {
        let _ = cwd;
        todo!()
    }

    /// Wiring as the process gives it, as the binary runs: HOME, the
    /// working directory (or why it cannot be read, which
    /// [`Builder::build`] refuses), CTXLOOM_ROOT, CTXLOOM_PROJECT_ID, and
    /// the variables whose names and values are UTF-8, PATH among them.
    pub fn from_process() -> Builder {
        todo!("Builder::at(std::env::current_dir()), then the process's variables, read once")
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

    /// The environment's variables, replacing any given before: the
    /// configuration's layer (`TASKLOOM_CONFIG_<KEY>`), and PATH, where a
    /// registration's command is looked for.
    pub fn vars(self, vars: Vec<(String, String)>) -> Builder {
        let _ = vars;
        todo!()
    }

    /// The `--config-set` overrides (`<path>=<value>`), applied after
    /// every other layer, replacing any given before: bound once, here.
    pub fn config_set(self, overrides: Vec<String>) -> Builder {
        let _ = overrides;
        todo!()
    }

    /// The filesystem every adapter goes through, in place of the real
    /// one over the working directory. It must work in the builder's
    /// working directory (the interim rule on [`Builder`]).
    pub fn fs(self, fs: Arc<dyn Fs>) -> Builder {
        let _ = fs;
        todo!()
    }

    /// The wiring. Refused when the working directory could not be read
    /// ([`BuildError::NoWorkingDirectory`]) or is not absolute
    /// ([`BuildError::RelativeCwd`]); with no filesystem given, the real
    /// one, relative paths resolved against it.
    pub fn build(self) -> Result<Taskloom, BuildError> {
        let _ = (
            self.cwd,
            self.home,
            self.root,
            self.project_id,
            self.vars,
            self.config_set,
            self.fs,
        );
        todo!("file_system::Os::new(cwd) unless an fs was given")
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
