//! The re-fold digest of a project's store (store-export-import.plan.md,
//! section 5.3.2): every stream of the tasks' domains, the task
//! aggregate's and the schema stream's, folded as the kernel loads it, its
//! head and its state. The build before a proto generation's crossing and
//! the build after each fold their own store, and the two digests must
//! agree: what a crossing has to keep.

use std::future::Future;

use angzarr_embedded::Registry;
use ports::{ProjectIdentity, TaskStores};

use crate::AppError;

/// One stream folded: its cover, its head, its state packed in its
/// snapshot form (angzarr-embedded's).
pub use angzarr_embedded::Folded;

/// The digest, as a driving adapter calls it. The future is `Send`, as
/// [`crate::TaskService`]'s are.
pub trait DigestService {
    /// Every task stream of the store of the project `project` pins (else
    /// the work root's), then its schema stream, each in the store's order
    /// and folded as the kernel loads it. Empty where the project has no
    /// store yet; refused ([`AppError::NoProject`]) where there is no
    /// project. Resolving never mints, and nothing is written.
    fn fold_digest(
        &mut self,
        project: Option<String>,
    ) -> impl Future<Output = Result<Vec<Folded>, AppError>> + Send;
}

/// [`DigestService::fold_digest`], over the project's resolution and its
/// store, under `registry`.
pub(crate) async fn fold<I: ProjectIdentity, S: TaskStores>(
    identity: &mut I,
    stores: &mut S,
    registry: &Registry,
    pinned: Option<&str>,
) -> Result<Vec<Folded>, AppError> {
    let _ = (identity, stores, registry, pinned);
    todo!("the tasks' digest")
}

#[cfg(test)]
#[path = "digest.test.rs"]
mod tests;
