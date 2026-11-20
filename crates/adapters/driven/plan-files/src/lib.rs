//! The plan files, through the file-system seam: the [`PlanFiles`] port.
//! Every path is absolute and `/`-separated. The work root is found when a
//! call needs it, and not before: making the adapter does no I/O.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use angzarr_diagnostic::{code, Arg, Diagnostic, Report};
use file_system::{Fs, Kind, Open, Perms};
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
    /// Replacing it failed: `detail` is the OS's text.
    Replace { path: String, detail: String },
}

impl fmt::Display for PlanFilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for PlanFilesError {
    fn code(&self) -> &str {
        match self {
            PlanFilesError::Create { .. } => code!("create"),
            PlanFilesError::Replace { .. } => code!("replace"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            PlanFilesError::Create { path, detail } | PlanFilesError::Replace { path, detail } => {
                vec![
                    ("path", path.as_str().into()),
                    ("ext_err_txt", detail.as_str().into()),
                ]
            }
        }
    }
}

/// This crate's en-US messages, its name first.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!(concat!(env!("OUT_DIR"), "/en-US.ftl")),
);

/// One value of every error, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(PlanFilesError::Create {
            path: ".taskloom/plans/x.plan.md".to_string(),
            detail: "permission denied".to_string(),
        }),
        Box::new(PlanFilesError::Replace {
            path: "docs/x.plan.md".to_string(),
            detail: "read-only file system".to_string(),
        }),
    ]
}

/// The directories a scan does not enter: version control's, and what a
/// build leaves.
const SKIPPED: [&str; 3] = [".git", "node_modules", "target"];

/// A plan file's suffix.
const SUFFIX: &str = ".plan.md";

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

    fn created(&self, at: &Path, text: &str) -> io::Result<bool> {
        if let Some(parent) = at.parent() {
            self.fs.create_dir_all(parent, Mode::DIR)?;
        }
        let mut file = match self.fs.open(at, Open::CreateNew(Mode::FILE)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
            Err(e) => return Err(e),
        };
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        Ok(true)
    }

    /// The plan files under `dir`, appended to `found`.
    fn walk(&self, dir: &Path, found: &mut Vec<String>) {
        let Ok(entries) = self.fs.read_dir(dir) else {
            return;
        };
        for entry in entries {
            let Ok(name) = entry.name.into_string() else {
                continue;
            };
            match entry.kind {
                Kind::File if name.ends_with(SUFFIX) => {
                    found.push(slashed(&dir.join(&name)));
                }
                Kind::Dir if !SKIPPED.contains(&name.as_str()) => {
                    self.walk(&dir.join(&name), found);
                }
                _ => {}
            }
        }
    }
}

/// `path` with `/` for separators.
fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

impl PlanFiles for FsPlanFiles {
    fn work_root(&self) -> Result<String, Report> {
        Ok(slashed(&(self.root)()?))
    }

    fn read(&self, path: &str) -> Option<String> {
        self.fs.read_to_string(Path::new(path)).ok()
    }

    fn replace(&self, path: &str, text: &str) -> Result<(), Report> {
        self.fs
            .write_atomic(Path::new(path), text.as_bytes(), Perms::KeepOr(Mode::FILE))
            .map_err(|e| {
                Report::of(&PlanFilesError::Replace {
                    path: path.to_string(),
                    detail: e.to_string(),
                })
            })
    }

    fn create(&self, path: &str, text: &str) -> Result<bool, Report> {
        self.created(Path::new(path), text).map_err(|e| {
            Report::of(&PlanFilesError::Create {
                path: path.to_string(),
                detail: e.to_string(),
            })
        })
    }

    fn scan(&self, root: &str) -> Vec<String> {
        let mut found = Vec::new();
        self.walk(Path::new(root), &mut found);
        found.sort();
        found
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
