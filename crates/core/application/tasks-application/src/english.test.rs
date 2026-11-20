//! A diagnostic's en-US message, from this crate's catalogs (and the
//! domain's and task-priority's): what an edge shows, for the tests that
//! hold the English to Go's. A code no catalog has is its key and args, as
//! an edge shows it.

use angzarr_diagnostic::Diagnostic;

/// `d`'s en-US message, rendered as angzarr-presentation renders it.
pub fn english(d: &dyn Diagnostic) -> String {
    angzarr_presentation::english::english_in(&crate::CATALOGS.map(|(_, ftl)| ftl), d)
}

/// [`english`], as a method of every diagnostic.
pub trait English {
    fn english(&self) -> String;
}

impl<D: Diagnostic> English for D {
    fn english(&self) -> String {
        english(self)
    }
}
