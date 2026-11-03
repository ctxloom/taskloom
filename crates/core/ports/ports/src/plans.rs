//! What the plans use cases need from outside: the plan files, the
//! project's tasks as far as plans name them, the store's tag rules, and
//! fresh plan ids.
//!
//! These futures are not promised `Send`, unlike the other ports': the
//! plans use cases run on the caller's thread (the binary's
//! current-thread runtime), and a test's fake holds its state in a
//! `RefCell`. A server that spawns them would add the bound.

use std::future::Future;

use angzarr_diagnostic::Report;

/// The plan files, as the file-system seam reaches them. Every path is
/// absolute and `/`-separated; the use cases place a relative root against
/// the work root themselves.
///
/// Synchronous, and it blocks on the filesystem, as [`crate::Configuration`]
/// does.
pub trait PlanFiles {
    /// The work root, absolute, `/`-separated; refused where there is none
    /// (no CTXLOOM_ROOT and no git repository above the working directory).
    fn work_root(&self) -> Result<String, Report>;
    /// The file's text, or None when it is missing, unreadable or not
    /// UTF-8.
    fn read(&self, path: &str) -> Option<String>;
    /// Replace the file's text, whole and atomically, keeping its
    /// permissions.
    fn replace(&self, path: &str, text: &str) -> Result<(), Report>;
    /// Create the file (and its directories) holding `text`: false, writing
    /// nothing, when it already exists.
    fn create(&self, path: &str, text: &str) -> Result<bool, Report>;
    /// The `*.plan.md` files under `root` and its directories (not `.git`,
    /// `node_modules` or `target`, and no symbolic link followed), sorted;
    /// none where `root` cannot be read.
    fn scan(&self, root: &str) -> Vec<String>;
}

impl<T: PlanFiles + ?Sized> PlanFiles for &T {
    fn work_root(&self) -> Result<String, Report> {
        (**self).work_root()
    }
    fn read(&self, path: &str) -> Option<String> {
        (**self).read(path)
    }
    fn replace(&self, path: &str, text: &str) -> Result<(), Report> {
        (**self).replace(path, text)
    }
    fn create(&self, path: &str, text: &str) -> Result<bool, Report> {
        (**self).create(path, text)
    }
    fn scan(&self, root: &str) -> Vec<String> {
        (**self).scan(root)
    }
}

/// The project's tasks and rules, as far as plans need them: whether a
/// task exists (a `governs` link names it), and the store's tag rules (a
/// plan's tags are held to them as a task's are). Plans keep no record in
/// the store; they only read it.
pub trait Tasks {
    /// Whether the project holds the task `harp_id`.
    fn exists(&mut self, harp_id: &str) -> impl Future<Output = Result<bool, Report>>;
    /// The store's tag rules in tagma's grammar, one tag a line, sorted:
    /// the seed's where the project has no store yet.
    fn rules(&mut self) -> impl Future<Output = Result<Vec<String>, Report>>;
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
