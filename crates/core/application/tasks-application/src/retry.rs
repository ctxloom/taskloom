//! Running one command through angzarr-embedded's kernel, and running it
//! again when the kernel says a retry may mend it.
//!
//! **One retry loop, taskloom's.** The kernel's own capped retry is on by
//! default; the application turns it off ([`KERNEL_RETRY`]), so every
//! retryable failure (a lost race: a conflict at the write, a stale
//! command; a store that could not say) comes straight back here, and one
//! failure is one attempt: never the kernel's retries times these, and
//! nothing a person's command could not do is dead-lettered behind them.
//! A retry is safe because it re-runs the SAME command id: if the first
//! attempt landed after all, the kernel answers it DUPLICATE. The waits
//! between attempts are capped exponential backoff with full jitter (each
//! wait drawn from zero to its ceiling), over the [`Delay`] and [`Jitter`]
//! ports, so many contending processes spread out instead of colliding
//! again in lockstep.
//!
//! There is no unit of work: a command that depends on the task as it is
//! (a scalar tag displacing the one held, a removal only of a task that
//! still qualifies) is shaped from a read of its stream and sent at the
//! head it read (`Handling.expected_sequence`, STRICT), so a write landing
//! between is the kernel's conflict, and the retry reads and shapes again.

use std::time::Duration;

use angzarr_embedded::next::v1::{CommandBook, Cover, Handling, Uuid};
use angzarr_embedded::next::{
    DeadLetterLog, EventStore, Failure, Kernel, KernelError, OnRetryable, Purge, Recovery,
    Registry, Sampled, Store, StoreError, Stream, StreamRef,
};
use angzarr_embedded_diagnostic::Report;
use ports::{Clock, Delay, Jitter};
use tasks::{TaskAggregate, TaskState};

use crate::AppError;

/// How the application has the kernel retry: not at all. taskloom's loop
/// ([`Backoff`]) is the one that retries, so a failure is never retried
/// twice over (the kernel's three retries of each of taskloom's ten
/// attempts), its waits are the ports' (so a test's are exact), and a
/// person's command that cannot land comes back to them, not to a dead
/// letter. The kernel's own backoff otherwise, unused.
///
/// A command stamped with the head it loaded that loses the race at its
/// write is run once only (`max_race_runs` 1; the kernel takes 0 as 1):
/// the kernel does not re-run it on the new head (angzarr decision 14.46),
/// taskloom's loop is the one that retries.
pub const KERNEL_RETRY: OnRetryable = OnRetryable {
    max_retries: 0,
    max_race_runs: 1,
    initial_backoff: Duration::from_millis(100),
    multiplier: 2.0,
    max_backoff: Duration::from_secs(5),
    jitter: 0.2,
};

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
    /// The domain refused it: its key and args.
    Refused(Report),
}

impl Outcome {
    /// Whether it was refused under `code`.
    pub(crate) fn refused_as(&self, code: &str) -> bool {
        matches!(self, Outcome::Refused(report) if angzarr_embedded_diagnostic::Diagnostic::code(report) == code)
    }
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
    /// Run `book` under `command_id` on `store` until it commits, is
    /// refused, fails in a way a retry will not mend, or the attempts run
    /// out.
    pub async fn run<S: Store>(
        &mut self,
        store: &S,
        book: &CommandBook,
        command_id: &Uuid,
    ) -> Result<Outcome, AppError> {
        let always: Option<&fn(&TaskState) -> bool> = None;
        let outcome = self.run_where(store, book, command_id, always).await?;
        // With no condition `shaped` always sends the book, so nothing is
        // declined: a `None` here is a broken invariant, never a commit.
        Ok(outcome.expect("a command run with no condition is never declined"))
    }

    /// [`Runner::run`], if the task on `book`'s stream, read at each
    /// attempt, meets `holds`: `None` (nothing sent) when it does not. The
    /// command is sent at the head read, so nothing changes the task
    /// between the reading and the write unseen.
    pub async fn run_where<S: Store, P: Fn(&TaskState) -> bool + Sync>(
        &mut self,
        store: &S,
        book: &CommandBook,
        command_id: &Uuid,
        holds: Option<&P>,
    ) -> Result<Option<Outcome>, AppError> {
        let shape = holds.map(|holds| |state: &TaskState| holds(state).then(|| book.clone()));
        self.run_shaped(store, book, command_id, shape.as_ref())
            .await
    }

    /// [`Runner::run`] of what `shape` makes of the task on `book`'s
    /// stream, read at each attempt: `None` (nothing sent) when it makes
    /// nothing; `book` itself, unread, when there is no `shape`. A shaped
    /// book is sent at the head it was shaped from.
    pub async fn run_shaped<S: Store, F: Fn(&TaskState) -> Option<CommandBook> + Sync>(
        &mut self,
        store: &S,
        book: &CommandBook,
        command_id: &Uuid,
        shape: Option<&F>,
    ) -> Result<Option<Outcome>, AppError> {
        let mut failed = 0;
        loop {
            let sampled = Sampled {
                now: self.clock.now(),
                command_id: command_id.clone(),
            };
            let (failure, recovery) = match shaped(self.registry, store, book, shape).await {
                Ok(None) => return Ok(None),
                Ok(Some(book)) => match Kernel::new(self.registry, self.dead_letters)
                    .on_retryable(KERNEL_RETRY)
                    .handle::<TaskAggregate, _, _>(store, book, &sampled, &mut ())
                    .await
                {
                    Ok(_) => return Ok(Some(Outcome::Committed)),
                    Err(Failure::Rejected(rejection)) => {
                        return Ok(Some(Outcome::Refused(rejection.report().clone())))
                    }
                    Err(failure) => (Report::of(&failure), failure.recovery()),
                },
                Err(failed) => failed,
            };
            self.again(&mut failed, failure, recovery).await?;
        }
    }

    /// Purge the stream of the task `stream` names, if it holds a removed
    /// task, read first and purged at the head read (spec 14.43), so a live
    /// task minted on the harp since is never purged: the store refuses the
    /// stale head as a conflict, and the next attempt reads again. How many events it deleted: 0
    /// when the stream holds no removed task.
    pub async fn purge<S: Store + Purge>(
        &mut self,
        store: &S,
        stream: &Cover,
    ) -> Result<u64, AppError> {
        let mut failed = 0;
        loop {
            let (failure, recovery) = match purge_removed(self.registry, store, stream).await {
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

/// A store's failure, as the kernel would report it, with what to do.
fn store_failed(error: StoreError) -> (Report, Recovery) {
    let error = KernelError::Store(error);
    let recovery = error.recovery();
    (Report::of(&error), recovery)
}

/// The task `stream` holds, replayed, and its head; `None` for a stream
/// with no events.
async fn task_at<S: EventStore>(
    registry: &Registry,
    store: &S,
    stream: &Cover,
) -> Result<Option<(TaskState, u64)>, (Report, Recovery)> {
    let at = StreamRef::try_from(stream).map_err(store_failed)?;
    let slice = store.read(at, 0, None).await.map_err(store_failed)?;
    if slice.pages.is_empty() {
        return Ok(None);
    }
    let replayed = Stream::<TaskAggregate>::replay(stream.clone(), &slice.pages, registry)
        .map_err(|e| (Report::of(&e), Recovery::Bug))?;
    Ok(Some((
        replayed.state().state().clone(),
        slice.next_sequence,
    )))
}

/// Purge `stream`, if it holds a removed task.
async fn purge_removed<S: EventStore + Purge>(
    registry: &Registry,
    store: &S,
    stream: &Cover,
) -> Result<u64, (Report, Recovery)> {
    match task_at(registry, store, stream).await? {
        // At the head read: a stream moved since (purged by another
        // prune, a task minted on the freed harp) is the store's conflict,
        // and the retry reads it again.
        Some((task, head)) if task.removed => {
            let at = StreamRef::try_from(stream).map_err(store_failed)?;
            store.purge(at, head).await.map_err(store_failed)
        }
        _ => Ok(0),
    }
}

/// What `shape` makes of the task on `book`'s stream, read from `store`,
/// sent at the head read; `book` itself when there is no `shape`, without
/// reading.
async fn shaped<S: EventStore, F: Fn(&TaskState) -> Option<CommandBook> + Sync>(
    registry: &Registry,
    store: &S,
    book: &CommandBook,
    shape: Option<&F>,
) -> Result<Option<CommandBook>, (Report, Recovery)> {
    let Some(shape) = shape else {
        return Ok(Some(book.clone()));
    };
    let cover = book.cover.clone().unwrap_or_default();
    let (task, head) = match task_at(registry, store, &cover).await {
        Ok(read) => read.unwrap_or_default(),
        // A stream that does not replay is the kernel's to report, as it
        // replays it again.
        Err((_, Recovery::Bug)) => return Ok(Some(book.clone())),
        Err(failed) => return Err(failed),
    };
    Ok(shape(&task).map(|mut shaped| {
        shaped.handling = Some(Handling {
            expected_sequence: Some(head),
            ..Handling::default()
        });
        shaped
    }))
}

#[cfg(test)]
#[path = "retry.test.rs"]
mod tests;
