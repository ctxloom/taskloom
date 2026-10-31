//! What the plans use cases need from outside: the work tree's plan files,
//! the project's tasks as far as plans link them, and fresh plan ids.
//!
//! These futures are not promised `Send`, unlike the other ports': the
//! plans use cases run on the caller's thread (the binary's
//! current-thread runtime), and a test's fake holds its state in a
//! `RefCell`. A server that spawns them would add the bound.

use std::future::Future;

use angzarr_embedded_diagnostic::Report;

/// The work tree's files, as the file-system seam reaches them. A path is
/// work-root-relative and `/`-separated, unless it is absolute (a plan
/// `plan import` finds outside the work root).
///
/// Synchronous, and it blocks on the filesystem, as [`crate::Configuration`]
/// does.
pub trait PlanFiles {
    /// The work root, absolute, `/`-separated; refused where there is none
    /// (no CTXLOOM_ROOT and no git repository above the working directory).
    fn work_root(&self) -> Result<String, Report>;
    /// The file's bytes, or None when it is missing or unreadable.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// Create the file (and its directories) holding `text`: false, writing
    /// nothing, when it already exists.
    fn create(&self, path: &str, text: &[u8]) -> Result<bool, Report>;
    /// The `*.plan.md` files directly under `dir`, as work-root-relative
    /// paths, sorted; none where `dir` cannot be read.
    fn plan_files(&self, dir: &str) -> Vec<String>;
}

impl<T: PlanFiles + ?Sized> PlanFiles for &T {
    fn work_root(&self) -> Result<String, Report> {
        (**self).work_root()
    }
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        (**self).read(path)
    }
    fn create(&self, path: &str, text: &[u8]) -> Result<bool, Report> {
        (**self).create(path, text)
    }
    fn plan_files(&self, dir: &str) -> Vec<String> {
        (**self).plan_files(dir)
    }
}

/// The project's tasks, as far as plans need them: whether one exists (a
/// `governs` link names it), and its tags of one key, to read and remove
/// (`plan import` turns `ctxloom:plan` tags into links).
pub trait Tasks {
    /// Whether the project holds the task `harp_id`.
    fn exists(&mut self, harp_id: &str) -> impl Future<Output = Result<bool, Report>>;
    /// Every task, whatever its status, carrying a tag keyed `key`
    /// (`namespace:key`), with each of its values: (harp id, value).
    fn tagged(&mut self, key: &str) -> impl Future<Output = Result<Vec<(String, String)>, Report>>;
    /// Remove the tag `key="value"` from the task.
    fn untag(
        &mut self,
        harp_id: &str,
        key: &str,
        value: &str,
    ) -> impl Future<Output = Result<(), Report>>;
}

/// Harp ids for new plans, from the same words as tasks': uniqueness is
/// not promised, so a caller checks.
pub trait Mint {
    fn mint(&self) -> String;
}

impl<T: Mint + ?Sized> Mint for &T {
    fn mint(&self) -> String {
        (**self).mint()
    }
}
