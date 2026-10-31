//! The tasks views: answers as a person reads them (text) or a program
//! does (JSON), on presentation's [`Render`]. A task's JSON is
//! tasks-presentation's.

use std::io::{self, Write};

use angzarr_embedded_diagnostic::Diagnostic;
use presentation::{text, Catalog, Envelope, Render};
use serde::Serialize;
use tasks_application::{
    Declared, Details, LintReport, Listing, PermitList, Permitted, Reach, Row, RuleFinding,
    SchemaSummary, Shown, StatusInfo, Statuses, Summary, Tag, Task, Vocabulary,
};
pub(crate) use tasks_presentation::{headline, project_label, CompactJson, RowJson};
use tasks_presentation::{
    FindingJson, PermitsJson, SchemaJson, SummaryJson, TagCountJson, TaskJson,
};

/// The heading of an answer read across every project.
pub fn projects_heading(out: &mut dyn Write, projects: usize) -> io::Result<()> {
    writeln!(out, "Projects: {projects} (--global)\n")
}

/// How wide the status column is: wide enough for every default status.
const STATUS_WIDTH: usize = 11;

/// The tags of `tags` a person is shown, in their canonical spelling.
fn shown(tags: &[Tag], shown: &Shown) -> Vec<String> {
    shown
        .shown(tags)
        .into_iter()
        .map(ToString::to_string)
        .collect()
}

/// The list view: one line per task, blank lines between, the id column as
/// wide as the widest id, columns measured in display width, the tags
/// `shown` shows; `(no tasks)` for none.
pub fn table(out: &mut dyn Write, tasks: &[&Task], shown: &Shown) -> io::Result<()> {
    if tasks.is_empty() {
        return writeln!(out, "(no tasks)");
    }
    let width = tasks
        .iter()
        .map(|t| text::width(&t.harp_id))
        .max()
        .unwrap_or(0);
    for (i, task) in tasks.iter().enumerate() {
        if i > 0 {
            writeln!(out)?;
        }
        let check = if task.checked { "x" } else { " " };
        let mut line = headline(&task.text);
        if !task.trigger.is_empty() {
            line = format!("{line}  (trigger: {})", task.trigger);
        }
        let tags = self::shown(&task.tags, shown);
        if !tags.is_empty() {
            line = format!("{line}  [{}]", tags.join(", "));
        }
        writeln!(
            out,
            "[{check}] {}  {}  {line}",
            text::pad(&task.harp_id, width),
            text::pad(&task.status, STATUS_WIDTH)
        )?;
    }
    Ok(())
}

/// A task `add`, `status` or `edit` wrote: JSON the task; text its id,
/// status and text.
pub struct Added<'a>(pub &'a Task);

/// A task `tag` wrote: JSON the task, every tag; text its id, status and
/// the tags a person is shown.
pub struct Tagged<'a>(pub &'a Task, pub &'a Shown);

/// A listing: JSON an array of tasks, every tag, each a compact row if
/// `compact`, and each with its project where the listing spans projects;
/// text the project, then the table, the tags a person is shown, or across
/// projects a table for each.
pub struct Listed<'a> {
    pub listing: &'a Listing,
    pub compact: bool,
}

/// One row of a listing as JSON: the task whole or compact, alone or with
/// its project.
#[derive(Serialize)]
#[serde(untagged)]
pub enum RowView<'a> {
    Whole(TaskJson<'a>),
    Compact(CompactJson<'a>),
    WholeIn(RowJson<'a, TaskJson<'a>>),
    CompactIn(RowJson<'a, CompactJson<'a>>),
}

/// `listing`'s rows as JSON: each task whole, with its derived priority, or
/// compact if `compact`; with its project if the listing spans projects.
pub fn rows_json(listing: &Listing, compact: bool) -> Vec<RowView<'_>> {
    let attributed = listing.reach.is_every();
    listing
        .ranked()
        .map(|(row, priority)| row_view(row, priority, compact, attributed))
        .collect()
}

fn row_view(row: &Row, priority: Option<f64>, compact: bool, attributed: bool) -> RowView<'_> {
    let whole = || TaskJson::of(&row.task).with_priority(priority);
    match (compact, attributed) {
        (false, false) => RowView::Whole(whole()),
        (true, false) => RowView::Compact(CompactJson::of(&row.task)),
        (false, true) => RowView::WholeIn(RowJson::of(whole(), &row.project)),
        (true, true) => RowView::CompactIn(RowJson::of(CompactJson::of(&row.task), &row.project)),
    }
}

/// `rows` as tables, one for each project in turn, each headed by it.
pub fn tables(out: &mut dyn Write, rows: &[Row], shown: &Shown) -> io::Result<()> {
    if rows.is_empty() {
        return writeln!(out, "(no tasks)");
    }
    for group in rows.chunk_by(|a, b| a.project.id == b.project.id) {
        writeln!(out, "Project: {}", project_label(&group[0].project))?;
        let tasks: Vec<&Task> = group.iter().map(|row| &row.task).collect();
        table(out, &tasks, shown)?;
        writeln!(out)?;
    }
    Ok(())
}

impl Render for Added<'_> {
    fn json(&self) -> impl Serialize + '_ {
        TaskJson::of(self.0)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        let task = self.0;
        writeln!(out, "{}\t{}\t{}", task.harp_id, task.status, task.text)
    }
}

impl Render for Tagged<'_> {
    fn json(&self) -> impl Serialize + '_ {
        TaskJson::of(self.0)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        let task = self.0;
        writeln!(
            out,
            "{}\t{}\t{}",
            task.harp_id,
            task.status,
            shown(&task.tags, self.1).join(",")
        )
    }
}

impl Render for Listed<'_> {
    fn json(&self) -> impl Serialize + '_ {
        rows_json(self.listing, self.compact)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        let listing = self.listing;
        match &listing.reach {
            Reach::Project(project) => writeln!(out, "Project: {}\n", project_label(project))?,
            Reach::NoProject => {}
            Reach::Every { projects, .. } => {
                projects_heading(out, *projects)?;
                return tables(out, &listing.rows, &listing.shown);
            }
        }
        let tasks: Vec<&Task> = listing.tasks().collect();
        table(out, &tasks, &listing.shown)
    }
}

/// The tasks `show` found: JSON one task with its project when one id was
/// asked for, else an array of them (a group is a list, one value an
/// object); text each task whole (the plans governing it among its
/// headers), a blank line between, each headed by its project when read
/// across projects.
pub struct Detailed<'a>(pub &'a Details);

impl Render for Detailed<'_> {
    fn json(&self) -> impl Serialize + '_ {
        let rows = &self.0.rows;
        match rows.as_slice() {
            [one] => OneOrMany::One(attributed(one)),
            _ => OneOrMany::Many(rows.iter().map(attributed).collect()),
        }
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        let every = self.0.reach.is_every();
        for (i, row) in self.0.rows.iter().enumerate() {
            if i > 0 {
                writeln!(out)?;
            }
            if every {
                writeln!(out, "project: {}", project_label(&row.project))?;
            }
            let plans = self.0.governing.get(i).map(Vec::as_slice);
            detail(out, &row.task, plans.unwrap_or_default(), &self.0.shown)?;
        }
        Ok(())
    }
}

/// `row`'s task whole, with its project.
fn attributed(row: &Row) -> RowJson<'_, TaskJson<'_>> {
    RowJson::of(TaskJson::of(&row.task), &row.project)
}

/// One value, or several.
#[derive(Serialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

/// A summary: JSON the counts by status and each task In Progress with its
/// project; text a line per status, then the tasks In Progress, by id
/// (with its project across projects).
pub struct Summed<'a>(pub &'a Summary);

impl Render for Summed<'_> {
    fn json(&self) -> impl Serialize + '_ {
        SummaryJson::of(self.0)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        let summary = self.0;
        if let Reach::Every { projects, .. } = summary.reach {
            projects_heading(out, projects)?;
        }
        for (status, count) in &summary.counts {
            writeln!(out, "{status}\t{count}")?;
        }
        if summary.in_progress.is_empty() {
            return Ok(());
        }
        let every = summary.reach.is_every();
        let names: Vec<String> = summary
            .in_progress
            .iter()
            .map(|row| match every {
                true => format!("{} ({})", row.task.harp_id, project_label(&row.project)),
                false => row.task.harp_id.clone(),
            })
            .collect();
        writeln!(out, "\nIn-progress: {}", names.join(", "))
    }
}

/// The statuses: JSON an array of them; text a line each, its order, its
/// name, and whether it is terminal or requires a trigger.
pub struct Taxonomy<'a>(pub &'a Statuses);

#[derive(Serialize)]
struct StatusJson<'a> {
    name: &'a str,
    order: usize,
    terminal: bool,
    requires_trigger: bool,
}

impl<'a> StatusJson<'a> {
    fn of(status: &'a StatusInfo) -> StatusJson<'a> {
        StatusJson {
            name: &status.name,
            order: status.order,
            terminal: status.terminal,
            requires_trigger: status.requires_trigger,
        }
    }
}

impl Render for Taxonomy<'_> {
    fn json(&self) -> impl Serialize + '_ {
        self.0
            .statuses
            .iter()
            .map(StatusJson::of)
            .collect::<Vec<_>>()
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        for status in &self.0.statuses {
            write!(out, "{}\t{}", status.order, status.name)?;
            if status.terminal {
                write!(out, "\tterminal")?;
            }
            if status.requires_trigger {
                write!(out, "\trequires-trigger")?;
            }
            writeln!(out)?;
        }
        Ok(())
    }
}

/// The tags in use: JSON an array of tag counts; text the project, then a
/// line per tag, its counts aligned.
pub struct Counted<'a>(pub &'a Vocabulary);

impl Render for Counted<'_> {
    fn json(&self) -> impl Serialize + '_ {
        self.0.tags.iter().map(TagCountJson::of).collect::<Vec<_>>()
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        let vocabulary = self.0;
        match &vocabulary.reach {
            Reach::Project(project) => writeln!(out, "Project: {}\n", project_label(project))?,
            Reach::Every { projects, .. } => projects_heading(out, *projects)?,
            Reach::NoProject => {}
        }
        if vocabulary.tags.is_empty() {
            return writeln!(
                out,
                "(no tags in use \u{2014} apply one with `taskloom tag <harp-id> --add <tag>`)"
            );
        }
        let spelled: Vec<String> = vocabulary.tags.iter().map(|c| c.tag.to_string()).collect();
        let width = spelled.iter().map(|t| text::width(t)).max().unwrap_or(0);
        for (tag, count) in spelled.iter().zip(&vocabulary.tags) {
            writeln!(
                out,
                "{}  {:>3} active  {:>3} total",
                text::pad(tag, width),
                count.active,
                count.total
            )?;
        }
        Ok(())
    }
}

/// A lint's report, with the catalog its findings are worded from: JSON
/// the result (each violation's subject and reason, with its project and
/// its finding's envelope), text a line per violation, or what the sweep
/// came to.
pub struct Linted<'a> {
    pub report: &'a LintReport,
    pub catalog: &'a Catalog,
}

#[derive(Serialize)]
struct LintJson<'a> {
    violations: Vec<ViolationJson<'a>>,
    checked_targets: usize,
}

#[derive(Serialize)]
struct ViolationJson<'a> {
    harp_id: &'a str,
    project_id: &'a str,
    reason: String,
    finding: Envelope,
}

impl Render for Linted<'_> {
    fn json(&self) -> impl Serialize + '_ {
        LintJson {
            violations: self
                .report
                .violations
                .iter()
                .map(|v| ViolationJson {
                    harp_id: &v.harp_id,
                    project_id: v.project.as_deref().unwrap_or_default(),
                    reason: self.catalog.message(&v.finding as &dyn Diagnostic),
                    finding: self.catalog.envelope(&v.finding),
                })
                .collect(),
            checked_targets: self.report.checked_targets,
        }
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        let report = self.report;
        if report.checked_targets == 0 {
            writeln!(
                out,
                "0 targets checked: the tag schema declares no enum or range for lint to check"
            )?;
        } else if report.violations.is_empty() {
            writeln!(out, "no tag-standard violations found")?;
        }
        let every = report.reach.is_every();
        for violation in &report.violations {
            let reason = self.catalog.message(&violation.finding as &dyn Diagnostic);
            match (&violation.project, every) {
                (Some(project), true) => {
                    writeln!(out, "{} ({project}): {reason}", violation.harp_id)?
                }
                _ => writeln!(out, "{}: {reason}", violation.harp_id)?,
            }
        }
        Ok(())
    }
}

/// One task whole: its check, id and status; the tags a person is shown
/// and its trigger, if any; then its whole text.
fn detail(out: &mut dyn Write, task: &Task, plans: &[String], shown: &Shown) -> io::Result<()> {
    let check = if task.checked { "x" } else { " " };
    writeln!(out, "[{check}] {}  {}", task.harp_id, task.status)?;
    let tags = self::shown(&task.tags, shown);
    if !tags.is_empty() {
        writeln!(out, "    tags: {}", tags.join(", "))?;
    }
    if !task.trigger.is_empty() {
        writeln!(out, "    trigger: {}", task.trigger)?;
    }
    if !plans.is_empty() {
        writeln!(out, "    plans: {}", plans.join(", "))?;
    }
    writeln!(out)?;
    writeln!(out, "{}", task.text)
}

/// A permit or revoke done: JSON the permit and whether the config
/// changed; text what was done.
pub struct Changed<'a> {
    pub permitted: &'a Permitted,
    /// A revoke, not a permit.
    pub revoked: bool,
}

#[derive(Serialize)]
struct ChangedJson<'a> {
    permit: &'a str,
    changed: bool,
}

impl Render for Changed<'_> {
    fn json(&self) -> impl Serialize + '_ {
        ChangedJson {
            permit: &self.permitted.permit,
            changed: self.permitted.changed,
        }
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        let permit = &self.permitted.permit;
        match (self.revoked, self.permitted.changed) {
            (true, _) => writeln!(out, "revoked {permit}"),
            (false, true) => writeln!(out, "permitted {permit}"),
            (false, false) => writeln!(out, "{permit} was already permitted"),
        }
    }
}

/// What a project permits: JSON tasks-presentation's [`PermitsJson`];
/// text one line a namespace, its keys in a column, then the flat tags and
/// the targets the schema permits.
pub struct Permits<'a>(pub &'a PermitList);

impl Render for Permits<'_> {
    fn json(&self) -> impl Serialize + '_ {
        PermitsJson::of(self.0)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        let doc = PermitsJson::of(self.0);
        if !doc.declared {
            writeln!(
                out,
                "No tag permits: this store declares none, so every tag its rules admit is permitted (`taskloom permit <permit>` starts them)."
            )?;
        } else {
            if doc.namespaces.is_empty() {
                writeln!(out, "Namespaces: (none)")?;
            } else {
                writeln!(out, "Namespaces:")?;
                let width = doc
                    .namespaces
                    .iter()
                    .map(|n| text::width(&n.namespace) + 1)
                    .max()
                    .unwrap_or(0)
                    + 2;
                for namespace in &doc.namespaces {
                    let keys = match namespace.open {
                        true => "(open: any key)".to_string(),
                        false => namespace.keys.join(", "),
                    };
                    let name = format!("{}:", namespace.namespace);
                    writeln!(out, "  {}{keys}", text::pad(&name, width))?;
                }
            }
            writeln!(out, "Flat tags: {}", listed(doc.tags.iter()))?;
        }
        writeln!(
            out,
            "Constrained by the store's rules: {}",
            listed(doc.schema_targets.iter())
        )
    }
}

/// The store's tag rules: JSON tasks-presentation's [`SchemaJson`]; text
/// one line a target, its facets after it in a column, and its formulas
/// indented beneath.
pub struct Schema<'a>(pub &'a SchemaSummary);

impl Render for Schema<'_> {
    fn json(&self) -> impl Serialize + '_ {
        SchemaJson::of(self.0)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        let doc = SchemaJson::of(self.0);
        if doc.targets.is_empty() {
            return writeln!(out, "No tag rules: the store declares no target.");
        }
        let width = doc
            .targets
            .iter()
            .map(|t| text::width(&t.target))
            .max()
            .unwrap_or(0)
            + 2;
        for target in &doc.targets {
            let mut facets = Vec::new();
            if target.scalar {
                facets.push("scalar".to_string());
            }
            if !target.members.is_empty() {
                facets.push(format!("enum: {}", target.members.join(", ")));
            }
            if let Some(range) = target.range {
                let side = |b: Option<f64>| b.map_or("any".to_string(), |b| b.to_string());
                facets.push(format!("range: {} to {}", side(range.min), side(range.max)));
            }
            if !target.kind.is_empty() {
                facets.push(format!("type: {}", target.kind));
            }
            match facets.is_empty() {
                true => writeln!(out, "{}", target.target)?,
                false => writeln!(
                    out,
                    "{}{}",
                    text::pad(&target.target, width),
                    facets.join("; ")
                )?,
            }
            if !target.priority_fn.is_empty() {
                writeln!(out, "    priority_fn: {}", target.priority_fn)?;
            }
            if !target.decay_fn.is_empty() {
                writeln!(out, "    decay_fn: {}", target.decay_fn)?;
            }
        }
        Ok(())
    }
}

/// The store's schema tags, in tagma syntax (`taskloom schema export`):
/// one a line, as tagma reads them back. `export` prints its text in any
/// format; its JSON, an array of them, is for a frontend that asks.
pub struct Exported<'a>(pub &'a [String]);

impl Render for Exported<'_> {
    fn json(&self) -> impl Serialize + '_ {
        self.0
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        self.0.iter().try_for_each(|line| writeln!(out, "{line}"))
    }
}

/// tagma's findings over the store's rules (`taskloom schema check`):
/// JSON tagma's §13 shape, an array; text one a line, its code, its tag
/// and its args, or that there is none.
pub struct Checked<'a>(pub &'a [RuleFinding]);

impl Render for Checked<'_> {
    fn json(&self) -> impl Serialize + '_ {
        FindingJson::all(self.0)
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        if self.0.is_empty() {
            return writeln!(out, "The store's tag rules check clean.");
        }
        for finding in FindingJson::all(self.0) {
            let args: Vec<String> = finding
                .args
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            match args.is_empty() {
                true => writeln!(out, "{}\t{}", finding.code, finding.tag)?,
                false => writeln!(
                    out,
                    "{}\t{}\t{}",
                    finding.code,
                    finding.tag,
                    args.join(", ")
                )?,
            }
        }
        Ok(())
    }
}

/// What a declaration left on the store's schema stream: JSON
/// `{"tags", "changed"}`; text whether it changed, then the tags one a
/// line.
pub struct DeclaredView<'a>(pub &'a Declared);

#[derive(Serialize)]
struct DeclaredJson<'a> {
    tags: &'a [String],
    changed: bool,
}

impl Render for DeclaredView<'_> {
    fn json(&self) -> impl Serialize + '_ {
        DeclaredJson {
            tags: &self.0.tags,
            changed: self.0.changed,
        }
    }

    fn text(&self, out: &mut dyn Write) -> io::Result<()> {
        match self.0.changed {
            true => writeln!(out, "declared: the store's tag rules are now")?,
            false => writeln!(out, "unchanged: the store's tag rules are still")?,
        }
        self.0
            .tags
            .iter()
            .try_for_each(|line| writeln!(out, "  {line}"))
    }
}

/// `items` comma-separated, or `(none)`.
fn listed<'i>(items: impl Iterator<Item = &'i String>) -> String {
    let items: Vec<&str> = items.map(String::as_str).collect();
    match items.is_empty() {
        true => "(none)".to_string(),
        false => items.join(", "),
    }
}

#[cfg(test)]
#[path = "render.test.rs"]
mod tests;
