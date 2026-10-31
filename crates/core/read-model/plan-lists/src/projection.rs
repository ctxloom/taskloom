//! The plan list as angzarr-embedded projects it: live, from each
//! committed event, and caught up from a store.

use angzarr_embedded::next::v1::Cover;
use angzarr_embedded::next::{
    projection, Committed, KernelError, Payload, Projection, ReadStore, ReadUnit, Registry,
    StoreError, Stream, StreamRef,
};
use plans::{PlanAggregate, PlanEvent, PLAN_DOMAIN};

use crate::PlanList;

impl PlanList {
    /// Project every plan stream `store` holds that is not yet projected,
    /// listed and read in one read-only unit, so what is projected is one
    /// moment of the store.
    pub async fn catch_up<S: ReadStore>(
        &mut self,
        store: &S,
        registry: &Registry,
    ) -> Result<(), Box<KernelError>> {
        let mut reads = store.begin_read().await.map_err(store_error)?;
        let covers = reads.covers(PLAN_DOMAIN).await.map_err(store_error)?;
        let covers: Vec<Cover> = covers
            .into_iter()
            .filter(|cover| cover.edition.is_none())
            .collect();
        for cover in covers {
            let stream = StreamRef::try_from(&cover).map_err(store_error)?;
            let slice = reads.read(stream, 0, None).await.map_err(store_error)?;
            let stream = Stream::<PlanAggregate>::replay(cover, &slice.pages, registry)?;
            stream.dispatch(0, registry, &mut [&mut projection(self)])?;
        }
        Ok(())
    }
}

impl Projection<PlanAggregate> for PlanList {
    fn project(&mut self, committed: &Committed<'_, PlanAggregate>) {
        // Lists show the main timeline; a what-if edition is not the project.
        if committed.cover().edition.is_some() {
            return;
        }
        let harp = committed.state.state().harp_id.clone();
        let sequence = committed.sequence();
        if self
            .projected
            .get(&harp)
            .is_some_and(|last| sequence <= *last)
        {
            return;
        }
        self.projected.insert(harp.clone(), sequence);
        // An event this crate's plans cannot read is no plan's change.
        if let Ok(event) = PlanEvent::from_any(&committed.event().to_any()) {
            self.fold(&harp, &event);
        }
    }
}

fn store_error(error: StoreError) -> Box<KernelError> {
    Box::new(KernelError::Store(error))
}

#[cfg(test)]
#[path = "projection.test.rs"]
mod tests;
