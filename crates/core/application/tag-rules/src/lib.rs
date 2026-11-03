//! Tags as a write takes them, and the store's tag rules: shared by every
//! thing that holds tags (tasks, and plans), so none depends on a use
//! case crate to validate one.
//!
//! - [`tags`]: strings in tagma's grammar parsed into [`Tag`]s, and
//!   admitted if they could be queried, stay out of the reserved
//!   namespaces and are not retired ([`TagRefusal`]);
//! - [`rules`]: the store's rules as tagma's tags ([`StoreRules`]), read
//!   from a [`tasks::schema::SchemaState`] its caller reads from the
//!   store, and the findings tagma judges a write by ([`judged`]).
//!
//! Pure: what a store holds is read elsewhere and handed in.

pub mod rules;
pub mod tags;

pub use rules::{judged, RuleWarning, RulesRefusal, StoreRules};
pub use tags::{Tag, TagRefusal};

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!(concat!(env!("OUT_DIR"), "/en-US.ftl")),
);

/// One value of every diagnostic this crate defines, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn angzarr_diagnostic::Diagnostic>> {
    let mut samples: Vec<Box<dyn angzarr_diagnostic::Diagnostic>> = tags::samples()
        .into_iter()
        .map(|t| Box::new(t) as Box<dyn angzarr_diagnostic::Diagnostic>)
        .collect();
    samples.extend(rules::samples());
    samples
}

#[cfg(test)]
#[path = "english.test.rs"]
mod english;

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
