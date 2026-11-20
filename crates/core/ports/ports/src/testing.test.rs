//! The stand-ins answer as their tests rely on.

use std::future::Future;
use std::time::Duration;

use super::{FakeClock, FakeDelay};
use crate::{Clock, CommandIds, Delay, Jitter, Timestamp, Uuid};

#[test]
fn the_clock_moves_a_second_at_each_reading() {
    let mut clock = FakeClock::default();
    assert_eq!(
        clock.now(),
        Timestamp {
            seconds: 1_700_000_001,
            nanos: 0
        }
    );
    assert_eq!(
        clock.now(),
        Timestamp {
            seconds: 1_700_000_002,
            nanos: 0
        }
    );
}

#[test]
fn command_ids_are_numbered_from_one_and_kept() {
    let mut clock = FakeClock::default();
    assert_eq!(clock.command_id(), Uuid::from_bytes([1; 16]));
    assert_eq!(clock.command_id(), Uuid::from_bytes([2; 16]));
    assert_eq!(
        clock.ids,
        [Uuid::from_bytes([1; 16]), Uuid::from_bytes([2; 16])]
    );
}

#[test]
fn a_wait_is_recorded_and_jitter_is_the_fraction_asked_for() {
    let mut delay = FakeDelay::default();
    {
        let wait = std::pin::pin!(delay.sleep(Duration::from_millis(7)));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(wait.poll(&mut cx).is_ready());
    }
    assert_eq!(delay.slept, [Duration::from_millis(7)]);
    let ceiling = Duration::from_millis(100);
    assert_eq!(delay.up_to(ceiling), ceiling);
    delay.permille = 250;
    assert_eq!(delay.up_to(ceiling), Duration::from_millis(25));
    delay.over = Some(Duration::from_millis(3));
    assert_eq!(delay.up_to(ceiling), Duration::from_millis(3));
    assert_eq!(delay.ceilings, [ceiling, ceiling, ceiling]);
}
