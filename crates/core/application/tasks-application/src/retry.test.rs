//! The backoff schedule, and running a command again when the kernel says a retry may mend it.

use std::time::Duration;

use angzarr_embedded_diagnostic::Diagnostic;

use super::{Backoff, Outcome, Ported, Runner, DRAW_SCALE};
use crate::fakes::{ready, FakeClock, FakeDelay, Fault, Flaky};
use crate::{book, registry, AppError};
use tasks::AddTask;

const MS: Duration = Duration::from_millis(1);

#[test]
fn test_each_ceiling_doubles_the_last_from_the_base() {
    let backoff = Backoff {
        base: 10 * MS,
        cap: Duration::from_secs(60),
        attempts: 10,
    };
    let ceilings: Vec<Duration> = (1..=5).map(|n| backoff.ceiling(n)).collect();
    assert_eq!(ceilings, [10 * MS, 20 * MS, 40 * MS, 80 * MS, 160 * MS]);
}

#[test]
fn test_no_ceiling_exceeds_the_cap() {
    let backoff = Backoff {
        base: 10 * MS,
        cap: 50 * MS,
        attempts: 10,
    };
    assert_eq!(backoff.ceiling(3), 40 * MS);
    assert_eq!(backoff.ceiling(4), 50 * MS);
    assert_eq!(
        backoff.ceiling(40),
        50 * MS,
        "a doubling past u32 saturates"
    );
    assert_eq!(backoff.ceiling(u32::MAX), 50 * MS);
}

#[test]
fn test_the_default_is_10ms_doubling_to_1s_over_10_attempts() {
    assert_eq!(
        Backoff::DEFAULT,
        Backoff {
            base: 10 * MS,
            cap: Duration::from_secs(1),
            attempts: 10,
        }
    );
}

/// Run an add of "swift-amber" on `store`, under the first command id.
fn run(store: &mut Flaky, delay: &mut FakeDelay, attempts: u32) -> Result<Outcome, AppError> {
    run_as(1, store, delay, attempts)
}

/// [`run`], under the command id of `id`s.
fn run_as(
    id: u8,
    store: &mut Flaky,
    delay: &mut FakeDelay,
    attempts: u32,
) -> Result<Outcome, AppError> {
    let registry = registry();
    let mut clock = FakeClock::default();
    let command_id = angzarr_embedded::next::v1::Uuid::from_bytes([id; 16]);
    let add = AddTask {
        harp_id: "swift-amber".to_string(),
        text: "Ship it".to_string(),
        status: None,
        trigger: None,
        tags: Vec::new(),
        origin_session: None,
        defers: tasks::DEFERRED.to_string(),
        initial: tasks::TO_DO.to_string(),
    };
    let book = book("swift-amber", add.into());
    let mut runner = Runner {
        registry: &registry,
        clock: &mut clock,
        delay,
        backoff: Backoff {
            base: 10 * MS,
            cap: 30 * MS,
            attempts,
        },
        dead_letters: &angzarr_embedded::next::MemoryDeadLetters::new(),
    };
    ready(runner.run(store, &book, &command_id))
}

fn events(store: &mut Flaky) -> usize {
    let cover = crate::cover("swift-amber");
    ready(store.read_from(&cover, 0))
        .unwrap()
        .map_or(0, |slice| slice.pages.len())
}

#[test]
fn test_a_command_that_commits_at_once_waits_for_nothing() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    let outcome = run(&mut store, &mut delay, 3).unwrap();
    assert!(matches!(outcome, Outcome::Committed));
    assert_eq!(store.begun(), 1);
    assert!(delay.slept.is_empty());
    assert_eq!(events(&mut store), 1);
}

/// The kernel runs the command again (angzarr's owner ruling, 2026-10-31:
/// the kernel owns race re-runs), its waits on taskloom's ports: each a
/// draw of the Jitter port (asked for a fraction of [`DRAW_SCALE`]) taken
/// to the Delay port. It counts a store's failures and lost races apart,
/// each from the base: the failures wait 10ms then 20ms, the race between
/// them 10ms.
#[test]
fn test_a_retryable_failure_is_run_again_after_a_growing_capped_wait() {
    let mut store = Flaky::failing(&[Fault::Unavailable, Fault::Conflict, Fault::Unavailable]);
    let mut delay = FakeDelay::default();
    let outcome = run(&mut store, &mut delay, 5).unwrap();
    assert!(matches!(outcome, Outcome::Committed));
    assert_eq!(store.begun(), 4, "three failures, then the commit");
    assert_eq!(delay.ceilings, [DRAW_SCALE; 3], "one draw a wait");
    assert_eq!(delay.slept, [10 * MS, 10 * MS, 20 * MS]);
    assert_eq!(events(&mut store), 1, "landed once");
}

/// A store's failures past the cap wait at most the cap.
#[test]
fn test_no_wait_exceeds_the_cap() {
    let mut store = Flaky::failing(&[Fault::Unavailable; 3]);
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 5).unwrap();
    assert_eq!(delay.slept, [10 * MS, 20 * MS, 30 * MS]);
}

#[test]
fn test_each_wait_is_the_jitter_drawn_under_its_ceiling() {
    let mut store = Flaky::failing(&[Fault::Unavailable, Fault::Unavailable]);
    let mut delay = FakeDelay {
        permille: 500,
        ..FakeDelay::default()
    };
    run(&mut store, &mut delay, 5).unwrap();
    assert_eq!(delay.slept, [5 * MS, 10 * MS]);
}

#[test]
fn test_a_jitter_past_its_ceiling_is_held_to_it() {
    let mut store = Flaky::failing(&[Fault::Unavailable]);
    let mut delay = FakeDelay {
        over: Some(DRAW_SCALE * 9),
        ..FakeDelay::default()
    };
    run(&mut store, &mut delay, 5).unwrap();
    assert_eq!(delay.slept, [10 * MS]);
}

#[test]
fn test_a_retry_after_a_lost_answer_is_the_duplicate_of_what_landed() {
    let mut store = Flaky::failing(&[Fault::LandsThenFails]);
    let mut delay = FakeDelay::default();
    let outcome = run(&mut store, &mut delay, 5).unwrap();
    assert!(matches!(outcome, Outcome::Committed));
    assert_eq!(
        store.begun(),
        1,
        "one write: the retry found it on the stream"
    );
    assert_eq!(delay.slept.len(), 1, "retried once");
    assert_eq!(events(&mut store), 1, "not added twice");
}

#[test]
fn test_the_attempts_run_out_and_it_gives_up_with_the_last_failure() {
    let mut store = Flaky::failing(&[Fault::Unavailable; 3]);
    let mut delay = FakeDelay::default();
    let Err(AppError::GaveUp { attempts, error }) = run(&mut store, &mut delay, 3) else {
        panic!("gave up");
    };
    assert_eq!(attempts, 3);
    assert_eq!(store.begun(), 3);
    assert_eq!(delay.slept.len(), 2, "no wait after the last attempt");
    assert_eq!(error.code(), "angzarr-ports-store-unavailable");
    assert_eq!(events(&mut store), 0);
}

#[test]
fn test_a_failure_no_retry_mends_is_not_retried() {
    let mut store = Flaky::failing(&[Fault::Discontinuous]);
    let mut delay = FakeDelay::default();
    let Err(AppError::Failed(error)) = run(&mut store, &mut delay, 5) else {
        panic!("failed");
    };
    assert_eq!(error.code(), "angzarr-ports-store-discontinuous");
    assert_eq!(store.begun(), 1);
    assert!(delay.slept.is_empty());
}

#[test]
fn test_a_rejection_is_answered_not_retried() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 5).unwrap();
    let Ok(Outcome::Refused(rejection)) = run_as(2, &mut store, &mut delay, 5) else {
        panic!("refused");
    };
    assert_eq!(
        rejection.to_string(),
        "tasks-task-exists harp_id=swift-amber",
        "its key and args"
    );
    assert_eq!(
        store.begun(),
        1,
        "the add's write; a refusal writes nothing"
    );
    assert!(delay.slept.is_empty());
}

// ------------------------------------------------------------ conditions and purges

/// A runner over `registry`, `clock` and `delay`, 5 attempts.
fn runner<'a>(
    registry: &'a angzarr_embedded::next::Registry,
    clock: &'a mut FakeClock,
    delay: &'a mut FakeDelay,
) -> Runner<'a, FakeClock, FakeDelay> {
    Runner {
        registry,
        clock,
        delay,
        backoff: Backoff {
            base: 10 * MS,
            cap: 30 * MS,
            attempts: 5,
        },
        // None is written here; leaked, so the runner may outlive this.
        dead_letters: Box::leak(Box::new(angzarr_embedded::next::MemoryDeadLetters::new())),
    }
}

fn removal() -> angzarr_embedded::next::v1::CommandBook {
    let remove = tasks::RemoveTask {
        reason: tasks::RemovalReason::Pruned,
    };
    book("swift-amber", remove.into())
}

#[test]
fn test_a_command_whose_condition_fails_on_the_task_read_in_its_unit_is_not_sent() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 3).unwrap();
    let registry = registry();
    let mut clock = FakeClock::default();
    let command_id = angzarr_embedded::next::v1::Uuid::from_bytes([9; 16]);
    let mut runner = runner(&registry, &mut clock, &mut delay);
    let other = |task: &tasks::TaskState| task.text == "Something else";
    let outcome = ready(runner.run_where(&store, &removal(), &command_id, Some(&other)));
    assert!(outcome.unwrap().is_none(), "declined");
    assert_eq!(events(&mut store), 1, "nothing sent");

    let this = |task: &tasks::TaskState| task.text == "Ship it";
    let outcome = ready(runner.run_where(&store, &removal(), &command_id, Some(&this)));
    assert!(matches!(outcome, Ok(Some(Outcome::Committed))));
    assert_eq!(events(&mut store), 2, "the removal landed");
}

#[test]
fn test_a_condition_reads_through_a_unit_that_can_fail_and_be_retried() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 3).unwrap();
    store.fail([Fault::Unavailable]);
    let registry = registry();
    let mut clock = FakeClock::default();
    let command_id = angzarr_embedded::next::v1::Uuid::from_bytes([9; 16]);
    let mut runner = runner(&registry, &mut clock, &mut delay);
    let any = |_: &tasks::TaskState| true;
    let outcome = ready(runner.run_where(&store, &removal(), &command_id, Some(&any)));
    assert!(matches!(outcome, Ok(Some(Outcome::Committed))));
    assert_eq!(
        store.begun(),
        3,
        "the add, then the failed attempt and its retry"
    );
}

#[test]
fn test_a_purge_deletes_a_removed_tasks_stream_and_says_how_many_events() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 3).unwrap();
    let registry = registry();
    let mut clock = FakeClock::default();
    let command_id = angzarr_embedded::next::v1::Uuid::from_bytes([9; 16]);
    let mut runner = runner(&registry, &mut clock, &mut delay);
    ready(runner.run(&store, &removal(), &command_id)).unwrap();
    let stream = crate::cover("swift-amber");
    assert_eq!(ready(runner.purge(&store, &stream)).unwrap(), 2);
    assert_eq!(events(&mut store), 0);
    assert_eq!(
        ready(runner.purge(&store, &stream)).unwrap(),
        0,
        "no such stream"
    );
}

#[test]
fn test_a_purge_leaves_a_live_task_alone() {
    // A task minted on the harp after the removal was purged by another.
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 3).unwrap();
    let registry = registry();
    let mut clock = FakeClock::default();
    let mut runner = runner(&registry, &mut clock, &mut delay);
    let stream = crate::cover("swift-amber");
    assert_eq!(ready(runner.purge(&store, &stream)).unwrap(), 0);
    assert_eq!(events(&mut store), 1, "kept");
}

#[test]
fn test_a_purge_gives_up_once_its_attempts_are_spent() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 3).unwrap();
    let registry = registry();
    let mut clock = FakeClock::default();
    let command_id = angzarr_embedded::next::v1::Uuid::from_bytes([9; 16]);
    let mut runner = runner(&registry, &mut clock, &mut delay);
    ready(runner.run(&store, &removal(), &command_id)).unwrap();
    runner.delay.slept.clear();
    store.fail([Fault::Conflict; 5]);
    let stream = crate::cover("swift-amber");
    let Err(AppError::GaveUp { attempts, error }) = ready(runner.purge(&store, &stream)) else {
        panic!("gave up");
    };
    assert_eq!(attempts, 5);
    assert_eq!(error.code(), "angzarr-ports-store-conflict");
    assert_eq!(runner.delay.slept.len(), 4);
    assert_eq!(events(&mut store), 2, "nothing purged");
}

// ------------------------------------------------------------ one retry loop

#[test]
fn test_the_kernel_retries_under_taskloom_s_default_backoff() {
    assert_eq!(
        super::KERNEL_RETRY.max_race_runs,
        angzarr_embedded::next::DEFAULT_RACE_RUNS,
        "the kernel's default race re-run cap"
    );
    assert_eq!(
        super::KERNEL_RETRY,
        angzarr_embedded::next::OnRetryable {
            max_retries: 9,
            max_race_runs: 10,
            initial_backoff: 10 * MS,
            multiplier: 2.0,
            max_backoff: Duration::from_secs(1),
            jitter: 1.0,
        },
        "Backoff::DEFAULT's attempts, base and cap, with full jitter"
    );
    assert_eq!(Backoff::DEFAULT.on_retryable(), super::KERNEL_RETRY);
    let none = Backoff {
        attempts: 0,
        ..Backoff::DEFAULT
    };
    assert_eq!(none.on_retryable().max_retries, 0, "no attempts, no retry");
}

/// The kernel's waits are the ports': a backoff slept on the Delay port, a
/// draw what the Jitter port did not keep of [`DRAW_SCALE`], a draw past it
/// held to it.
#[test]
fn test_the_kernel_waits_and_draws_on_taskloom_s_ports() {
    use angzarr_embedded::next::Wait;
    let mut delay = FakeDelay {
        permille: 250,
        ..FakeDelay::default()
    };
    {
        let ported = Ported::new(&mut delay);
        assert_eq!(ported.draw(), 0.75);
        ready(ported.wait(7 * MS));
        ready(ported.wait(9 * MS));
    }
    assert_eq!(
        delay.slept,
        [7 * MS, 9 * MS],
        "each wait, slept on the port"
    );
    assert_eq!(delay.ceilings, [DRAW_SCALE]);
    let mut over = FakeDelay {
        over: Some(DRAW_SCALE * 9),
        ..FakeDelay::default()
    };
    assert_eq!(Ported::new(&mut over).draw(), 0.0, "held to the scale");
}

#[test]
fn test_a_store_that_never_answers_is_given_up_at_the_kernels_cap_under_its_key() {
    // The kernel's tries are taskloom's attempts, never its retries times
    // taskloom's; at its cap it dead-letters the command under the spec's
    // key (angzarr decision 14.51), and taskloom does not try it again.
    let store = Flaky::failing(&[Fault::Unavailable; 4]);
    let mut delay = FakeDelay::default();
    let dead_letters = crate::dead_letter::DeadLetters::default();
    let registry = registry();
    let mut clock = FakeClock::default();
    let mut runner = Runner {
        registry: &registry,
        clock: &mut clock,
        delay: &mut delay,
        backoff: Backoff {
            base: 10 * MS,
            cap: 30 * MS,
            attempts: 4,
        },
        dead_letters: &dead_letters,
    };
    let add = AddTask {
        harp_id: "swift-amber".to_string(),
        text: "Ship it".to_string(),
        status: None,
        trigger: None,
        tags: Vec::new(),
        origin_session: None,
        defers: tasks::DEFERRED.to_string(),
        initial: tasks::TO_DO.to_string(),
    };
    let book = book("swift-amber", add.into());
    let id = angzarr_embedded::next::v1::Uuid::from_bytes([1; 16]);
    let Err(AppError::GaveUp { attempts, error }) = ready(runner.run(&store, &book, &id)) else {
        panic!("gave up");
    };
    assert_eq!(attempts, 4);
    assert_eq!(error.code(), "angzarr-ports-store-unavailable");
    assert_eq!(store.begun(), 4, "the kernel's tries, taskloom's none");
    let letters = dead_letters.take();
    assert_eq!(letters.len(), 1, "{letters:?}");
    assert!(
        letters[0]
            .line
            .contains("COORDINATOR_REASON_STORE_UNAVAILABLE cause=fake-busy retry_count=3"),
        "{letters:?}"
    );
}

#[test]
fn test_an_outcome_is_refused_as_only_its_own_key() {
    let refused = Outcome::Refused(angzarr_embedded_diagnostic::Report::new("tasks-removed"));
    assert!(refused.refused_as("tasks-removed"));
    assert!(!refused.refused_as("tasks-no-task"));
    assert!(!Outcome::Committed.refused_as("tasks-removed"));
}
