//! Deterministic stand-ins for the time and waiting ports, for the use
//! cases' tests (feature `test-support`): the one copy every application's
//! fakes share.

use std::time::Duration;

use crate::{Clock, CommandIds, Delay, Jitter, Timestamp, Uuid};

/// A clock one second on at each reading (from second 1_700_000_001), and
/// command ids numbered from 1, each `[n; 16]`, every one handed out kept
/// in `ids`.
#[derive(Default)]
pub struct FakeClock {
    pub seconds: i64,
    pub ids: Vec<Uuid>,
}

impl Clock for FakeClock {
    fn now(&mut self) -> Timestamp {
        self.seconds += 1;
        Timestamp {
            seconds: 1_700_000_000 + self.seconds,
            nanos: 0,
        }
    }
}

impl CommandIds for FakeClock {
    fn command_id(&mut self) -> Uuid {
        let n = u8::try_from(self.ids.len() + 1).expect("few ids");
        let id = Uuid::from_bytes([n; 16]);
        self.ids.push(id.clone());
        id
    }
}

/// Waits recorded, never waited; jitter is a fixed fraction (in
/// thousandths, the whole ceiling by default) of the ceiling, or a fixed
/// duration when `over` is set. Each ceiling asked for is kept.
pub struct FakeDelay {
    pub slept: Vec<Duration>,
    pub ceilings: Vec<Duration>,
    pub permille: u32,
    pub over: Option<Duration>,
}

impl Default for FakeDelay {
    fn default() -> Self {
        FakeDelay {
            slept: Vec::new(),
            ceilings: Vec::new(),
            permille: 1000,
            over: None,
        }
    }
}

impl Delay for FakeDelay {
    async fn sleep(&mut self, duration: Duration) {
        self.slept.push(duration);
    }
}

impl Jitter for FakeDelay {
    fn up_to(&mut self, ceiling: Duration) -> Duration {
        self.ceilings.push(ceiling);
        self.over.unwrap_or(ceiling * self.permille / 1000)
    }
}

#[cfg(test)]
#[path = "testing.test.rs"]
mod tests;
