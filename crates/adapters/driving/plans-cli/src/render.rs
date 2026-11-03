//! The plan commands' answers as a person reads them (text) or a program
//! does (JSON: one document, for jq). A plan's JSON keeps the fields the VS
//! Code extension reads (ctxloom-vscode `plans-data.ts`): `path`
//! (absolute), `name`, `title`, `session` (the first of `sessions`) and
//! `sessions`; and adds the plan's own.

use std::io::{self, Write};

use angzarr_presentation::{Catalog, Render};
use plans_application::{CheckReport, Plan, PlanText};
use serde::Serialize;

/// A plan as JSON.
#[derive(Serialize, Debug)]
pub struct PlanJson<'a> {
    /// Null until taskloom first writes the file.
    pub id: Option<&'a str>,
    /// Absolute.
    pub path: &'a str,
    /// The file's name without `.plan.md`.
    pub name: &'a str,
    pub title: &'a str,
    pub status: &'a str,
    pub tags: &'a [String],
    pub governs: &'a [String],
    pub supersedes: &'a [String],
    /// The ids of the plans superseding this one.
    pub superseded_by: &'a [String],
    /// The first of `sessions`; empty when there are none.
    pub session: &'a str,
    pub sessions: &'a [String],
}

impl<'a> PlanJson<'a> {
    pub fn of(plan: &'a Plan) -> PlanJson<'a> {
        PlanJson {
            id: plan.id.as_deref(),
            path: &plan.path,
            name: &plan.name,
            title: &plan.title,
            status: &plan.status,
            tags: &plan.tags,
            governs: &plan.governs,
            supersedes: &plan.supersedes,
            superseded_by: &plan.superseded_by,
            session: plan.session(),
            sessions: &plan.sessions,
        }
    }
}

/// A plan's line: its id (a dash where it has none), status, title and
/// path.
fn line(out: &mut dyn Write, plan: &Plan) -> io::Result<()> {
    writeln!(
        out,
        "{} [{}] {} ({})",
        plan.id.as_deref().unwrap_or("-"),
        plan.status,
        plan.title,
        plan.path
    )
}

/// `plans add`'s, `set`'s, `tag`'s, `link`'s and `unlink`'s answer: the
/// plan as it is now.
pub struct Written<'a>(pub &'a Plan);

impl Render for Written<'_> {
    fn json(&self) -> impl Serialize + '_ {
        PlanJson::of(self.0)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        line(out, self.0)
    }
}

/// `plans list`'s answer.
pub struct Listed<'a>(pub &'a [Plan]);

impl Render for Listed<'_> {
    fn json(&self) -> impl Serialize + '_ {
        self.0.iter().map(PlanJson::of).collect::<Vec<_>>()
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        self.0.iter().try_for_each(|plan| line(out, plan))
    }
}

#[derive(Serialize)]
struct ShownJson<'a> {
    #[serde(flatten)]
    plan: PlanJson<'a>,
    content: &'a str,
}

/// `plans show`'s answer: in text the plan's markdown, raw; in JSON the
/// plan and its content.
pub struct Shown<'a>(pub &'a PlanText);

impl Render for Shown<'_> {
    fn json(&self) -> impl Serialize + '_ {
        ShownJson {
            plan: PlanJson::of(&self.0.plan),
            content: &self.0.content,
        }
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        out.write_all(self.0.content.as_bytes())
    }
}

#[derive(Serialize)]
struct CheckJson {
    ok: bool,
    findings: Vec<angzarr_presentation::Envelope>,
}

/// `plans check`'s answer: each finding as a diagnostic, its catalog
/// message in text, its envelope (code, args, message) in JSON.
pub struct Checked<'a> {
    pub report: &'a CheckReport,
    pub catalog: &'a Catalog,
}

impl Render for Checked<'_> {
    fn json(&self) -> impl Serialize + '_ {
        CheckJson {
            ok: !self.report.failed(),
            findings: self
                .report
                .findings
                .iter()
                .map(|finding| self.catalog.envelope(finding))
                .collect(),
        }
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        if self.report.findings.is_empty() {
            return writeln!(out, "plans: no findings");
        }
        self.report
            .findings
            .iter()
            .try_for_each(|finding| writeln!(out, "{}", self.catalog.message(finding)))
    }
}

#[cfg(test)]
#[path = "render.test.rs"]
mod tests;
