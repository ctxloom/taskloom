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
//! **The kernel shapes again what was shaped.** A command that depends on
//! the task as it is (a scalar tag displacing the one held, a removal only
//! of a task that still qualifies) is shaped from the stream as the kernel
//! loads it and sent at that head, STRICT ([`Runner::shaped`],
//! [`Kernel::handle_shaped`]): a write landing between is stale, and the
//! kernel loads and shapes it again, under its race re-run cap. What the
//! kernel never runs, a purge at the head read, taskloom reads and retries
//! itself ([`Runner::purge`]).
//!
//! **A refusal is typed.** The domain's own rejection comes back as its
//! handler made it ([`Refusal`]), matched as a value, never by its code.

use std::time::Duration;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};

use angzarr_diagnostic::Report;
use angzarr_embedded::markup::Rejection;
use angzarr_embedded::v1::{CommandBook, Cover, Uuid};
use angzarr_embedded::{
    DeadLetter, Domain, Failure, Handled, Kernel, KernelError, OnRetryable, Payload, Purge,
    Recovery, Registry, Rejected, Sampled, Store, StreamRef, Wait,
};
use ports::{Clock, Delay, Jitter};
use tasks::TaskAggregate;

use crate::AppError;

/// How the application has the kernel retry: [`Backoff::DEFAULT`], as
/// [`Backoff::on_retryable`] makes it. A command stamped with the head it
/// loaded that loses the race at its write is re-run on the new head up to
/// the kernel's default race re-run cap, 10 runs (angzarr decision 14.46),
/// and a store that could not say is tried up to 10 times. At either cap
/// the kernel gives it up and answers taskloom, its waiting caller, with no
/// dead letter (angzarr's owner ruling, 2026-10-31: only work with no
/// caller left is dead-lettered); taskloom then gives up ([`AppError::GaveUp`]).
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

/// A domain's refusal of a command, as the use case reads it: its own
/// rejection, typed, as its handler made it (`None` for one the router
/// made on its behalf), and as reported.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Refusal<R> {
    pub rejection: Option<R>,
    pub report: Report,
}

impl<R> From<Refusal<R>> for AppError {
    fn from(refusal: Refusal<R>) -> AppError {
        AppError::Rejected(refusal.report)
    }
}

/// The dead letters a kernel call answered, handled or failed.
pub fn answered<R>(handled: &Result<Handled, Failure<R>>) -> &[DeadLetter] {
    match handled {
        Ok(handled) => handled.dead_letters(),
        Err(failure) => failure.dead_letters(),
    }
}

/// What [`Runner::run`] needs besides the store and the command.
pub(crate) struct Runner<'a, C, D> {
    pub registry: &'a Registry,
    pub clock: &'a mut C,
    pub delay: &'a mut D,
    pub backoff: Backoff,
    /// The dead letters the kernel's calls answered, gathered for the use
    /// case to return with its result.
    pub dead_letters: &'a mut Vec<DeadLetter>,
}

impl<C: Clock, D: Delay + Jitter + Send> Runner<'_, C, D> {
    /// Run `book`, a command to a stream of the marked-up aggregate `A`,
    /// under `command_id` on `store`, the kernel retrying it: what it came
    /// to, committed (or a duplicate of what did) or refused; or the
    /// failure, a retryable one given up on at the kernel's cap.
    pub async fn run<A, R, S>(
        &mut self,
        store: &S,
        book: CommandBook,
        command_id: &Uuid,
    ) -> Result<Result<Handled, Refusal<R>>, AppError>
    where
        A: Domain<Rejection = Rejection<R>>,
        R: Clone + Send + Sync + 'static,
        S: Store,
    {
        let sampled = self.sampled(command_id);
        let waits = Ported::new(&mut *self.delay);
        let handled = kernel(self.registry, self.backoff, &waits)
            .handle::<A, _>(store, book, &sampled)
            .await;
        self.dead_letters.extend_from_slice(answered(&handled));
        self.settled(handled)
    }

    /// Run what `shape` makes of the `A` stream `stream`, as the kernel
    /// loads it, under `command_id`: sent at the head shaped from, shaped
    /// again by the kernel when a write lands between
    /// ([`Kernel::handle_shaped`]). Beside what it came to (`None`: the
    /// shape made nothing, and nothing was sent), the shape's value.
    pub async fn shaped<A, R, S, P, T>(
        &mut self,
        store: &S,
        stream: &Cover,
        command_id: &Uuid,
        shape: impl FnMut(&A::State) -> (Option<P>, T),
    ) -> Result<Result<(Option<Handled>, T), Refusal<R>>, AppError>
    where
        A: Domain<Rejection = Rejection<R>>,
        R: Clone + Send + Sync + 'static,
        S: Store,
        P: Payload,
    {
        let sampled = self.sampled(command_id);
        let waits = Ported::new(&mut *self.delay);
        let shaped = kernel(self.registry, self.backoff, &waits)
            .handle_shaped::<A, _, _, _>(store, stream, &sampled, shape)
            .await;
        if let Ok((Some(handled), _)) = &shaped {
            self.dead_letters.extend_from_slice(handled.dead_letters());
        }
        if let Err(failure) = &shaped {
            self.dead_letters.extend_from_slice(failure.dead_letters());
        }
        self.settled(shaped)
    }

    /// The instant now and `command_id`: what the kernel stamps a command
    /// with.
    fn sampled(&mut self, command_id: &Uuid) -> Sampled {
        Sampled {
            now: self.clock.now(),
            command_id: command_id.clone(),
        }
    }

    /// What the kernel answered, as the use case reads it: its answer, or
    /// the domain's refusal; a retryable failure, which has had the
    /// kernel's retries, given up on at once ([`AppError::GaveUp`]); any
    /// other failure, the use case's.
    fn settled<H, R: Clone + 'static>(
        &self,
        handled: Result<H, Failure<Rejected>>,
    ) -> Result<Result<H, Refusal<R>>, AppError> {
        match handled {
            Ok(handled) => Ok(Ok(handled)),
            Err(Failure::Rejected(rejected)) => Ok(Err(Refusal {
                rejection: rejected
                    .rejection::<Rejection<R>>()
                    .and_then(Rejection::message)
                    .cloned(),
                report: rejected.report().clone(),
            })),
            Err(failure) if failure.recovery() == Recovery::Retry => Err(AppError::GaveUp {
                attempts: self.backoff.attempts,
                error: Report::of(&failure),
            }),
            Err(failure) => Err(AppError::Failed(Report::of(&failure))),
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

    /// After the `failed`th failure of a purge, wait to try again; or the
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

/// The kernel over `registry`, retrying as `backoff` says, its waits
/// `waits`.
fn kernel<'k>(registry: &'k Registry, backoff: Backoff, waits: &'k dyn Wait) -> Kernel<'k> {
    Kernel::new(registry)
        .on_retryable(backoff.on_retryable())
        .wait_with(waits)
}

/// Purge `stream`, if it holds a removed task, at the head it was loaded
/// at: a stream moved since (purged by another prune, a task minted on the
/// freed harp) is the store's conflict, and the retry loads it again.
async fn purge_removed<S: Store + Purge>(
    registry: &Registry,
    store: &S,
    stream: &Cover,
) -> Result<u64, (Report, Recovery)> {
    let kernel_failed = |error: KernelError| {
        let recovery = error.recovery();
        (Report::of(&error), recovery)
    };
    let (task, head) = Kernel::new(registry)
        .load::<TaskAggregate, _>(store, stream)
        .await
        .map_err(kernel_failed)?;
    if !task.state().state().removed || head == 0 {
        return Ok(0);
    }
    let at = StreamRef::try_from(stream).map_err(|e| kernel_failed(KernelError::Store(e)))?;
    store
        .purge(at, head)
        .await
        .map_err(|e| kernel_failed(KernelError::Store(e)))
}

#[cfg(test)]
#[path = "retry.test.rs"]
mod tests;
