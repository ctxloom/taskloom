//! Tags as the use cases take them: strings in tagma's grammar,
//! `(namespace:)key(=value)`, parsed into [`Tag`]s at the edge of every
//! write, and admitted only if they could be queried, stay out of the
//! config namespaces ([`schema_free`]) and are not retired ([`retired`]):
//! a write is refused at the first that is not, with the reason. What the
//! store's rules say of a tag (its value, its arity, its namespace's keys,
//! whether it is permitted) is tagma's to judge, over the tag set the
//! write leaves (`crate::store_schema`), never this module's.
//!
//! The value a scalar target is given displaces the one held before the
//! retag is sent ([`collapse`]): decide never reads the schema.

use std::collections::BTreeMap;
use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};

pub use tasks::Tag;

use crate::store_schema::{StoreRules, TAGMA_NAMESPACE, TASKLOOM_NAMESPACE};

/// The words tagma's query grammar reads as operators, in any case: a tag
/// spelled as one could never be matched.
pub const OPERATOR_WORDS: [&str; 3] = ["and", "or", "not"];

/// The namespaces no task's tag may be in, each with its dot-descendants:
/// tagma's, where tagma reads a tag as its own config whatever item
/// carries it, and taskloom's, the store's formulas.
pub const RESERVED_NAMESPACES: [&str; 2] = [TAGMA_NAMESPACE, TASKLOOM_NAMESPACE];

/// Why a tag given to a write was refused.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TagRefusal {
    /// Blank: a tag given is never read as none given.
    Blank,
    /// Not a tag in tagma's grammar: it could never be queried.
    Unparseable { tag: String, detail: String },
    /// `and`, `or` or `not`, which a query always reads as an operator.
    OperatorWord { tag: String },
    /// In `tagma` or `taskloom` (or under one), which are the schema's.
    ReservedNamespace { tag: String, namespace: String },
    /// A tag whose target is retired: plans' `ctxloom:plan`, whose work
    /// plans and their governs links now do.
    Retired { tag: String, target: String },
}

impl fmt::Display for TagRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for TagRefusal {
    fn code(&self) -> &str {
        match self {
            TagRefusal::Blank => "tasks-application-tag-blank",
            TagRefusal::Unparseable { .. } => "tasks-application-tag-unparseable",
            TagRefusal::OperatorWord { .. } => "tasks-application-tag-operator-word",
            TagRefusal::ReservedNamespace { .. } => "tasks-application-tag-reserved-namespace",
            TagRefusal::Retired { .. } => "tasks-application-tag-retired",
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        let s = |v: &String| Arg::from(v.as_str());
        match self {
            TagRefusal::Blank => Vec::new(),
            TagRefusal::Unparseable { tag, detail } => {
                vec![("tag", s(tag)), ("ext_err_txt", s(detail))]
            }
            TagRefusal::OperatorWord { tag } => vec![("tag", s(tag))],
            TagRefusal::ReservedNamespace { tag, namespace } => {
                vec![("tag", s(tag)), ("namespace", s(namespace))]
            }
            TagRefusal::Retired { tag, target } => vec![("tag", s(tag)), ("target", s(target))],
        }
    }
}

/// `spelled`, trimmed, parsed as tagma parses a tag; `None` when it is
/// empty once trimmed, which is no tag at all.
pub fn parse(spelled: &str) -> Option<Result<Tag, TagRefusal>> {
    let trimmed = spelled.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        tagma_core::Tag::parse(trimmed)
            .map(Tag::from)
            .map_err(|detail| TagRefusal::Unparseable {
                tag: trimmed.to_string(),
                detail,
            }),
    )
}

/// Every tag `spelled` names, in its order, as a write is given them;
/// refused at the first that does not parse, or is blank: a tag given is
/// never read as none (an import, reading what Go wrote, leaves a blank
/// one out through [`parse`] instead).
pub fn parse_all(spelled: &[String]) -> Result<Vec<Tag>, TagRefusal> {
    spelled
        .iter()
        .map(|s| parse(s).unwrap_or(Err(TagRefusal::Blank)))
        .collect()
}

/// Whether `tag` passes what every task's tag must, whatever the
/// store's rules: it could be queried (not a bare operator word), and it
/// stays out of the reserved namespaces ([`RESERVED_NAMESPACES`]). A write
/// and an import both apply it; tagma's parser takes `and` as an ordinary
/// key, and its rules exempt the `tagma` family, so neither check is
/// tagma's.
pub fn schema_free(tag: &Tag) -> Result<(), TagRefusal> {
    let bare = tag.namespace.is_none() && tag.value.is_none();
    if bare && OPERATOR_WORDS.contains(&tag.key.to_ascii_lowercase().as_str()) {
        return Err(TagRefusal::OperatorWord {
            tag: tag.to_string(),
        });
    }
    let Some(namespace) = &tag.namespace else {
        return Ok(());
    };
    if is_reserved(namespace) {
        return Err(TagRefusal::ReservedNamespace {
            tag: tag.to_string(),
            namespace: namespace.clone(),
        });
    }
    Ok(())
}

/// Whether `namespace` is a reserved one or under one.
pub(crate) fn is_reserved(namespace: &str) -> bool {
    RESERVED_NAMESPACES.iter().any(|root| {
        namespace
            .strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    })
}

/// Refused when `tag`'s target is retired ([`plans::tag_is_retired`]): a
/// write never adds one, though a task may still carry one an import kept
/// (`plan import` migrates them).
pub fn retired(tag: &Tag) -> Result<(), TagRefusal> {
    let target = tag.target();
    match plans::tag_is_retired(&target) {
        true => Err(TagRefusal::Retired {
            tag: tag.to_string(),
            target,
        }),
        false => Ok(()),
    }
}

/// Whether a write may add `tag`, whatever the store's rules: it passes
/// [`schema_free`] and its target is not [`retired`]. An import holds a
/// tag to [`schema_free`] alone, keeping a retired one for `plan import`.
pub(crate) fn admit(tag: &Tag) -> Result<(), TagRefusal> {
    schema_free(tag)?;
    retired(tag)
}

/// Every tag `spelled` names, parsed and held to [`admit`], in its order;
/// refused at the first that is not.
pub(crate) fn admit_all(spelled: &[String]) -> Result<Vec<Tag>, TagRefusal> {
    let tags = parse_all(spelled)?;
    for tag in &tags {
        admit(tag)?;
    }
    Ok(tags)
}

/// `add`, with every value of a scalar target but its last dropped, and the
/// tags of `held` that a surviving scalar value displaces: a different
/// value of the same target. Which targets are scalar is the store's
/// `tagma.arity` ([`StoreRules::is_scalar`]); tagma's validate collapses
/// the same way (§8: last value wins, within a batch too).
pub(crate) fn collapse(rules: &StoreRules, held: &[Tag], add: Vec<Tag>) -> (Vec<Tag>, Vec<Tag>) {
    let scalar = |tag: &Tag| rules.is_scalar(tag).then(|| tag.target());
    let mut last: BTreeMap<String, usize> = BTreeMap::new();
    for (i, tag) in add.iter().enumerate() {
        if let Some(target) = scalar(tag) {
            last.insert(target, i);
        }
    }
    let kept: Vec<Tag> = add
        .into_iter()
        .enumerate()
        .filter(|(i, tag)| scalar(tag).is_none_or(|target| last[&target] == *i))
        .map(|(_, tag)| tag)
        .collect();
    let surviving: BTreeMap<String, &Option<String>> = kept
        .iter()
        .filter_map(|tag| Some((scalar(tag)?, &tag.value)))
        .collect();
    let displaced = held
        .iter()
        .filter(|tag| {
            scalar(tag)
                .and_then(|target| surviving.get(&target).copied())
                .is_some_and(|value| *value != tag.value)
        })
        .cloned()
        .collect();
    (kept, displaced)
}

/// One of every refusal, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn samples() -> Vec<TagRefusal> {
    let x = || "x".to_string();
    vec![
        TagRefusal::Blank,
        TagRefusal::Unparseable {
            tag: x(),
            detail: x(),
        },
        TagRefusal::OperatorWord { tag: x() },
        TagRefusal::ReservedNamespace {
            tag: x(),
            namespace: x(),
        },
        TagRefusal::Retired {
            tag: x(),
            target: x(),
        },
    ]
}

#[cfg(test)]
#[path = "tags.test.rs"]
mod tests;
