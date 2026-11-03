//! The `store` command group: `export` and `fold-digest`
//! (store-export-import.plan.md, D7 (a)), translated into the project use
//! cases' export and each context's digest, their lines written to
//! stdout. They carry a project's database to a build of another proto
//! generation: the build before the crossing exports its own store, and
//! each build's digest of its own store must agree after.
//!
//! A binary flattens [`Group`] into its own subcommands, and hands the
//! group, the pinned project, the app's descriptor sets, its catalog (with
//! this crate's [`EN_US`] and angzarr-store-transfer's) and the use cases
//! to [`run`]. Exit 0, or 1 with the error on stderr (its catalog message
//! after the program's name). Stdout holds the JSONL alone: `export`'s
//! closing note is on stderr.

mod error;

use std::io::Write;

use angzarr_presentation::Catalog;
use clap::Subcommand;
use plans_application::DigestPlans;
use project_application::{ProjectAdmin, Projects, StoreExport};
use tasks_application::DigestService;

#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{CliError, Exported, EN_US};

/// The group, mounted as `store`.
#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Group {
    /// Carry this project's database across a proto generation: export
    /// it, and digest its streams.
    #[command(subcommand)]
    Store(StoreCommand),
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum StoreCommand {
    /// Write this project's database whole to stdout as JSONL, every row
    /// in proto3's JSON mapping. Run it on a copy of the store (VACUUM
    /// INTO): it opens the database read-only and changes nothing, and
    /// refuses one whose migrations are not exactly this build's, and one
    /// with outbox entries still to deliver.
    Export,
    /// Write the digest of this project's database to stdout: one JSONL
    /// line per stream, its cover, head and state as this build folds it.
    /// Two builds' digests of one store's crossing must be the same bytes.
    FoldDigest,
}

/// What the mounting binary gives the group.
#[derive(Clone, Copy, Debug)]
pub struct Settings<'a> {
    /// `--project`: the project to act on, overriding the directory's.
    pub project: Option<&'a str>,
    /// The app's encoded `FileDescriptorSet`s, which its payloads and
    /// states are expanded through (angzarr's own are always taken).
    pub descriptors: &'a [&'a [u8]],
    /// The program's name: every diagnostic's prefix.
    pub prog: &'a str,
}

/// The use cases the group runs: the project use cases (for the export),
/// and each context's digest.
pub struct UseCases<'u, A, C, T, L> {
    pub projects: &'u mut Projects<A, C>,
    pub tasks: &'u mut T,
    pub plans: &'u mut L,
}

/// Run `group` over `use_cases`, writing to `out` and `err` with messages
/// from `catalog`; the exit code.
pub async fn run<A, C, T, L>(
    group: Group,
    settings: &Settings<'_>,
    catalog: &Catalog,
    use_cases: UseCases<'_, A, C, T, L>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8
where
    A: ProjectAdmin,
    C: StoreExport,
    T: DigestService,
    L: DigestPlans,
{
    let _ = (group, settings, catalog, use_cases, out, err);
    todo!("the store group")
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
