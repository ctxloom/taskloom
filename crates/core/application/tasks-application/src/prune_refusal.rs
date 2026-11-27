//! Why a prune is refused. The refusals, their codes and their catalog
//! messages are in every flavour (the catalog is one text per crate); the
//! verb that raises them is the `purge` feature's alone (`prune`).

use std::fmt;

use coded_diagnostic::{code, Arg, Diagnostic};

/// Why a prune was refused before it read anything.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PruneRefusal {
    /// No status to select by: an empty set is never read as every status.
    NoStatus,
    /// No age to select by.
    AgeRequired,
    /// An offset of zero or less.
    AgeNotPositive { age: String },
    /// Neither an offset nor a date.
    AgeUnreadable { age: String },
}

impl fmt::Display for PruneRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl Diagnostic for PruneRefusal {
    fn code(&self) -> &str {
        match self {
            PruneRefusal::NoStatus => code!("prune-no-status"),
            PruneRefusal::AgeRequired => code!("prune-age-required"),
            PruneRefusal::AgeNotPositive { .. } => code!("prune-age-not-positive"),
            PruneRefusal::AgeUnreadable { .. } => code!("prune-age-unreadable"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            PruneRefusal::AgeNotPositive { age } | PruneRefusal::AgeUnreadable { age } => {
                vec![("age", age.as_str().into())]
            }
            PruneRefusal::NoStatus | PruneRefusal::AgeRequired => Vec::new(),
        }
    }
}

/// One of every refusal, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn samples() -> Vec<PruneRefusal> {
    let age = || "0d".to_string();
    vec![
        PruneRefusal::NoStatus,
        PruneRefusal::AgeRequired,
        PruneRefusal::AgeNotPositive { age: age() },
        PruneRefusal::AgeUnreadable { age: age() },
    ]
}

#[cfg(test)]
#[path = "prune_refusal.test.rs"]
mod tests;
