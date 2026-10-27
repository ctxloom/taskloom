//! Dead letters, surfaced. annals' kernel writes a line to the log it is
//! handed for what angzarr would dead-letter: an ingest that panicked, a
//! conflict window decide never acknowledged, a policy's malformed book.
//! Each almost certainly marks a bug, and one can precede a retry that
//! then succeeds, so a use case returns the lines written while it ran as
//! [`DeadLetter`]s beside its result, for its driving adapter to say
//! loudly. The application does no I/O: it writes them nowhere itself.

use std::fmt;
use std::sync::{Mutex, PoisonError};

use annals::next::DeadLetterLog;
use annals_diagnostic::{Arg, Diagnostic};

/// One line annals' kernel dead-lettered, as it wrote it (starting with
/// annals' `DEAD_LETTER_PREFIX`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeadLetter {
    pub line: String,
}

impl fmt::Display for DeadLetter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.line)
    }
}

impl Diagnostic for DeadLetter {
    fn code(&self) -> &'static str {
        "tasks-application-dead-letter"
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        vec![("line", Arg::from(self.line.as_str()))]
    }
}

/// The log a use case hands annals' kernel: the lines written to it, kept
/// until the use case takes them.
#[derive(Default)]
pub(crate) struct DeadLetters(Mutex<Vec<String>>);

impl DeadLetters {
    /// Every line written since the last take, in the order written.
    pub(crate) fn take(&self) -> Vec<DeadLetter> {
        let mut lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut *lines)
            .into_iter()
            .map(|line| DeadLetter { line })
            .collect()
    }
}

impl DeadLetterLog for DeadLetters {
    fn write(&self, line: &str) {
        let mut lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        lines.push(line.to_string());
    }
}
