//! `taskloom plan`: `add`, `list`, `show`, `status`, `link`, `unlink`,
//! `check` and `import`, parsed into the plans use cases ([`Plans`]) and
//! their answers written as text or JSON.
//!
//! RED STUB (docs/plans/plans-aggregate.md): [`run`] is `todo!()`.
//! Implemented, the group mounts in the binary beside the task commands,
//! takes the shared `Globals` (`--format`, `--json`, `--project`) and
//! renders through presentation and the catalog as tasks-cli does; here it
//! parses its own arguments so the contract reads in one place.
//!
//! The VS Code extension's contract (ctxloom-vscode `plans-data.ts`):
//! `plan list --json` is an array of objects each with `path` (absolute),
//! `name`, `title`, `session` and `sessions`; `plan show <path>` (no
//! `--json`, stdout not a terminal) writes the plan's markdown, raw.

// RED STUB: the body is `todo!()`, so its arguments are unused.
#![allow(unused_variables)]

use std::io::Write;

use clap::{Args, Parser, Subcommand};
pub use plans_application::Plans;

/// The command line `run` parses: `taskloom plan ...`.
#[derive(Parser, Debug)]
#[command(name = "taskloom")]
pub struct Cli {
    #[command(subcommand)]
    pub group: PlanGroup,
    /// Write JSON (the default off a terminal, but for `plan show`).
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand, Debug)]
pub enum PlanGroup {
    /// Plans: markdown files in the work tree, their status and links in
    /// the project's store.
    Plan {
        #[command(subcommand)]
        command: PlanCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum PlanCommand {
    /// Add a plan: write `.taskloom/plans/<id>.plan.md`, or register an
    /// existing `.plan.md` in the work tree with --path.
    Add(AddArgs),
    /// List plans, by id.
    List(ListArgs),
    /// Print a plan's markdown (its record too, with --json).
    Show(ShowArgs),
    /// Set a plan's status (draft, active, done, abandoned; superseded is
    /// set by `plan link --supersedes`).
    Status(StatusArgs),
    /// Link a plan to the tasks it governs, or to a plan it supersedes.
    Link(LinkArgs),
    /// Remove a link `plan link` made.
    Unlink(LinkArgs),
    /// Report drift: a missing file, a hash not the one recorded, an
    /// unregistered plan file, an external plan, a governed task gone.
    /// Exit 1 on any but external.
    Check(CheckArgs),
    /// Turn tasks' ctxloom:plan="<path>" tags into plans and governs
    /// links, retiring the tags migrated; report the paths left.
    Import(ImportArgs),
}

#[derive(Args, Debug)]
pub struct AddArgs {
    #[arg(long)]
    pub title: String,
    /// A `.plan.md` in the work tree to register, relative or absolute.
    #[arg(long)]
    pub path: Option<String>,
    /// draft (the default) or active.
    #[arg(long)]
    pub status: Option<String>,
    /// A task this plan governs (repeatable).
    #[arg(long)]
    pub governs: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Only plans in this status (repeatable).
    #[arg(long)]
    pub status: Vec<String>,
    /// Only plans governing this task.
    #[arg(long)]
    pub governs: Option<String>,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// A plan's harp id, or its path (relative, or absolute as `plan list`
    /// gives it).
    pub plan: String,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    pub plan: String,
    pub status: String,
}

#[derive(Args, Debug)]
pub struct LinkArgs {
    pub plan: String,
    /// A task (repeatable).
    #[arg(long)]
    pub governs: Vec<String>,
    /// A plan.
    #[arg(long, conflicts_with = "governs")]
    pub supersedes: Option<String>,
}

#[derive(Args, Debug)]
pub struct CheckArgs {
    /// Record this plan's file hash as it now is.
    #[arg(long)]
    pub accept: Option<String>,
}

#[derive(Args, Debug)]
pub struct ImportArgs {
    /// Report what would be imported; change nothing.
    #[arg(long)]
    pub dry_run: bool,
}

/// What `run` reads from its process besides its arguments.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Context {
    /// CTXLOOM_SESSION_HARP: the session a plan is added in.
    pub session: String,
    pub stdout_is_terminal: bool,
}

/// Run `args` (`taskloom plan ...`) on `plans`: the answer on `out`,
/// errors on `err`, and the exit code (0, 1 on an error or a failing
/// check, 2 on a usage error).
pub fn run(
    args: &[&str],
    context: &Context,
    plans: &mut dyn Plans,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    todo!("RED: plans_cli::run")
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
