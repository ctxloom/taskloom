//! The plan list on angzarr-embedded's projector port: each plan stream's
//! book folded as it is handed, caught up from a store.

use std::sync::{Mutex, MutexGuard, PoisonError};

use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::v1::{EventBook, Projection};
use angzarr_embedded::{CatchUp, Payload, ProjectorHandler};
use plans::{messages, PlanEvent, PLAN_DOMAIN};

use crate::PlanList;

impl PlanList {
    /// An empty list on the projector port, to catch up from a store: its
    /// positions held in memory, so its first step folds every plan stream
    /// from its start, and each later one only what was written since.
    pub fn catch_up() -> CatchUp<PlanLists> {
        CatchUp::new(&[PLAN_DOMAIN], PlanLists::default())
    }

    /// Fold `book`, one plan stream's events after the last projected of
    /// it, as the projector port hands it, each through [`PlanList::fold`]:
    /// a book from sequence 0 starts the stream afresh, and a book with no
    /// event at 0 says the stream is gone; either way, what the list held
    /// of a plan on that stream leaves it first. An event this crate's
    /// plans cannot read is no plan's change.
    pub fn project(&mut self, book: &EventBook) {
        let root = book
            .cover
            .as_ref()
            .and_then(|cover| cover.root.as_ref())
            .map(|r| r.value.to_vec())
            .unwrap_or_default();
        if book
            .pages
            .first()
            .map_or(book.next_sequence, |p| p.sequence)
            == 0
        {
            self.forget(&root);
        }
        for page in &book.pages {
            let payload = page.payload.clone().unwrap_or_default();
            let Ok(event) = messages::PlanEvent::from_any(&payload).map(PlanEvent::from) else {
                continue;
            };
            if let PlanEvent::PlanAdded(add) = &event {
                self.streams.insert(root.clone(), add.harp_id.clone());
            }
            if let Some(harp) = self.streams.get(&root).cloned() {
                self.fold(&harp, &event);
            }
        }
    }
}

/// A [`PlanList`] on angzarr-embedded's projector port: what a [`CatchUp`]
/// hands each plan stream's book.
#[derive(Default)]
pub struct PlanLists(Mutex<PlanList>);

impl PlanLists {
    /// The list as projected so far.
    pub fn list(&self) -> MutexGuard<'_, PlanList> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ProjectorHandler for PlanLists {
    fn name(&self) -> &str {
        "plan-list"
    }

    async fn dispatch(&self, events: &EventBook) -> Result<Projection, CodedError> {
        self.list().project(events);
        Ok(Projection {
            cover: events.cover.clone(),
            projector: self.name().to_string(),
            next_sequence: events.next_sequence,
            projection: None,
        })
    }
}

#[cfg(test)]
#[path = "projection.test.rs"]
mod tests;
