//! The plan commands' answers as a person reads them (text) or a program
//! does (JSON: one document, for jq). A plan's JSON keeps the fields the VS
//! Code extension reads (ctxloom-vscode `plans-data.ts`): `path`
//! (absolute), `name`, `title`, `session` (the origin session) and
//! `sessions` (the frontmatter's, else `[]`); and adds the plan's own.

use std::io::{self, Write};

use angzarr_presentation::Render;
use plans_application::{CheckReport, Finding, PlanText, PlanView};
use serde::Serialize;

/// A plan as JSON.
#[derive(Serialize, Debug)]
pub struct PlanJson<'a> {
    /// Absolute.
    pub path: &'a str,
    pub name: &'a str,
    pub title: &'a str,
    /// The session it was added in.
    pub session: &'a str,
    /// Its file's frontmatter `sessions:`.
    pub sessions: &'a [String],
    pub id: &'a str,
    /// As recorded: work-root-relative.
    pub rel_path: &'a str,
    pub status: &'static str,
    pub content_hash: &'a str,
    pub governs: &'a [String],
    pub supersedes: &'a [String],
    /// Null unless superseded.
    pub superseded_by: Option<&'a str>,
}

impl<'a> PlanJson<'a> {
    pub fn of(plan: &'a PlanView) -> PlanJson<'a> {
        PlanJson {
            path: &plan.abs_path,
            name: &plan.name,
            title: &plan.title,
            session: &plan.origin_session,
            sessions: &plan.sessions,
            id: &plan.harp_id,
            rel_path: &plan.path,
            status: plan.status.name(),
            content_hash: &plan.content_hash,
            governs: &plan.governs,
            supersedes: &plan.supersedes,
            superseded_by: (!plan.superseded_by.is_empty()).then_some(plan.superseded_by.as_str()),
        }
    }
}

/// `plan list`'s line for `plan`.
fn line(out: &mut dyn Write, plan: &PlanView) -> io::Result<()> {
    writeln!(
        out,
        "{} [{}] {} ({})",
        plan.harp_id,
        plan.status.name(),
        plan.title,
        plan.path
    )
}

/// `plan add`'s answer.
pub struct Added<'a>(pub &'a PlanView);

impl Render for Added<'_> {
    fn json(&self) -> impl Serialize + '_ {
        PlanJson::of(self.0)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(out, "added {} {}", self.0.harp_id, self.0.path)
    }
}

/// `plan status`'s answer.
pub struct Moved<'a>(pub &'a PlanView);

impl Render for Moved<'_> {
    fn json(&self) -> impl Serialize + '_ {
        PlanJson::of(self.0)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(out, "{} {}", self.0.harp_id, self.0.status.name())
    }
}

/// `plan link` and `unlink`'s answer: the plan after the last link, and a
/// line for each link made or removed.
pub struct Linked<'a> {
    pub plan: &'a PlanView,
    pub lines: &'a [String],
}

impl Render for Linked<'_> {
    fn json(&self) -> impl Serialize + '_ {
        PlanJson::of(self.plan)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        self.lines.iter().try_for_each(|l| writeln!(out, "{l}"))
    }
}

/// `plan list`'s answer.
pub struct Listed<'a>(pub &'a [PlanView]);

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
    content: Option<&'a str>,
}

/// `plan show`'s answer: in text the plan's markdown, raw; in JSON the plan
/// and its content.
pub struct Shown<'a>(pub &'a PlanText);

impl Render for Shown<'_> {
    fn json(&self) -> impl Serialize + '_ {
        ShownJson {
            plan: PlanJson::of(&self.0.plan),
            content: self.0.content.as_deref(),
        }
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        out.write_all(self.0.content.as_deref().unwrap_or_default().as_bytes())
    }
}

/// A finding as JSON, tagged by its kind.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum FindingJson<'a> {
    Missing { plan: &'a str, path: &'a str },
    MissingTask { plan: &'a str, task: &'a str },
    Unregistered { path: &'a str },
}

#[derive(Serialize)]
struct CheckJson<'a> {
    ok: bool,
    findings: Vec<FindingJson<'a>>,
}

/// `plan check`'s answer.
pub struct Checked<'a>(pub &'a CheckReport);

impl Render for Checked<'_> {
    fn json(&self) -> impl Serialize + '_ {
        CheckJson {
            ok: !self.0.failed(),
            findings: self
                .0
                .findings
                .iter()
                .map(|finding| match finding {
                    Finding::Missing { plan, path } => FindingJson::Missing { plan, path },
                    Finding::MissingTask { plan, task } => FindingJson::MissingTask { plan, task },
                    Finding::Unregistered { path } => FindingJson::Unregistered { path },
                })
                .collect(),
        }
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        if self.0.findings.is_empty() {
            return writeln!(out, "plans: no drift");
        }
        self.0
            .findings
            .iter()
            .try_for_each(|finding| match finding {
                Finding::Missing { plan, path } => writeln!(out, "missing: {plan} {path}"),
                Finding::MissingTask { plan, task } => writeln!(out, "missing task: {plan} {task}"),
                Finding::Unregistered { path } => writeln!(out, "unregistered: {path}"),
            })
    }
}

#[cfg(test)]
#[path = "render.test.rs"]
mod tests;
