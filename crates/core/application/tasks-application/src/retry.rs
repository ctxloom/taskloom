//! Running one command through angzarr-embedded's kernel, which retries it
//! (angzarr's owner ruling, 2026-10-31: the kernel owns race re-runs).
//!
//! **The kernel retries what it runs.** A command that loses the race at
//! its write is re-run on the new head, and a store that could not say is
//! tried again, both by the kernel, under taskloom's [`Backoff`]
//! ([`Backoff::on_retryable`]; [`KERNEL_RETRY`] the default), its waits
//! over taskloom's [`Delay`] and [`Jitter`] ports ([`Ported`]), so a test's
//! are exact. A retry is safe because it re-runs the SAME command id: if
//! the first attempt landed after all, the kernel answers it DUPLICATE. The
//! waits are capped exponential backoff with full jitter (each wait drawn
//! from zero to its ceiling), so many contending processes spread out
//! instead of colliding again in lockstep. A retryable failure the kernel
//! hands back has had its retries: taskloom gives up on it at once
//! ([`AppError::GaveUp`]), never retrying it again.
//!
//! **taskloom re-reads what it shaped.** A command that depends on the task
//! as it is (a scalar tag displacing the one held, a removal only of a task
//! that still qualifies) is shaped from a read of its stream and sent at
//! the head it read (`Handling.expected_sequence`, STRICT), so a write
//! landing between is the kernel's stale answer (ABORTED: re-read and
//! decide again, the caller's, angzarr decision 14.37), and taskloom reads
//! and shapes again. So it does for what the kernel never runs: a read of
//! the stream, and a purge at the head read.

use std::time::Duration;

use angzarr_embedded::next::v1::{CommandBook, Cover, Handling, Uuid};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};

use angzarr_embedded::next::{
    DeadLetterLog, Domain, EventStore, Failure, Kernel, KernelError, OnRetryable, Purge, Recovery,
    Registry, Rejected, Sampled, Store, StoreError, Stream, StreamRef, Wait,
};
use angzarr_embedded_diagnostic::Report;
use ports::{Clock, Delay, Jitter};
use tasks::{TaskAggregate, TaskState};

use crate::AppError;

/// How the application has the kernel retry: [`Backoff::DEFAULT`], as
/// [`Backoff::on_retryable`] makes it. A command stamped with the head it
/// loaded that loses the race at its write is re-run on the new head up to
/// the kernel's default race re-run cap, 10 runs (angzarr decision 14.46),
/// and a store that could not say is tried up to 10 times; at either cap
/// the kernel dead-letters it, into the use case's log.
pub const KERNEL_RETRY: OnRetryable = Backoff::DEFAULT.on_retryable();

/// The kernel's [`Wait`] over taskloom's [`Delay`] and [`Jitter`] ports:
/// each backoff waited out by [`Delay::sleep`], each jitter drawn by
/// [`Jitter::up_to`] as a fraction of [`DRAW_SCALE`], so a test's waits and
/// draws are its fakes'.
pub struct Ported<'d, D>(Mutex<Option<&'d mut D>>);

/// What [`Ported`] asks [`Jitter::up_to`] for: a draw is the fraction of it
/// drawn, at most all of it. A power of two seconds, so a fake's even
/// fraction of it divides back exactly.
pub const DRAW_SCALE: Duration = Duration::from_secs(1024);

impl<'d, D> Ported<'d, D> {
    pub fn new(delay: &'d mut D) -> Self {
        Ported(Mutex::new(Some(delay)))
    }
}

impl<D: Delay + Jitter + Send> Wait for Ported<'_, D> {
    /// Sleeps `backoff` on the port. The kernel waits one backoff at a
    /// time; a second wait begun during the first waits not at all.
    fn wait(&self, backoff: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let taken = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(delay) = taken {
                delay.sleep(backoff).await;
                *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(delay);
            }
        })
    }

    /// The kernel takes `jitter * draw` of each wait off (with
    /// [`Backoff::on_retryable`]'s jitter of 1, `draw` of it), and the port
    /// draws the wait to keep, so the draw is what the port did not keep:
    /// `1 - up_to(DRAW_SCALE) / DRAW_SCALE`, a draw past the scale held to
    /// it.
    fn draw(&self) -> f64 {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let kept = held
            .as_mut()
            .map_or(DRAW_SCALE, |delay| delay.up_to(DRAW_SCALE))
            .min(DRAW_SCALE);
        1.0 - kept.as_secs_f64() / DRAW_SCALE.as_secs_f64()
    }
}

/// Whether `failure` is the kernel's answer to re-read and decide again
/// (ABORTED: a command sent at a head the stream has passed, an overlap, an
/// unresolved window), which the kernel never retries for its caller.
pub fn to_redecide(failure: &Failure<Rejected>) -> bool {
    matches!(
        failure,
        Failure::Kernel(
            KernelError::Stale { .. }
                | KernelError::Overlap { .. }
                | KernelError::Unresolved { .. }
        )
    )
}

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
    /// The kernel's retry of a command, as this backoff says: `attempts`
    /// tries of a store that could not say (the first included) and
    /// `attempts` runs of one that keeps losing its race, each wait's
    /// ceiling [`Backoff::ceiling`]'s, drawn by full jitter (from zero to
    /// the ceiling).
    pub const fn on_retryable(&self) -> OnRetryable {
        OnRetryable {
            max_retries: self.attempts.saturating_sub(1),
            max_race_runs: self.attempts,
            initial_backoff: self.base,
            multiplier: 2.0,
            max_backoff: self.cap,
            jitter: 1.0,
        }
    }

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

impl<C: Clock, D: Delay + Jitter + Send> Runner<'_, C, D> {
    /// Run `book` under `command_id` on `store`, the kernel retrying it,
    /// until it commits, is refused, or fails: in a way a retry will not
    /// mend, or with its retries spent.
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
        let shape = shape.map(|shape| |task: &TaskAggregate| shape(task.state()));
        self.run_shaped_on::<TaskAggregate, _, _>(store, book, command_id, shape.as_ref())
            .await
    }

    /// [`Runner::run_shaped`] on a stream of the domain `A`, any marked-up
    /// aggregate: the schema stream's as well as a task's.
    pub async fn run_shaped_on<A, S, F>(
        &mut self,
        store: &S,
        book: &CommandBook,
        command_id: &Uuid,
        shape: Option<&F>,
    ) -> Result<Option<Outcome>, AppError>
    where
        A: Domain<State = A> + Default + Clone,
        S: Store,
        F: Fn(&A) -> Option<CommandBook> + Sync,
    {
        let mut failed = 0;
        loop {
            let sampled = Sampled {
                now: self.clock.now(),
                command_id: command_id.clone(),
            };
            let (failure, recovery) =
                match shaped::<A, _, _>(self.registry, store, book, shape).await {
                    Ok(None) => return Ok(None),
                    Ok(Some(book)) => {
                        let waits = Ported::new(&mut *self.delay);
                        let handled = Kernel::new(self.registry, self.dead_letters)
                            .on_retryable(self.backoff.on_retryable())
                            .wait_with(&waits)
                            .handle::<A, _, _>(store, book, &sampled, &mut ())
                            .await;
                        match handled {
                            Ok(_) => return Ok(Some(Outcome::Committed)),
                            Err(Failure::Rejected(rejection)) => {
                                return Ok(Some(Outcome::Refused(rejection.report().clone())))
                            }
                            Err(failure) => self.retried(failure)?,
                        }
                    }
                    Err(failed) => failed,
                };
            self.again(&mut failed, failure, recovery).await?;
        }
    }

    /// What the kernel handed back, `failure`, as a failure to re-read
    /// after: the kernel's answer to decide again ([`to_redecide`]), or one
    /// no retry mends. A retryable one it did not ask to be re-read has
    /// had the kernel's retries, and is given up on at once.
    fn retried(&self, failure: Failure<Rejected>) -> Result<(Report, Recovery), AppError> {
        let recovery = failure.recovery();
        if recovery == Recovery::Retry && !to_redecide(&failure) {
            return Err(AppError::GaveUp {
                attempts: self.backoff.attempts,
                error: Report::of(&failure),
            });
        }
        Ok((Report::of(&failure), recovery))
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

    /// After the `failed`th failure of what taskloom re-reads (a read, a
    /// shaped command answered stale, a purge), wait to try again; or the
    /// error to end with, when a retry will not mend it or the attempts
    /// are spent.
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
    let read = state_at::<TaskAggregate, _>(registry, store, stream).await?;
    Ok(read.map(|(task, head)| (task.state().clone(), head)))
}

/// The aggregate `stream` holds, replayed, and its head; `None` for a
/// stream with no events.
async fn state_at<A: Domain<State = A> + Clone, S: EventStore>(
    registry: &Registry,
    store: &S,
    stream: &Cover,
) -> Result<Option<(A, u64)>, (Report, Recovery)> {
    let at = StreamRef::try_from(stream).map_err(store_failed)?;
    let slice = store.read(at, 0, None).await.map_err(store_failed)?;
    if slice.pages.is_empty() {
        return Ok(None);
    }
    let replayed = Stream::<A>::replay(stream.clone(), &slice.pages, registry)
        .map_err(|e| (Report::of(&e), Recovery::Bug))?;
    Ok(Some((replayed.state().clone(), slice.next_sequence)))
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

/// What `shape` makes of the aggregate on `book`'s stream, read from
/// `store`, sent at the head read; `book` itself when there is no `shape`,
/// without reading.
async fn shaped<A, S, F>(
    registry: &Registry,
    store: &S,
    book: &CommandBook,
    shape: Option<&F>,
) -> Result<Option<CommandBook>, (Report, Recovery)>
where
    A: Domain<State = A> + Default + Clone,
    S: EventStore,
    F: Fn(&A) -> Option<CommandBook> + Sync,
{
    let Some(shape) = shape else {
        return Ok(Some(book.clone()));
    };
    let cover = book.cover.clone().unwrap_or_default();
    let (task, head) = match state_at::<A, _>(registry, store, &cover).await {
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
