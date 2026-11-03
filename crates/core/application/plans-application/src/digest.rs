//! The re-fold digest of a project's plan streams
//! (store-export-import.plan.md, section 5.3.2): every stream of the plans'
//! domain folded as the kernel loads it, its head and its state, for the
//! builds on either side of a proto generation's crossing to compare.

use std::future::Future;

use ports::{ProjectIdentity, TaskStores};

use crate::{PlanError, PlanService};

/// One stream folded: its cover, its head, its state packed in its
/// snapshot form (angzarr-embedded's).
pub use angzarr_embedded::Folded;

/// The digest, as a driving adapter calls it.
pub trait DigestPlans {
    /// Every plan stream of the store of the project the service acts on
    /// (its pinned one, else the work root's), in the store's order,
    /// folded as the kernel loads it. Empty where there is no project or
    /// no store: no plan. Nothing is minted or written.
    fn fold_digest(&mut self) -> impl Future<Output = Result<Vec<Folded>, PlanError>>;
}

impl<I, S, F, T, M, C, D> DigestPlans for PlanService<I, S, F, T, M, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
{
    async fn fold_digest(&mut self) -> Result<Vec<Folded>, PlanError> {
        todo!("the plans' digest")
    }
}

#[cfg(test)]
#[path = "digest.test.rs"]
mod tests;
