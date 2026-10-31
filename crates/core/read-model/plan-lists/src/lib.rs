//! The plan list: a projector's read model, folded from a project's plan
//! streams. It answers both directions of `governs` (the tasks a plan
//! governs, the plans governing a task) and walks `supersedes`.
//!
//! [`PlanList`] is a [`Projection`] of [`PlanAggregate`], as the task list
//! is of the task aggregate: the kernel hands it each committed event live,
//! and [`PlanList::catch_up`] reads every plan stream the store holds in one
//! read-only unit ([`ReadStore`]), as a process that did not run the
//! command does. [`PlanList::fold`] is the fold both go through.
//!
//! No order crosses streams (ruling D7): each stream's events arrive in
//! that stream's order, streams in any order, and every answer is sorted by
//! harp id, never by when a plan was added. A list kept across catch-ups
//! projects each stream by its own sequence, and keeps no cursor.

mod projection;

use std::collections::{BTreeMap, BTreeSet};

use plans::{PlanEvent, PlanStatus};

/// One plan as the list holds it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PlanRow {
    pub harp_id: String,
    pub title: String,
    pub status: PlanStatus,
    pub path: String,
    pub content_hash: String,
    pub origin_session: String,
    /// Sorted.
    pub governs: Vec<String>,
    /// Sorted.
    pub supersedes: Vec<String>,
    /// Empty unless Superseded.
    pub superseded_by: String,
}

/// Every plan of a project.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PlanList {
    /// Every plan, by harp id.
    rows: BTreeMap<String, PlanRow>,
    /// The reverse of `governs`: each task, the plans governing it.
    governing: BTreeMap<String, BTreeSet<String>>,
    /// The last sequence projected of each plan stream, by harp id.
    pub(crate) projected: BTreeMap<String, u64>,
}

impl PlanList {
    pub fn new() -> PlanList {
        PlanList::default()
    }

    /// Fold `event`, from plan `harp_id`'s stream, in that stream's order.
    /// An event for a plan whose add was not seen (a stream read from
    /// past its start) changes nothing.
    pub fn fold(&mut self, harp_id: &str, event: &PlanEvent) {
        if let PlanEvent::PlanAdded(add) = event {
            self.rows.insert(
                harp_id.to_string(),
                PlanRow {
                    harp_id: harp_id.to_string(),
                    title: add.title.clone(),
                    status: add.status,
                    path: add.path.clone(),
                    content_hash: add.content_hash.clone(),
                    origin_session: add.origin_session.clone(),
                    ..PlanRow::default()
                },
            );
            return;
        }
        let Some(row) = self.rows.get_mut(harp_id) else {
            return;
        };
        match event {
            PlanEvent::PlanAdded(_) => unreachable!("an add is folded above"),
            PlanEvent::StatusChanged { to, .. } => row.status = *to,
            PlanEvent::Governed(task) => {
                insert_sorted(&mut row.governs, task);
                self.governing
                    .entry(task.clone())
                    .or_default()
                    .insert(harp_id.to_string());
            }
            PlanEvent::Ungoverned(task) => {
                row.governs.retain(|t| t != task);
                if let Some(plans) = self.governing.get_mut(task) {
                    plans.remove(harp_id);
                    if plans.is_empty() {
                        self.governing.remove(task);
                    }
                }
            }
            PlanEvent::Superseded(plan) => insert_sorted(&mut row.supersedes, plan),
            PlanEvent::Unsuperseded(plan) => row.supersedes.retain(|p| p != plan),
            PlanEvent::MarkedSuperseded { by, .. } => {
                row.status = PlanStatus::Superseded;
                row.superseded_by = by.clone();
            }
            PlanEvent::Reinstated => {
                row.status = PlanStatus::Active;
                row.superseded_by.clear();
            }
            PlanEvent::Rehashed(hash) => row.content_hash = hash.clone(),
        }
    }

    /// Every plan, by harp id.
    pub fn plans(&self) -> Vec<&PlanRow> {
        self.rows.values().collect()
    }

    /// The plans in `status`, by harp id.
    pub fn with_status(&self, status: PlanStatus) -> Vec<&PlanRow> {
        self.rows.values().filter(|r| r.status == status).collect()
    }

    pub fn get(&self, harp_id: &str) -> Option<&PlanRow> {
        self.rows.get(harp_id)
    }

    /// The plan whose text is at `path` (as recorded).
    pub fn by_path(&self, path: &str) -> Option<&PlanRow> {
        self.rows.values().find(|r| r.path == path)
    }

    /// The plans governing task `task`, by harp id: the reverse of a
    /// row's `governs`.
    pub fn governing(&self, task: &str) -> Vec<&PlanRow> {
        self.governing
            .get(task)
            .into_iter()
            .flatten()
            .filter_map(|plan| self.rows.get(plan))
            .collect()
    }

    /// Whether following `supersedes` from `from` reaches `to` (`from`
    /// itself included): a supersede of `to` by `from` closes a cycle
    /// when `to` reaches `from`.
    pub fn supersedes_reaches(&self, from: &str, to: &str) -> bool {
        let mut seen = BTreeSet::new();
        let mut next = vec![from];
        while let Some(plan) = next.pop() {
            if plan == to {
                return true;
            }
            if !seen.insert(plan) {
                continue;
            }
            if let Some(row) = self.rows.get(plan) {
                next.extend(row.supersedes.iter().map(String::as_str));
            }
        }
        false
    }
}

/// `value` into the sorted `list`, once.
fn insert_sorted(list: &mut Vec<String>, value: &str) {
    if let Err(at) = list.binary_search_by(|held| held.as_str().cmp(value)) {
        list.insert(at, value.to_string());
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
