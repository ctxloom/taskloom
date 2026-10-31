//! The plan list: a projector's read model, folded from a project's plan
//! streams. It answers both directions of `governs` (the tasks a plan
//! governs, the plans governing a task) and walks `supersedes`.
//!
//! RED STUB (docs/plans/plans-aggregate.md): every body `todo!()`.
//! Implemented, it is an angzarr-embedded projection as the task list is.
//!
//! No order crosses streams (ruling D7): each stream's events arrive in
//! that stream's order, streams in any order, and every answer is sorted by
//! harp id, never by when a plan was added.

// RED STUB: the bodies are `todo!()`, so their arguments are unused.
#![allow(unused_variables)]

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
pub struct PlanList {}

impl PlanList {
    pub fn new() -> PlanList {
        PlanList::default()
    }

    /// Fold `event`, from plan `harp_id`'s stream, in that stream's order.
    pub fn fold(&mut self, harp_id: &str, event: &PlanEvent) {
        todo!("RED: PlanList::fold")
    }

    /// Every plan, by harp id.
    pub fn plans(&self) -> Vec<&PlanRow> {
        todo!("RED: PlanList::plans")
    }

    /// The plans in `status`, by harp id.
    pub fn with_status(&self, status: PlanStatus) -> Vec<&PlanRow> {
        todo!("RED: PlanList::with_status")
    }

    pub fn get(&self, harp_id: &str) -> Option<&PlanRow> {
        todo!("RED: PlanList::get")
    }

    /// The plan whose text is at `path` (as recorded).
    pub fn by_path(&self, path: &str) -> Option<&PlanRow> {
        todo!("RED: PlanList::by_path")
    }

    /// The plans governing task `task`, by harp id: the reverse of a
    /// row's `governs`.
    pub fn governing(&self, task: &str) -> Vec<&PlanRow> {
        todo!("RED: PlanList::governing")
    }

    /// Whether following `supersedes` from `from` reaches `to` (`from`
    /// itself included): a supersede of `to` by `from` closes a cycle
    /// when `to` reaches `from`.
    pub fn supersedes_reaches(&self, from: &str, to: &str) -> bool {
        todo!("RED: PlanList::supersedes_reaches")
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
