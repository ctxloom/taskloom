//! Running one command through annals' kernel, and running it again when
//! annals says to.
//!
//! annals never retries; it answers [`Recovery::Retry`] for a store
//! conflict, a stale command or a store that could not say, and a retry is
//! safe because it re-runs the SAME command id in a fresh unit of work: if
//! the first attempt landed after all, the retry is answered DUPLICATE. The
//! waits between attempts are capped exponential backoff with full jitter
//! (each wait drawn from zero to its ceiling), so many contending processes
//! spread out instead of colliding again in lockstep.

use std::time::Duration;

use annals::next::v1::{CommandBook, Cover, Uuid};
use annals::next::{
    AsyncEventStore, AsyncPurge, AsyncStore, AsyncUnitOfWork, DeadLetterLog, Failure, Kernel,
    Recovery, Registry, Routes, Sampled, StoreError, Stream,
};
use annals_diagnostic::Report;
use ports::{Clock, Delay, Jitter, PurgingStore};
use tasks::{TaskRejection, TaskState, Tasks};

use crate::AppError;

/// How long to wait between attempts, and how many to make.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Backoff {
    /// The ceiling of the first wait; each later one doubles it.
    pub base: Duration,
    /// No wait's ceiling exceeds this.
    pub cap: Duration,
    /// Attempts in all, the first included.
    pub attempts: u32,
}

impl Backoff {
    /// 10ms doubling to a 1s cap, 10 attempts: the waits' ceilings sum to
    /// about 3.3s, on top of the store's own lock wait per attempt.
    pub const DEFAULT: Backoff = Backoff {
        base: Duration::from_millis(10),
        cap: Duration::from_secs(1),
        attempts: 10,
    };

    /// The ceiling of the wait after the `failed`th failed attempt (from
    /// 1): `base * 2^(failed - 1)`, at most `cap`.
    pub fn ceiling(&self, failed: u32) -> Duration {
        let doubling = 1u32
            .checked_shl(failed.saturating_sub(1))
            .unwrap_or(u32::MAX);
        self.base.saturating_mul(doubling).min(self.cap)
    }
}

/// What one command came to, short of an error.
pub(crate) enum Outcome {
    /// It committed, or had already (a retry answered DUPLICATE): either
    /// way the command's events are on the stream once.
    Committed,
    /// The domain refused it.
    Refused(TaskRejection),
}

/// What [`Runner::run`] needs besides the store and the command.
pub(crate) struct Runner<'a, C, D> {
    pub registry: &'a Registry,
    pub clock: &'a mut C,
    pub delay: &'a mut D,
    pub backoff: Backoff,
    /// Where the kernel writes what it dead-letters: the use case's log,
    /// whose lines it returns with its result.
    pub dead_letters: &'a dyn DeadLetterLog,
}

impl<C: Clock, D: Delay + Jitter> Runner<'_, C, D> {
    /// Run `book` under `command_id` on `store`, each attempt in a
    /// fresh unit of work, until it commits, is refused, fails in a way a
    /// retry will not mend, or the attempts run out.
    pub async fn run<S: AsyncStore>(
        &mut self,
        store: &mut S,
        book: &CommandBook,
        command_id: &Uuid,
    ) -> Result<Outcome, AppError> {
        let always: Option<&fn(&TaskState) -> bool> = None;
        let outcome = self.run_where(store, book, command_id, always).await?;
        // With no condition `shaped` always sends the book, so nothing is
        // declined: a `None` here is a broken invariant, never a commit.
        Ok(outcome.expect("a command run with no condition is never declined"))
    }

    /// [`Runner::run`], if the task on `book`'s stream, read in each
    /// attempt's own unit of work before the command is decided there,
    /// meets `holds`: `None` (nothing sent) when it does not. A unit of
    /// work holds the store's write lock from its first read, so nothing
    /// changes the task between the reading and the commit.
    pub async fn run_where<S: AsyncStore, P: Fn(&TaskState) -> bool + Sync>(
        &mut self,
        store: &mut S,
        book: &CommandBook,
        command_id: &Uuid,
        holds: Option<&P>,
    ) -> Result<Option<Outcome>, AppError> {
        let shape = holds.map(|holds| |state: &TaskState| holds(state).then(|| book.clone()));
        self.run_shaped(store, book, command_id, shape.as_ref())
            .await
    }

    /// [`Runner::run`] of what `shape` makes of the task on `book`'s
    /// stream, read in each attempt's own unit of work as
    /// [`Runner::run_where`] reads it: `None` (nothing sent) when it makes
    /// nothing; `book` itself, unread, when there is no `shape`. A command
    /// that depends on the task as it is (a scalar value displacing the
    /// one it holds) is shaped here, so nothing written between an earlier
    /// read and this unit is missed.
    pub async fn run_shaped<S: AsyncStore, F: Fn(&TaskState) -> Option<CommandBook> + Sync>(
        &mut self,
        store: &mut S,
        book: &CommandBook,
        command_id: &Uuid,
        shape: Option<&F>,
    ) -> Result<Option<Outcome>, AppError> {
        let kernel = Kernel::new(self.registry, self.dead_letters);
        let routes = Routes::new();
        let mut failed = 0;
        loop {
            let sampled = Sampled {
                now: self.clock.now(),
                command_id: command_id.clone(),
            };
            let mut unit = store.begin().await;
            let (failure, recovery) = match shaped(self.registry, &mut unit, book, shape).await {
                Ok(None) => return Ok(None),
                Ok(Some(book)) => match kernel
                    .handle_async::<Tasks, _, _>(unit, &routes, book, &sampled, &mut ())
                    .await
                {
                    Ok(_) => return Ok(Some(Outcome::Committed)),
                    Err(Failure::Rejected(rejection)) => {
                        return Ok(Some(Outcome::Refused(rejection)))
                    }
                    Err(failure) => (Report::of(&failure), failure.recovery()),
                },
                Err(error) => (Report::of(&error), error.recovery()),
            };
            self.again(&mut failed, failure, recovery).await?;
        }
    }

    /// Purge the stream of the task `harp_id` names, if it holds a removed
    /// task: in a unit of work of its own, which reads the stream first,
    /// so a task minted on the harp since is never purged. Each attempt
    /// is a fresh unit, as [`Runner::run`]'s are. How many events it
    /// deleted: 0 when the stream holds no removed task.
    pub async fn purge<S: PurgingStore>(
        &mut self,
        store: &mut S,
        stream: &Cover,
    ) -> Result<u64, AppError> {
        let mut failed = 0;
        loop {
            let unit = store.begin_purging().await;
            let (failure, recovery) = match purge_removed(self.registry, unit, stream).await {
                Ok(deleted) => return Ok(deleted),
                Err(failed) => failed,
            };
            self.again(&mut failed, failure, recovery).await?;
        }
    }

    /// After the `failed`th failure, wait to try again; or the error to
    /// end with, when a retry will not mend it or the attempts are spent.
    async fn again(
        &mut self,
        failed: &mut u32,
        failure: Report,
        recovery: Recovery,
    ) -> Result<(), AppError> {
        if recovery != Recovery::Retry {
            return Err(AppError::Failed(failure));
        }
        *failed += 1;
        if *failed >= self.backoff.attempts {
            return Err(AppError::GaveUp {
                attempts: *failed,
                error: failure,
            });
        }
        let ceiling = self.backoff.ceiling(*failed);
        let wait = self.delay.up_to(ceiling).min(ceiling);
        self.delay.sleep(wait).await;
        Ok(())
    }
}

/// Purge `stream` through `unit` and commit, if it holds a removed task.
async fn purge_removed<U: AsyncUnitOfWork + AsyncPurge>(
    registry: &Registry,
    mut unit: U,
    stream: &Cover,
) -> Result<u64, (Report, Recovery)> {
    let store = |e: StoreError| (Report::of(&e), e.recovery());
    let Some(slice) = unit.read_from(stream, 0).await.map_err(store)? else {
        return Ok(0);
    };
    let replayed = Stream::<Tasks>::replay(stream.clone(), &slice.pages, registry)
        .map_err(|e| (Report::of(&e), Recovery::Bug))?;
    if !replayed.state().removed {
        return Ok(0);
    }
    let deleted = unit.purge(stream).await.map_err(store)?;
    unit.commit().await.map_err(store)?;
    Ok(deleted)
}

/// What `shape` makes of the task on `book`'s stream, read through
/// `unit`; `book` itself when there is no `shape`, without reading.
async fn shaped<U: AsyncEventStore, F: Fn(&TaskState) -> Option<CommandBook> + Sync>(
    registry: &Registry,
    unit: &mut U,
    book: &CommandBook,
    shape: Option<&F>,
) -> Result<Option<CommandBook>, StoreError> {
    let Some(shape) = shape else {
        return Ok(Some(book.clone()));
    };
    let cover = book.cover.clone().unwrap_or_default();
    let pages = unit
        .read_from(&cover, 0)
        .await?
        .map(|slice| slice.pages)
        .unwrap_or_default();
    // A stream that does not replay is the kernel's to report, as it
    // replays it again.
    let Ok(stream) = Stream::<Tasks>::replay(cover, &pages, registry) else {
        return Ok(Some(book.clone()));
    };
    Ok(shape(stream.state()))
}

#[cfg(test)]
#[path = "retry.test.rs"]
mod tests;
