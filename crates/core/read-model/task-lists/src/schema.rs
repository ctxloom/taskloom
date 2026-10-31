//! RED STUB (red/self-hosted-schema): the store's tag rules as a read
//! model, projected from the schema stream's `SchemaRetagged` events
//! (docs/plans/self-hosted-schema.md). What `taskloom schema export`
//! prints, and what the application hands tagma (`Rules::from_tags`).
//! Nothing here interprets a rule: that is tagma's.

use tasks::schema::SchemaRetagged;
use tasks::Tag;

/// The store's schema tags, as last projected.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SchemaView {
    tags: Vec<Tag>,
    /// How far the schema stream is projected: its own sequence, nothing
    /// store-wide.
    next: u32,
}

impl SchemaView {
    /// An empty view: no stream projected.
    pub fn new() -> SchemaView {
        SchemaView::default()
    }

    /// `event`, at `sequence` of the schema stream, projected: an event at
    /// or below what is projected already changes nothing.
    pub fn project(&mut self, sequence: u32, event: &SchemaRetagged) {
        let _ = (sequence, event, &mut self.tags, self.next);
        todo!("red: project SchemaRetagged")
    }

    /// The schema's tags, as tagma reads them.
    pub fn tags(&self) -> Vec<tagma_core::Tag> {
        todo!("red: the projected tags, in tagma's type")
    }

    /// The schema in tagma syntax, one config tag a line, sorted by its
    /// canonical spelling: `taskloom schema export`'s output.
    pub fn export(&self) -> Vec<String> {
        todo!("red: export in tagma syntax")
    }
}

#[cfg(test)]
#[path = "schema.test.rs"]
mod tests;
