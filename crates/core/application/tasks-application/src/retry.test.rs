//! The backoff schedule, and running a command again when the kernel says a retry may mend it.

use std::time::Duration;

use angzarr_diagnostic::Diagnostic;

use angzarr_embedded::v1::CommandBook;
use angzarr_embedded::{CommandBookExt, Handled, Payload};

use super::{Backoff, Ported, Refusal, Runner, DRAW_SCALE};
use crate::fakes::{ready, FakeClock, FakeDelay, Fault, Flaky};
use crate::{registry, task_cover, AppError};
use tasks::{AddTask, TaskAggregate, TaskCommand, TaskRejection};

/// What a run came to: committed, or the task domain's refusal.
type Ran = Result<Result<Handled, Refusal<TaskRejection>>, AppError>;

/// `command` to the task swift-amber's stream.
fn book(command: impl Into<TaskCommand>) -> CommandBook {
    CommandBook::command(task_cover("swift-amber"), command.into().to_any())
}

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
fn run(store: &mut Flaky, delay: &mut FakeDelay, attempts: u32) -> Ran {
    run_as(1, store, delay, attempts)
}

/// [`run`], under the command id of `id`s.
fn run_as(id: u8, store: &mut Flaky, delay: &mut FakeDelay, attempts: u32) -> Ran {
    let registry = registry();
    let mut clock = FakeClock::default();
    let command_id = angzarr_embedded::v1::Uuid::from_bytes([id; 16]);
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
    let book = book(add);
    let mut runner = Runner {
        registry: &registry,
        clock: &mut clock,
        delay,
        backoff: Backoff {
            base: 10 * MS,
            cap: 30 * MS,
            attempts,
        },
        dead_letters: &mut Vec::new(),
    };
    ready(runner.run::<TaskAggregate, _, _>(store, book, &command_id))
}

fn events(store: &mut Flaky) -> usize {
    let cover = task_cover("swift-amber");
    ready(store.read_from(&cover, 0))
        .unwrap()
        .map_or(0, |slice| slice.pages.len())
}

#[test]
fn test_a_command_that_commits_at_once_waits_for_nothing() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    let outcome = run(&mut store, &mut delay, 3).unwrap();
    assert!(outcome.is_ok(), "committed");
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
    assert!(outcome.is_ok(), "committed");
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
    run(&mut store, &mut delay, 5).unwrap().unwrap();
    assert_eq!(delay.slept, [10 * MS, 20 * MS, 30 * MS]);
}

#[test]
fn test_each_wait_is_the_jitter_drawn_under_its_ceiling() {
    let mut store = Flaky::failing(&[Fault::Unavailable, Fault::Unavailable]);
    let mut delay = FakeDelay {
        permille: 500,
        ..FakeDelay::default()
    };
    run(&mut store, &mut delay, 5).unwrap().unwrap();
    assert_eq!(delay.slept, [5 * MS, 10 * MS]);
}

#[test]
fn test_a_jitter_past_its_ceiling_is_held_to_it() {
    let mut store = Flaky::failing(&[Fault::Unavailable]);
    let mut delay = FakeDelay {
        over: Some(DRAW_SCALE * 9),
        ..FakeDelay::default()
    };
    run(&mut store, &mut delay, 5).unwrap().unwrap();
    assert_eq!(delay.slept, [10 * MS]);
}

#[test]
fn test_a_retry_after_a_lost_answer_is_the_duplicate_of_what_landed() {
    let mut store = Flaky::failing(&[Fault::LandsThenFails]);
    let mut delay = FakeDelay::default();
    let outcome = run(&mut store, &mut delay, 5).unwrap();
    assert!(outcome.is_ok(), "committed");
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
    run(&mut store, &mut delay, 5).unwrap().unwrap();
    let Ok(Err(refusal)) = run_as(2, &mut store, &mut delay, 5) else {
        panic!("refused");
    };
    assert_eq!(
        refusal.rejection,
        Some(TaskRejection::TaskExists {
            harp_id: "swift-amber".to_string()
        }),
        "the domain's own rejection, typed"
    );
    assert_eq!(
        refusal.report.to_string(),
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
    registry: &'a angzarr_embedded::Registry,
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
        // None is answered here; leaked, so the runner may outlive this.
        dead_letters: Box::leak(Box::new(Vec::new())),
    }
}

fn removal() -> TaskCommand {
    tasks::RemoveTask {
        reason: tasks::RemovalReason::Pruned,
    }
    .into()
}

/// A shaped command is what the shape makes of the task as the kernel
/// loads it: nothing, and nothing is sent, when the task does not qualify.
#[test]
fn test_a_command_whose_shape_declines_the_task_loaded_is_not_sent() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 3).unwrap().unwrap();
    let registry = registry();
    let mut clock = FakeClock::default();
    let command_id = angzarr_embedded::v1::Uuid::from_bytes([9; 16]);
    let mut runner = runner(&registry, &mut clock, &mut delay);
    let stream = task_cover("swift-amber");
    let other = |task: &TaskAggregate| {
        let text = task.state().text.clone();
        ((text == "Something else").then(removal), text)
    };
    let shaped =
        ready(runner.shaped::<TaskAggregate, _, _, _, _>(&store, &stream, &command_id, other));
    let Ok(Ok((None, seen))) = shaped else {
        panic!("declined");
    };
    assert_eq!(seen, "Ship it", "the shape's value comes back beside");
    assert_eq!(events(&mut store), 1, "nothing sent");

    let this = |task: &TaskAggregate| ((task.state().text == "Ship it").then(removal), ());
    let shaped =
        ready(runner.shaped::<TaskAggregate, _, _, _, _>(&store, &stream, &command_id, this));
    assert!(matches!(shaped, Ok(Ok((Some(_), ())))));
    assert_eq!(events(&mut store), 2, "the removal landed");
}

/// The race, forced: another process's edit lands between the kernel's
/// load for the shape and its send. The send is stale; the kernel loads
/// and shapes again, so the shape sees the edit, and the removal lands
/// once, on the new head.
#[test]
fn test_a_shaped_command_that_loses_the_race_is_shaped_again_on_the_new_head() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 3).unwrap().unwrap();
    let registry = registry();
    let mut clock = FakeClock::default();
    let command_id = angzarr_embedded::v1::Uuid::from_bytes([9; 16]);
    let mut runner = runner(&registry, &mut clock, &mut delay);
    let stream = task_cover("swift-amber");
    // The shape's load reads the stream once; the kernel's own load for
    // the send reads it next: the edit lands just before that.
    store.set_before_read(
        2,
        Box::new(|inner| {
            crate::fakes::run(
                inner,
                "swift-amber",
                tasks::EditText {
                    text: "Ship it now".to_string(),
                }
                .into(),
            )
        }),
    );
    let mut seen = Vec::new();
    let shape = |task: &TaskAggregate| {
        seen.push(task.state().text.clone());
        (Some(removal()), ())
    };
    let shaped =
        ready(runner.shaped::<TaskAggregate, _, _, _, _>(&store, &stream, &command_id, shape));
    assert!(matches!(shaped, Ok(Ok((Some(_), ())))));
    assert!(store.before_read_ran());
    assert_eq!(
        seen,
        ["Ship it", "Ship it now"],
        "shaped again on the new head"
    );
    assert_eq!(events(&mut store), 3, "the add, the edit, the removal once");
}

/// A refusal of a shaped command is the domain's own, typed.
#[test]
fn test_a_shaped_command_the_domain_refuses_comes_back_typed() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 3).unwrap().unwrap();
    let registry = registry();
    let mut clock = FakeClock::default();
    let command_id = angzarr_embedded::v1::Uuid::from_bytes([9; 16]);
    let mut runner = runner(&registry, &mut clock, &mut delay);
    let stream = task_cover("swift-amber");
    let twice = |_: &TaskAggregate| (Some(removal()), ());
    ready(runner.shaped::<TaskAggregate, _, _, _, _>(&store, &stream, &command_id, twice))
        .unwrap()
        .unwrap();
    let again = angzarr_embedded::v1::Uuid::from_bytes([10; 16]);
    let shaped = ready(runner.shaped::<TaskAggregate, _, _, _, _>(&store, &stream, &again, twice));
    let Ok(Err(refusal)) = shaped else {
        panic!("refused");
    };
    assert_eq!(
        refusal.rejection,
        Some(TaskRejection::Removed {
            harp_id: "swift-amber".to_string()
        })
    );
}

#[test]
fn test_a_purge_deletes_a_removed_tasks_stream_and_says_how_many_events() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 3).unwrap().unwrap();
    let registry = registry();
    let mut clock = FakeClock::default();
    let command_id = angzarr_embedded::v1::Uuid::from_bytes([9; 16]);
    let mut runner = runner(&registry, &mut clock, &mut delay);
    ready(runner.run::<TaskAggregate, _, _>(&store, book(removal()), &command_id))
        .unwrap()
        .unwrap();
    let stream = task_cover("swift-amber");
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
    run(&mut store, &mut delay, 3).unwrap().unwrap();
    let registry = registry();
    let mut clock = FakeClock::default();
    let mut runner = runner(&registry, &mut clock, &mut delay);
    let stream = task_cover("swift-amber");
    assert_eq!(ready(runner.purge(&store, &stream)).unwrap(), 0);
    assert_eq!(events(&mut store), 1, "kept");
}

#[test]
fn test_a_purge_gives_up_once_its_attempts_are_spent() {
    let mut store = Flaky::default();
    let mut delay = FakeDelay::default();
    run(&mut store, &mut delay, 3).unwrap().unwrap();
    let registry = registry();
    let mut clock = FakeClock::default();
    let command_id = angzarr_embedded::v1::Uuid::from_bytes([9; 16]);
    let mut runner = runner(&registry, &mut clock, &mut delay);
    ready(runner.run::<TaskAggregate, _, _>(&store, book(removal()), &command_id))
        .unwrap()
        .unwrap();
    runner.delay.slept.clear();
    store.fail([Fault::Conflict; 5]);
    let stream = task_cover("swift-amber");
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
        angzarr_embedded::DEFAULT_RACE_RUNS,
        "the kernel's default race re-run cap"
    );
    assert_eq!(
        super::KERNEL_RETRY,
        angzarr_embedded::OnRetryable {
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
    use angzarr_embedded::Wait;
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
    // taskloom's; at its cap it answers under the spec's key (angzarr
    // decision 14.51), dead-lettering nothing, since taskloom is waiting
    // (angzarr's owner ruling, 2026-10-31), and taskloom does not try it
    // again.
    let store = Flaky::failing(&[Fault::Unavailable; 4]);
    let mut delay = FakeDelay::default();
    let mut dead_letters = Vec::new();
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
        dead_letters: &mut dead_letters,
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
    let book = book(add);
    let id = angzarr_embedded::v1::Uuid::from_bytes([1; 16]);
    let Err(AppError::GaveUp { attempts, error }) =
        ready(runner.run::<TaskAggregate, _, _>(&store, book, &id))
    else {
        panic!("gave up");
    };
    assert_eq!(attempts, 4);
    assert_eq!(error.code(), "angzarr-ports-store-unavailable");
    assert_eq!(store.begun(), 4, "the kernel's tries, taskloom's none");
    let letters = dead_letters;
    assert!(letters.is_empty(), "{letters:?}");
}
