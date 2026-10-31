//! The plan commands' answers as a person reads them (text) or a program
//! does (JSON: one document, for jq). A plan's JSON keeps the fields the VS
//! Code extension reads (ctxloom-vscode `plans-data.ts`): `path`
//! (absolute), `name`, `title`, `session` (the origin session) and
//! `sessions` (the frontmatter's, else `[]`); and adds the plan's own.

use std::io::{self, Write};

use plans_application::{CheckReport, Finding, ImportReport, LeftTag, PlanText, PlanView};
use presentation::Render;
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
                Finding::Unregistered { path } => writeln!(out, "unregistered: {path}"),
            })
    }
}

#[derive(Serialize)]
struct ImportedJson<'a> {
    id: &'a str,
    path: &'a str,
    external: bool,
    new: bool,
    governs: &'a [String],
}

#[derive(Serialize)]
struct RetiredJson<'a> {
    task: &'a str,
    value: &'a str,
}

#[derive(Serialize)]
struct LeftJson<'a> {
    value: &'a str,
    tasks: &'a [String],
}

fn left_json(left: &[LeftTag]) -> Vec<LeftJson<'_>> {
    left.iter()
        .map(|left| LeftJson {
            value: &left.value,
            tasks: &left.tasks,
        })
        .collect()
}

#[derive(Serialize)]
struct ImportJson<'a> {
    dry_run: bool,
    plans: Vec<ImportedJson<'a>>,
    dead: Vec<LeftJson<'a>>,
    not_plans: Vec<LeftJson<'a>>,
    retired: Vec<RetiredJson<'a>>,
}

/// `plan import`'s answer.
pub struct Imported<'a> {
    pub report: &'a ImportReport,
    pub dry_run: bool,
}

/// A value left, with the tasks keeping its tag.
fn left(out: &mut dyn Write, what: &str, left: &LeftTag) -> io::Result<()> {
    writeln!(
        out,
        "{what}: {} ({}): tag kept",
        left.value,
        left.tasks.join(", ")
    )
}

impl Render for Imported<'_> {
    fn json(&self) -> impl Serialize + '_ {
        ImportJson {
            dry_run: self.dry_run,
            plans: self
                .report
                .plans
                .iter()
                .map(|plan| ImportedJson {
                    id: &plan.harp_id,
                    path: &plan.path,
                    external: plan.external,
                    new: plan.new,
                    governs: &plan.governs,
                })
                .collect(),
            dead: left_json(&self.report.dead),
            not_plans: left_json(&self.report.not_plans),
            retired: self
                .report
                .retired
                .iter()
                .map(|(task, value)| RetiredJson { task, value })
                .collect(),
        }
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        for plan in &self.report.plans {
            let how = match (plan.new, plan.external) {
                (true, true) => " (new, copied in)",
                (true, false) => " (new)",
                (false, _) => "",
            };
            writeln!(
                out,
                "{} {}{how}: governs {}",
                plan.harp_id,
                plan.path,
                plan.governs.join(", ")
            )?;
        }
        for dead in &self.report.dead {
            left(out, "dead", dead)?;
        }
        for not_plan in &self.report.not_plans {
            left(out, "not a plan", not_plan)?;
        }
        let count = self.report.retired.len();
        let tags = if count == 1 { "tag" } else { "tags" };
        match self.dry_run {
            false => writeln!(out, "{count} ctxloom:plan {tags} retired"),
            true => writeln!(
                out,
                "{count} ctxloom:plan {tags} would be retired (dry run: nothing changed)"
            ),
        }
    }
}

#[cfg(test)]
#[path = "render.test.rs"]
mod tests;
