//! Project identity: which project a directory is, and where its store is.
//!
//! The work root is `CTXLOOM_ROOT`, else the nearest ancestor holding
//! `.git`, else the working directory; canonical (symlinks resolved, else
//! cleaned lexically), and registered so.
//! Registry entries are compared canonical too. Its project id is, in order: the
//! one pinned (`--project`), `CTXLOOM_PROJECT_ID`, the marker
//! `<root>/.project-id`, the older marker `<root>/.ctxloom/project-id`
//! (adopted into `.project-id` on a write), and the registry entry for the
//! root's path. A linked worktree with no marker of its own (neither) is
//! its primary checkout's project, found through git's common directory:
//! resolved, bound and marked there, so no marker is written in the
//! worktree to be committed and merged back over the primary's. A write with none of these mints an id, under the
//! registry's lock, and binds it in both the registry and the marker; a read
//! with none finds no project. The store is
//! `~/.ctxloom/projects/<id>/state.db`.
//!
//! The same registry is behind the `ProjectAdmin` port: a survey of what
//! a directory's identity rests on, and changes to markers, the registry
//! and the projects directory, made under the registry's lock only while
//! a fresh survey matches the one they were planned from. The lock is
//! file-lock's: taken again when what it was granted is no longer the file
//! at its path (`ctxloom clean` reaps lock files), and waited for at most
//! [`LOCK_WAIT`].

mod admin;
mod error;
mod id;
mod index;
mod path;
mod worktree;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use angzarr_embedded_diagnostic::Report;
use file_system::Fs;
use ports::{IdSource, Names, Project, ProjectIdentity};

pub use error::{diagnostic_samples, RegistryError, EN_US};
pub use index::LOCK_WAIT;

/// How many project ids a mint draws before giving up, each already
/// registered.
pub const MINT_ATTEMPTS: u32 = 100;

/// What resolution reads from the process's environment. Where it runs
/// is the filesystem's working directory ([`Fs::cwd`]), asked of the
/// filesystem the registry is given, so the two cannot disagree.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Environment {
    /// HOME.
    pub home: Option<PathBuf>,
    /// CTXLOOM_ROOT: the work root, overriding the search for `.git`.
    pub root: Option<PathBuf>,
    /// CTXLOOM_PROJECT_ID: the project, overriding the work root's.
    pub project_id: Option<String>,
}

/// Project identity over the filesystem `fs`, minting ids from `names`.
pub struct ProjectRegistry<N> {
    fs: Arc<dyn Fs>,
    env: Environment,
    names: N,
    /// How long a change waits for the registry's lock.
    lock_wait: Duration,
}

impl<N> ProjectRegistry<N> {
    pub fn new(fs: Arc<dyn Fs>, env: Environment, names: N) -> Self {
        ProjectRegistry {
            fs,
            env,
            names,
            lock_wait: index::LOCK_WAIT,
        }
    }

    /// The same, waiting up to `wait` for the registry's lock rather than
    /// [`LOCK_WAIT`].
    pub fn with_lock_wait(self, wait: Duration) -> Self {
        ProjectRegistry {
            lock_wait: wait,
            ..self
        }
    }

    /// The work root, canonical: CTXLOOM_ROOT (under the working directory
    /// if relative), else the nearest ancestor of the working directory
    /// holding `.git` (a directory, or a linked worktree's file), else the
    /// working directory.
    pub fn work_root(&self) -> Result<PathBuf, RegistryError> {
        if let Some(root) = self.env.root.as_ref().filter(|r| !r.as_os_str().is_empty()) {
            if root.is_absolute() {
                return Ok(self.canonical(root));
            }
            return Ok(self.canonical(&self.cwd()?.join(root)));
        }
        let cwd = self.cwd()?;
        let root = cwd
            .ancestors()
            .find(|dir| self.exists(&dir.join(".git")))
            .unwrap_or(cwd);
        Ok(self.canonical(root))
    }

    /// `path`, canonical on the registry's filesystem.
    pub(crate) fn canonical(&self, path: &Path) -> PathBuf {
        path::canonical(&*self.fs, path)
    }

    /// Whether anything is at `path`; what cannot be looked at is not.
    fn exists(&self, path: &Path) -> bool {
        self.fs.exists(path).unwrap_or(false)
    }

    /// The working directory, canonical, if neither CTXLOOM_ROOT nor a
    /// `.git` above it gives it a work root of its own.
    fn rootless(&self) -> Result<Option<String>, RegistryError> {
        if self
            .env
            .root
            .as_ref()
            .is_some_and(|r| !r.as_os_str().is_empty())
        {
            return Ok(None);
        }
        let cwd = self.cwd()?;
        if cwd.ancestors().any(|dir| self.exists(&dir.join(".git"))) {
            return Ok(None);
        }
        Ok(Some(self.canonical(cwd).display().to_string()))
    }

    /// The filesystem's working directory, or why it has none.
    fn cwd(&self) -> Result<&Path, RegistryError> {
        self.fs
            .cwd()
            .map_err(|e| RegistryError::NoWorkingDirectory { detail: e.reason })
    }

    pub(crate) fn home(&self) -> Result<&Path, RegistryError> {
        self.env
            .home
            .as_deref()
            .filter(|h| !h.as_os_str().is_empty())
            .ok_or(RegistryError::NoHome {
                flavor: self.fs.os().flavor(),
            })
    }

    /// The directory the work root's identity is kept in: the work root
    /// itself when it has a marker of its own or is no linked worktree,
    /// else its primary checkout, so a linked worktree is its primary
    /// checkout's project and writes no marker of its own.
    pub fn identity_root(&self) -> Result<PathBuf, RegistryError> {
        let root = self.work_root()?;
        if self.exists(&marker(&root)) || self.exists(&legacy_marker(&root)) {
            return Ok(root);
        }
        Ok(worktree::primary_checkout(&*self.fs, &root)?.unwrap_or(root))
    }

    /// The project `id` names, resolved for `root` (`None`: a pinned
    /// project the registry holds no path for).
    fn project(&self, id: String, root: Option<String>) -> Result<Project, RegistryError> {
        let store = self
            .home()?
            .join(".ctxloom")
            .join("projects")
            .join(&id)
            .join("state.db");
        Ok(Project { id, root, store })
    }

    /// The id given explicitly: the pin, else CTXLOOM_PROJECT_ID. A pin is
    /// held to [`id::valid`] whatever it is, so an empty one is refused,
    /// never read as no pin; an empty CTXLOOM_PROJECT_ID is unset, as an
    /// empty variable of the environment is.
    pub(crate) fn pinned(
        &self,
        pinned: Option<&str>,
    ) -> Result<Option<(String, IdSource)>, RegistryError> {
        if let Some(id) = pinned {
            return id::valid(id, "--project").map(|id| Some((id, IdSource::Flag)));
        }
        match self.env.project_id.as_deref().filter(|p| !p.is_empty()) {
            Some(id) => {
                id::valid(id, "CTXLOOM_PROJECT_ID").map(|id| Some((id, IdSource::Environment)))
            }
            None => Ok(None),
        }
    }

    /// The id the work root is already bound to, and what binds it,
    /// adopting the older marker into `.project-id` when `adopt`.
    pub(crate) fn bound(
        &self,
        root: &Path,
        adopt: bool,
    ) -> Result<Option<(String, IdSource)>, RegistryError> {
        if let Some(id) = read_marker(&*self.fs, &marker(root))? {
            return Ok(Some((id, IdSource::Marker)));
        }
        if let Some(id) = read_marker(&*self.fs, &legacy_marker(root))? {
            if adopt {
                write_marker(&*self.fs, root, &id)?;
            }
            return Ok(Some((id, IdSource::LegacyMarker)));
        }
        let home = self.home()?;
        let index = index::load(&*self.fs, &index::path(home))?;
        Ok(index
            .id_for(&*self.fs, root)
            .map(|id| (id.to_string(), IdSource::Registry)))
    }
}

/// Resolving may bind a new project, minting its id from `names`.
impl<N: Names> ProjectRegistry<N> {
    /// Bind the work root to a project id under the registry's lock: the
    /// one another process bound while this one waited, else a new one.
    fn bind(&mut self, root: &Path) -> Result<String, RegistryError> {
        let path = index::path(self.home()?);
        let _locked = index::lock_within(&*self.fs, &path, self.lock_wait)?;
        if let Some((id, _)) = self.bound(root, true)? {
            return Ok(id);
        }
        let mut index = index::load(&*self.fs, &path)?;
        let id = self.mint(&index)?;
        index.register(&id, &root.display().to_string());
        index::save(&*self.fs, &path, &index)?;
        write_marker(&*self.fs, root, &id)?;
        Ok(id)
    }

    /// A project id neither `index` nor a database holds: an orphan's
    /// database (no registry entry names it) still holds its id's tasks.
    fn mint(&mut self, index: &index::Index) -> Result<String, RegistryError> {
        let projects = self.home()?.join(".ctxloom").join("projects");
        for _ in 0..MINT_ATTEMPTS {
            let id = self.names.project_id();
            if !index.holds(&id) && !self.exists(&projects.join(&id)) {
                return Ok(id);
            }
        }
        Err(RegistryError::Exhausted {
            attempts: MINT_ATTEMPTS,
        })
    }

    /// The project a call acts on. A pinned one (the pin, else
    /// CTXLOOM_PROJECT_ID) is resolved first, walking no working
    /// directory: it is rooted where the registry first holds it, or
    /// nowhere (`None`), as [`ProjectIdentity::every_project`]
    /// roots each project. Otherwise the identity root's, bound to a new
    /// id on a write that finds none.
    fn resolve(
        &mut self,
        pinned: Option<&str>,
        write: bool,
    ) -> Result<Option<Project>, RegistryError> {
        if let Some((id, _)) = self.pinned(pinned)? {
            let index = index::load(&*self.fs, &index::path(self.home()?))?;
            let root = index.root_of(&*self.fs, &id);
            return self.project(id, root).map(Some);
        }
        let root = self.identity_root()?;
        let id = match self.bound(&root, write)? {
            Some((id, _)) => Some(id),
            None if write => Some(self.bind(&root)?),
            None => None,
        };
        let root = root.display().to_string();
        id.map(|id| self.project(id, Some(root))).transpose()
    }
}

impl<N: Names> ProjectIdentity for ProjectRegistry<N> {
    fn for_write(&mut self, pinned: Option<&str>) -> Result<Project, Report> {
        match self.resolve(pinned, true) {
            Ok(Some(project)) => Ok(project),
            Ok(None) => unreachable!("a write binds a project"),
            Err(e) => Err(Report::of(&e)),
        }
    }

    fn for_read(&mut self, pinned: Option<&str>) -> Result<Option<Project>, Report> {
        self.resolve(pinned, false).map_err(|e| Report::of(&e))
    }

    fn every_project(&mut self) -> Result<Vec<Project>, Report> {
        self.every().map_err(|e| Report::of(&e))
    }

    fn rootless_directory(&mut self) -> Result<Option<String>, Report> {
        self.rootless().map_err(|e| Report::of(&e))
    }
}

/// The marker naming `root`'s project.
pub(crate) fn marker(root: &Path) -> PathBuf {
    root.join(".project-id")
}

/// The older, private marker.
pub(crate) fn legacy_marker(root: &Path) -> PathBuf {
    root.join(".ctxloom").join("project-id")
}

/// The id a marker at `path` holds; none if it is absent or blank.
pub(crate) fn read_marker(fs: &dyn Fs, path: &Path) -> Result<Option<String>, RegistryError> {
    let text = match fs.read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(RegistryError::io(path, e)),
    };
    match text.trim() {
        "" => Ok(None),
        id => id::valid(id, &path.display().to_string()).map(Some),
    }
}

pub(crate) fn write_marker(fs: &dyn Fs, root: &Path, id: &str) -> Result<(), RegistryError> {
    index::write_atomically(fs, &marker(root), format!("{id}\n").as_bytes())
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../../adapters/driving/presentation/src/english.test.rs"]
mod english;
