//! A plan as the use cases answer it: one `*.plan.md` file's metadata, read
//! from its frontmatter, with what is derived across files (`superseded`).

use crate::front::{self, FrontError, Meta};
use crate::rules::{self, plan_name, title_of};

/// The statuses a plan may be set to and a file may hold.
pub const STATUSES: [&str; 4] = ["draft", "active", "done", "abandoned"];

/// The status of a plan another plan supersedes: derived, never written.
pub const SUPERSEDED: &str = "superseded";

/// What a plan with no `status:` is.
pub const DEFAULT_STATUS: &str = "draft";

/// A plan file, read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Plan {
    /// Absolute, `/`-separated.
    pub path: String,
    /// The file's name without `.plan.md`.
    pub name: String,
    /// The `id:` (a harp); none until taskloom first writes the file.
    pub id: Option<String>,
    /// The `title:`, else the body's first heading, else the name.
    pub title: String,
    /// The `title:` as the file writes it; none when it has none.
    pub written_title: Option<String>,
    /// The `status:` as the file writes it; none when it has none.
    pub written_status: Option<String>,
    /// The status it has: `superseded` when another plan supersedes it,
    /// else the written one, else draft.
    pub status: String,
    pub tags: Vec<String>,
    pub governs: Vec<String>,
    pub supersedes: Vec<String>,
    pub sessions: Vec<String>,
    /// The ids of the plans whose `supersedes:` names this one.
    pub superseded_by: Vec<String>,
    /// Why the frontmatter could not be read, when it could not: the
    /// metadata is then what the lenient read finds of title and sessions,
    /// and nothing else.
    pub unreadable: Option<FrontError>,
}

impl Plan {
    /// The plan `text` is, in the file `path`. A frontmatter that cannot
    /// be read does not fail the read: the plan is listed, flagged
    /// [`Plan::unreadable`].
    pub fn parse(path: &str, text: &str) -> Plan {
        let name = plan_name(path);
        let (meta, unreadable) = match front::read(text) {
            Ok(meta) => (meta, None),
            Err(error) => {
                let lenient = rules::frontmatter(text);
                let meta = Meta {
                    title: Some(lenient.title).filter(|t| !t.is_empty()),
                    sessions: lenient.sessions,
                    ..Meta::default()
                };
                (meta, Some(error))
            }
        };
        let status = meta
            .status
            .clone()
            .unwrap_or_else(|| DEFAULT_STATUS.to_string());
        Plan {
            path: path.to_string(),
            title: title_of(meta.title.as_deref(), front::body(text), &name),
            written_title: meta.title,
            name,
            id: meta.id,
            written_status: meta.status,
            status,
            tags: meta.tags,
            governs: meta.governs,
            supersedes: meta.supersedes,
            sessions: meta.sessions,
            superseded_by: Vec::new(),
            unreadable,
        }
    }

    /// The first of the sessions that edited the plan; empty when none
    /// did.
    pub fn session(&self) -> &str {
        self.sessions.first().map_or("", String::as_str)
    }

    /// The metadata the frontmatter holds, as written (no derived status,
    /// no default title): what a change is measured against.
    pub fn meta(&self) -> Meta {
        Meta {
            id: self.id.clone(),
            title: self.written_title.clone(),
            status: self.written_status.clone(),
            tags: self.tags.clone(),
            governs: self.governs.clone(),
            supersedes: self.supersedes.clone(),
            sessions: self.sessions.clone(),
        }
    }
}

/// Which plans supersede which, across `plans`: each plan's
/// [`Plan::superseded_by`] and, where it is superseded, its status.
pub(crate) fn derive(plans: &mut [Plan]) {
    let edges: Vec<(String, String)> = plans
        .iter()
        .filter_map(|p| p.id.clone().map(|id| (id, p)))
        .flat_map(|(id, p)| {
            p.supersedes
                .iter()
                .map(move |target| (target.clone(), id.clone()))
        })
        .collect();
    for plan in plans.iter_mut() {
        let Some(id) = plan.id.as_deref() else {
            continue;
        };
        let mut by: Vec<String> = edges
            .iter()
            .filter(|(target, by)| target == id && by != id)
            .map(|(_, by)| by.clone())
            .collect();
        by.sort();
        by.dedup();
        if !by.is_empty() {
            plan.status = SUPERSEDED.to_string();
        }
        plan.superseded_by = by;
    }
}

#[cfg(test)]
#[path = "plan.test.rs"]
mod tests;
