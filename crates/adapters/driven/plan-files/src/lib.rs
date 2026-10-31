//! The work tree's plan files, through the file-system seam: the
//! [`PlanFiles`] port. A plan's path is work-root-relative and
//! `/`-separated (or absolute, for a file outside the work root), joined
//! onto the work root component by component. The work root is found when
//! a call needs it, and not before: making the adapter does no I/O.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use angzarr_embedded_diagnostic::{Arg, Diagnostic, Report};
use file_system::{Fs, Kind, Open};
use os::Mode;
use ports::PlanFiles;

/// How the work root is found: the registry's search, given by the
/// composition root.
type FindRoot = Arc<dyn Fn() -> Result<PathBuf, Report> + Send + Sync>;

/// The plan files under the work root `root` finds, on `fs`.
#[derive(Clone)]
pub struct FsPlanFiles {
    fs: Arc<dyn Fs>,
    root: FindRoot,
}

impl fmt::Debug for FsPlanFiles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FsPlanFiles").field("fs", &self.fs).finish()
    }
}

/// Why a plan file could not be written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PlanFilesError {
    /// Creating it failed: `detail` is the OS's text.
    Create { path: String, detail: String },
}

impl fmt::Display for PlanFilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for PlanFilesError {
    fn code(&self) -> &str {
        match self {
            PlanFilesError::Create { .. } => "plan-files-create",
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            PlanFilesError::Create { path, detail } => vec![
                ("path", path.as_str().into()),
                ("ext_err_txt", detail.as_str().into()),
            ],
        }
    }
}

/// This crate's en-US messages, its name first.
pub const EN_US: (&str, &str) = (
    "plan-files",
    include_str!("../locales/en-US/plan-files.ftl"),
);

/// One value of every error, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![Box::new(PlanFilesError::Create {
        path: ".taskloom/plans/x.plan.md".to_string(),
        detail: "permission denied".to_string(),
    })]
}

impl FsPlanFiles {
    /// The plan files on `fs` under the work root `root` finds each time a
    /// call needs it.
    pub fn finding_root(
        fs: Arc<dyn Fs>,
        root: impl Fn() -> Result<PathBuf, Report> + Send + Sync + 'static,
    ) -> FsPlanFiles {
        FsPlanFiles {
            fs,
            root: Arc::new(root),
        }
    }

    /// `path` on the filesystem: an absolute one as it is, a relative one
    /// under the work root, joined component by component.
    fn located(&self, path: &str) -> Result<PathBuf, Report> {
        if Path::new(path).is_absolute() {
            return Ok(PathBuf::from(path));
        }
        let mut located = (self.root)()?;
        located.extend(path.split('/').filter(|part| !part.is_empty()));
        Ok(located)
    }

    fn created(&self, at: &Path, text: &[u8]) -> io::Result<bool> {
        if let Some(parent) = at.parent() {
            self.fs.create_dir_all(parent, Mode::DIR)?;
        }
        let mut file = match self.fs.open(at, Open::CreateNew(Mode::FILE)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
            Err(e) => return Err(e),
        };
        file.write_all(text)?;
        file.sync_all()?;
        Ok(true)
    }
}

impl PlanFiles for FsPlanFiles {
    fn work_root(&self) -> Result<String, Report> {
        let root = (self.root)()?;
        Ok(root.to_string_lossy().replace('\\', "/"))
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let at = self.located(path).ok()?;
        self.fs.read(&at).ok()
    }

    fn create(&self, path: &str, text: &[u8]) -> Result<bool, Report> {
        let at = self.located(path)?;
        self.created(&at, text).map_err(|e| {
            Report::of(&PlanFilesError::Create {
                path: path.to_string(),
                detail: e.to_string(),
            })
        })
    }

    fn plan_files(&self, dir: &str) -> Vec<String> {
        let Ok(at) = self.located(dir) else {
            return Vec::new();
        };
        let mut files: Vec<String> = self
            .fs
            .read_dir(&at)
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| entry.kind == Kind::File)
            .filter_map(|entry| entry.name.into_string().ok())
            .filter(|name| name.ends_with(".plan.md"))
            .map(|name| format!("{}/{name}", dir.trim_end_matches('/')))
            .collect();
        files.sort();
        files
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
