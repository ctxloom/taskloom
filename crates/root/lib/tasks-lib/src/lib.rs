//! The tasks use cases over taskloom's default adapters, wired in one
//! place: the composition root the `taskloom` binary, its servers (MCP
//! now, gRPC next) and ctxloom (by git dependency) all build from, so no
//! frontend wires an adapter of its own.
//!
//! [`Taskloom`] holds what the wiring reads from the process (HOME,
//! CTXLOOM_ROOT, CTXLOOM_PROJECT_ID and the environment's variables as
//! given, PATH among them), the `--config-set` overrides bound once here,
//! and the one [`Fs`] every adapter is given, which holds the working
//! directory (or why there is none: then only what needs it fails).
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

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use agent_configs::AgentConfigFiles;
use file_system::{Fs, Os};
use harp_names::HarpNames;
use jsonl_task_log::JsonlTaskLogDir;
use os::Flavor;
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
    let mut sources = vec![EN_US, annals::EN_US, annals_store_sql::EN_US];
    sources.extend(tasks_application::CATALOGS);
    sources.extend([
        project_application::EN_US,
        registration_application::EN_US,
        agent_configs::EN_US,
        file_lock::EN_US,
        file_system::EN_US,
        project_registry::EN_US,
        task_store_sqlite::EN_US,
        jsonl_task_log::EN_US,
        tasks_config::EN_US,
    ]);
    sources.extend_from_slice(frontends);
    presentation::Catalog::en_us(&sources)
}

/// The wiring: what the default adapters read from the process, the
/// overrides bound at composition, and the filesystem they all go
/// through. Cheap to clone; a clone shares the filesystem.
#[derive(Clone, Debug)]
pub struct Taskloom {
    /// The filesystem, and with it the working directory ([`Fs::cwd`]):
    /// its only holder.
    fs: Arc<dyn Fs>,
    /// HOME, CTXLOOM_ROOT and CTXLOOM_PROJECT_ID, as the registry reads
    /// them.
    env: Environment,
    /// The environment's variables as given: the configuration's layer,
    /// and PATH, read without loss.
    vars: Vec<(OsString, OsString)>,
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
    /// there is a work root), the home's, the variables (the configuration
    /// reads the UTF-8 ones) and the overrides. Building them does no
    /// I/O: no project is resolved and no work root sought until a call
    /// needs one, so a command that needs neither (`version`, `loadout`)
    /// touches nothing, and runs with no working directory.
    pub fn tasks(&self) -> Tasks {
        // The work root is sought when the configuration is read, by a
        // registry that mints nothing; with none (no working directory),
        // the home's config alone, and the call that resolves a project
        // says why.
        let finder = self.registry(());
        let config = TaskloomConfig::finding_root(
            Arc::clone(&self.fs),
            self.env.home.clone(),
            move || finder.work_root().ok(),
            self.vars.clone(),
            self.config_set.clone(),
        );
        tasks_application::Service::new(
            self.registry(HarpNames),
            SqliteStores::new(Arc::clone(&self.fs)),
            HarpNames,
            System,
            System,
        )
        .with_configuration(config)
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
        tasks_application::Importer::new(
            self.registry(HarpNames),
            SqliteStores::new(Arc::clone(&self.fs)),
            System,
            JsonlTaskLogDir::in_home(Arc::clone(&self.fs), self.env.home.as_deref()),
        )
    }

    /// The project use cases.
    pub fn projects(&self) -> Projects {
        project_application::Projects::new(
            self.registry(()),
            SqliteStores::new(Arc::clone(&self.fs)),
        )
    }

    /// The registration of `taskloom mcp` in the agents' configurations
    /// under HOME, its command resolved on the PATH among the variables
    /// given, as given (none there, none resolved: the process's is never
    /// read).
    pub fn registrations(&self) -> Registrations {
        let flavor = self.fs.os().flavor();
        let path = var(flavor, &self.vars, "PATH").cloned();
        let pathext =
            var(flavor, &self.vars, "PATHEXT").and_then(|ext| ext.to_str().map(str::to_string));
        registration_application::Registrations::taskloom(AgentConfigFiles {
            fs: Arc::clone(&self.fs),
            home: self.env.home.clone(),
            path,
            pathext,
            waits: file_lock::Waits::DEFAULT,
        })
    }

    /// The project registry over the wiring's filesystem and environment,
    /// minting ids from `names`.
    fn registry<N>(&self, names: N) -> ProjectRegistry<N> {
        ProjectRegistry::new(Arc::clone(&self.fs), self.env.clone(), names)
    }
}

/// What [`Taskloom`] is built from. Nothing is read from the process but
/// by [`Builder::from_process`]; each setter overrides one thing.
///
/// The working directory has one holder, the filesystem: on the real one
/// ([`Builder::new`]) it is the directory that filesystem is made over;
/// on one given ([`Builder::on`]) it is that filesystem's own
/// ([`Fs::cwd`]). The builder keeps no copy, so the two cannot disagree.
#[derive(Clone, Debug)]
pub struct Builder {
    /// The filesystem, or the working directory to make the real one over.
    ground: Ground,
    home: Option<PathBuf>,
    root: Option<PathBuf>,
    project_id: Option<String>,
    vars: Vec<(OsString, OsString)>,
    config_set: Vec<String>,
}

/// Where a [`Builder`]'s filesystem comes from.
#[derive(Clone, Debug)]
enum Ground {
    /// The real filesystem, over this working directory (refused at
    /// [`Builder::build`] unless absolute).
    Cwd(PathBuf),
    /// This filesystem, its working directory its own.
    Fs(Arc<dyn Fs>),
}

impl Builder {
    /// Wiring on the real filesystem over the working directory `cwd`,
    /// and nothing else: no home, no work root or project pinned, no
    /// variables (so no PATH), no overrides.
    pub fn new(cwd: impl Into<PathBuf>) -> Builder {
        Builder::over(Ground::Cwd(cwd.into()))
    }

    /// Wiring on the filesystem `fs`, in its working directory
    /// ([`Fs::cwd`], or none), and nothing else: no home, no work root or
    /// project pinned, no variables (so no PATH), no overrides.
    pub fn on(fs: Arc<dyn Fs>) -> Builder {
        Builder::over(Ground::Fs(fs))
    }

    fn over(ground: Ground) -> Builder {
        Builder {
            ground,
            home: None,
            root: None,
            project_id: None,
            vars: Vec::new(),
            config_set: Vec::new(),
        }
    }

    /// Wiring as the process gives it, as the binary runs: the real
    /// filesystem over the working directory (`Os::without_cwd` when it
    /// cannot be read: no fallback, and only what needs it fails), HOME,
    /// CTXLOOM_ROOT, CTXLOOM_PROJECT_ID, and the variables as given.
    pub fn from_process() -> Builder {
        Builder::from_env(std::env::current_dir(), std::env::vars_os().collect())
    }

    /// Wiring as a process with the working directory `cwd` (or why it
    /// cannot be read) and the variables `vars` would give it: what
    /// [`Builder::from_process`] reads, assembled.
    fn from_env(cwd: io::Result<PathBuf>, vars: Vec<(OsString, OsString)>) -> Builder {
        let ground = match cwd {
            Ok(cwd) => Ground::Cwd(cwd),
            Err(why) => Ground::Fs(Arc::new(Os::without_cwd(os::host(), why))),
        };
        let flavor = os::host().flavor();
        let var = |name: &str| var(flavor, &vars, name).cloned();
        Builder {
            // HOME, or USERPROFILE on Windows (as Go's os.UserHomeDir);
            // set empty, unset.
            home: var(flavor.home_var())
                .filter(|home| !home.is_empty())
                .map(PathBuf::from),
            root: var("CTXLOOM_ROOT").map(PathBuf::from),
            project_id: var("CTXLOOM_PROJECT_ID").and_then(|id| id.into_string().ok()),
            ..Builder::over(ground)
        }
        .vars(vars)
    }

    /// HOME: where the registry, the stores, the home's config and the
    /// JSONL task logs are.
    pub fn home(self, home: impl Into<PathBuf>) -> Builder {
        Builder {
            home: Some(home.into()),
            ..self
        }
    }

    /// CTXLOOM_ROOT: the work root, overriding the search for `.git`.
    pub fn root(self, root: impl Into<PathBuf>) -> Builder {
        Builder {
            root: Some(root.into()),
            ..self
        }
    }

    /// CTXLOOM_PROJECT_ID: the project, overriding the work root's (a
    /// request's own `project` still wins).
    pub fn project_id(self, id: impl Into<String>) -> Builder {
        Builder {
            project_id: Some(id.into()),
            ..self
        }
    }

    /// The environment's variables as given, replacing any given before:
    /// the configuration's layer (`TASKLOOM_CONFIG_<KEY>`, read where it
    /// is UTF-8), and PATH, where a registration's command is looked for.
    pub fn vars(self, vars: Vec<(OsString, OsString)>) -> Builder {
        Builder { vars, ..self }
    }

    /// The `--config-set` overrides (`<path>=<value>`), applied after
    /// every other layer, replacing any given before: bound once, here.
    pub fn config_set(self, overrides: Vec<String>) -> Builder {
        Builder {
            config_set: overrides,
            ..self
        }
    }

    /// The wiring. On the real filesystem ([`Builder::new`]), refused
    /// ([`BuildError::RelativeCwd`]) unless the working directory given is
    /// absolute. A filesystem with no working directory is not refused:
    /// resolving a project or a relative path fails then, in the call
    /// that needs it.
    pub fn build(self) -> Result<Taskloom, BuildError> {
        let fs: Arc<dyn Fs> = match self.ground {
            Ground::Cwd(cwd) => {
                Arc::new(
                    Os::new(os::host(), &cwd).map_err(|_| BuildError::RelativeCwd {
                        cwd: cwd.display().to_string(),
                    })?,
                )
            }
            Ground::Fs(fs) => fs,
        };
        Ok(Taskloom {
            fs,
            env: Environment {
                home: self.home,
                root: self.root,
                project_id: self.project_id,
            },
            vars: self.vars,
            config_set: self.config_set,
        })
    }
}

/// The variable `name` among `vars`, its name compared as `flavor`
/// compares names (in any case on Windows); one whose name is not UTF-8 is
/// no taskloom variable.
fn var<'a>(flavor: Flavor, vars: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsString> {
    let key = flavor.env_key(name);
    vars.iter()
        .find(|(n, _)| n.to_str().is_some_and(|n| flavor.env_key(n) == key))
        .map(|(_, value)| value)
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
