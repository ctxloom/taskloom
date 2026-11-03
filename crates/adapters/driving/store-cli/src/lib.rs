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

use angzarr_diagnostic::Diagnostic;
use angzarr_presentation::Catalog;
use angzarr_store_transfer::angzarr_ports::angzarr_proto::v1::Cover;
use angzarr_store_transfer::{descriptor_pool, write_digest, Digested};
use clap::Subcommand;
use plans_application::DigestPlans;
use project_application::{ProjectAdmin, Projects, StoreExport};
use tasks_application::{DigestService, Folded};

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
    /// line per stream, in the store's order, its cover, head and state as
    /// this build folds it (no state for a domain that takes no snapshot).
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
    let Group::Store(command) = group;
    let done = match command {
        StoreCommand::Export => export(settings, use_cases.projects, out),
        StoreCommand::FoldDigest => digest(settings, use_cases.tasks, use_cases.plans, out).await,
    };
    let said = match done {
        Ok(None) => return 0,
        Ok(Some(note)) => say(settings, catalog, &note, err).map(|()| 0),
        Err(error) => say(settings, catalog, &error, err).map(|()| 1),
    };
    said.unwrap_or(1)
}

/// `diagnostic` on stderr: its catalog message after the program's name.
fn say(
    settings: &Settings<'_>,
    catalog: &Catalog,
    diagnostic: &dyn Diagnostic,
    err: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(err, "{}: {}", settings.prog, catalog.message(diagnostic))
}

/// `store export`: the database through the project use case, the file
/// on `out`; its note, for stderr.
fn export<A: ProjectAdmin, C: StoreExport>(
    settings: &Settings<'_>,
    projects: &mut Projects<A, C>,
    out: &mut dyn Write,
) -> Result<Option<Exported>, CliError> {
    let exported = projects
        .export(settings.project, settings.descriptors, out)
        .map_err(CliError::Project)?;
    Ok(Some(Exported {
        records: exported.records,
        streams: exported.streams,
    }))
}

/// `store fold-digest`: every stream the tasks and the plans fold, in the
/// store's order, their lines on `out`. No note.
async fn digest<T: DigestService, L: DigestPlans>(
    settings: &Settings<'_>,
    tasks: &mut T,
    plans: &mut L,
    out: &mut dyn Write,
) -> Result<Option<Exported>, CliError> {
    let mut folded = tasks
        .fold_digest(settings.project.map(str::to_string))
        .await
        .map_err(CliError::Tasks)?;
    folded.extend(plans.fold_digest().await.map_err(CliError::Plans)?);
    // One order for every build: the store's (by domain, main first, then
    // edition, then root), whichever context folded a stream.
    folded.sort_by(|a, b| stream_order(&a.cover).cmp(&stream_order(&b.cover)));
    let pool = descriptor_pool(settings.descriptors).map_err(CliError::Digest)?;
    let digested: Vec<Digested> = folded.into_iter().map(digested).collect();
    let mut lines = Vec::new();
    write_digest(&digested, &pool, &mut lines).map_err(CliError::Digest)?;
    out.write_all(&lines)
        .and_then(|()| out.flush())
        .map_err(|e| CliError::Output {
            detail: e.to_string(),
        })?;
    Ok(None)
}

/// A stream's place in the store's order: its domain, its edition (main,
/// none, first), its root's bytes.
fn stream_order(cover: &Cover) -> (&str, Option<&str>, &[u8]) {
    let root = cover.root.as_ref().map_or(&[][..], |r| &r.value[..]);
    (&cover.domain, cover.edition.as_deref(), root)
}

/// A fold, as the digest writes it.
fn digested(folded: Folded) -> Digested {
    Digested {
        cover: folded.cover,
        head: folded.head,
        state: folded.state,
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
