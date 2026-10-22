//! `import`: a command group of its own, which a binary mounts beside the
//! task commands ([`crate::run_mounted`]) and runs on an importer it builds
//! for `import` alone ([`run_import`]), so no other command reaches the
//! JSONL task logs; and its answer: what each project's log came to, and
//! everything that could not be brought over, as text or JSON, its
//! diagnostics rendered from the catalog.

use std::io::{self, Write};

use annals_diagnostic::Diagnostic;
use presentation::{Catalog, Envelope, Format, Render};
use serde::Serialize;
use tasks_application::{ImportReport, ImportRequest, ImportService, Lost, ProjectImport};

use crate::{finish, output_failed, render, CliError, Mounted};

/// `import`, mounted beside the task commands.
#[derive(clap::Subcommand, Clone, PartialEq, Eq, Debug)]
pub enum ImportGroup {
    /// Import Go taskloom's task logs (~/.ctxloom/tasks/<project-id>.jsonl)
    /// into each project's store: every project's, or --project's alone.
    /// Running it again imports only what is new.
    Import,
}

/// `import` on `importer`, with the global flags parsed: its report on
/// `out`, and the exit code, 1 when the import could not start or left
/// something behind.
pub async fn run_import<S: ImportService>(
    flags: &Mounted,
    catalog: &Catalog,
    importer: &mut S,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let request = ImportRequest {
        project: flags.project.clone(),
    };
    let result = match importer.import(request).await {
        Ok(report) => imported(&report, flags.format, catalog, out),
        Err(e) => Err(CliError::App(e)),
    };
    finish(result, flags.format, catalog, err)
}

/// Render the report; refuse when it left something behind, so a script
/// sees it in the exit code.
fn imported(
    report: &ImportReport,
    format: Format,
    catalog: &Catalog,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    presentation::render(out, format, &Imported { report, catalog }).map_err(output_failed)?;
    if report.complete() {
        return Ok(());
    }
    let stopped = report
        .projects
        .iter()
        .filter(|p| p.failed.is_some())
        .count();
    let lost = report.projects.iter().map(|p| p.lost.len()).sum();
    Err(CliError::Incomplete { stopped, lost })
}

/// An import's report, with the catalog its diagnostics are rendered from:
/// JSON the whole report; text a line per project.
pub struct Imported<'a> {
    pub report: &'a ImportReport,
    pub catalog: &'a Catalog,
}

impl Render for Imported<'_> {
    fn json(&self) -> impl Serialize + '_ {
        ImportJson::of(self.report, self.catalog)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        text(out, self.report, self.catalog)
    }
}

/// The report as JSON.
#[derive(Serialize)]
pub struct ImportJson<'a> {
    /// Whether everything was brought over.
    complete: bool,
    projects: Vec<ProjectJson<'a>>,
}

#[derive(Serialize)]
struct ProjectJson<'a> {
    project_id: &'a str,
    failed: Option<Envelope>,
    records: usize,
    repeated: usize,
    tasks: usize,
    applied: usize,
    already: usize,
    unchanged: usize,
    ignored: usize,
    repaired: usize,
    lost: Vec<LostJson<'a>>,
}

#[derive(Serialize)]
struct LostJson<'a> {
    line: u32,
    op: &'static str,
    harp_id: &'a str,
    text: &'a str,
    why: Envelope,
}

impl<'a> ImportJson<'a> {
    /// `report` as JSON, each diagnostic an envelope from `catalog`.
    pub fn of(report: &'a ImportReport, catalog: &Catalog) -> ImportJson<'a> {
        ImportJson {
            complete: report.complete(),
            projects: report
                .projects
                .iter()
                .map(|p| project_json(p, catalog))
                .collect(),
        }
    }
}

fn project_json<'a>(project: &'a ProjectImport, catalog: &Catalog) -> ProjectJson<'a> {
    ProjectJson {
        project_id: &project.project_id,
        failed: project
            .failed
            .as_ref()
            .map(|r| catalog.envelope(r as &dyn Diagnostic)),
        records: project.records,
        repeated: project.repeated,
        tasks: project.tasks,
        applied: project.applied,
        already: project.already,
        unchanged: project.unchanged,
        ignored: project.ignored,
        repaired: project.repaired,
        lost: project
            .lost
            .iter()
            .map(|lost| LostJson {
                line: lost.line,
                op: lost.op.as_str(),
                harp_id: &lost.harp_id,
                text: &lost.text,
                why: catalog.envelope(&lost.why),
            })
            .collect(),
    }
}

/// The report as text: a line per project, then a line per record left
/// behind and the reason the project stopped, if it did, each reason its
/// message from `catalog`.
pub fn text(out: &mut dyn Write, report: &ImportReport, catalog: &Catalog) -> io::Result<()> {
    for project in &report.projects {
        writeln!(out, "{}: {}", project.project_id, summary(project))?;
        for lost in &project.lost {
            writeln!(out, "  not imported: {}", lost_line(lost, catalog))?;
        }
        if let Some(failed) = &project.failed {
            writeln!(out, "  stopped: {}", catalog.message(failed))?;
        }
    }
    Ok(())
}

/// What a project's import counted, the counts that are zero left out
/// after the first three.
fn summary(project: &ProjectImport) -> String {
    let mut parts = vec![
        format!("{} records", project.records),
        format!("{} tasks added", project.tasks),
        format!("{} records imported", project.applied),
    ];
    let optional = [
        (project.already, "already imported"),
        (project.repeated, "repeated lines skipped"),
        (project.unchanged, "retags changing nothing"),
        (project.ignored, "to no live task, as Go ignores them"),
        (project.repaired, "displaced adds Go's repair re-added"),
        (project.lost.len(), "not imported"),
    ];
    parts.extend(
        optional
            .iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, what)| format!("{count} {what}")),
    );
    parts.join(", ")
}

fn lost_line(lost: &Lost, catalog: &Catalog) -> String {
    let mut line = format!("line {}, {} {}", lost.line, lost.op.as_str(), lost.harp_id);
    if !lost.text.is_empty() {
        line.push_str(&format!(" {:?}", render::headline(&lost.text)));
    }
    line.push_str(&format!(": {}", catalog.message(&lost.why)));
    line
}

#[cfg(test)]
#[path = "import.test.rs"]
mod tests;
