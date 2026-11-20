//! Tags as the use cases take them: strings in tagma's grammar, parsed,
//! admitted and judged by `tag-rules`, which tasks and plans share.
//!
//! The value a scalar target is given displaces the one held before the
//! retag is sent ([`collapse`]): decide never reads the schema.

use std::collections::BTreeMap;

pub use tag_rules::tags::{
    admit, admit_all, is_reserved, is_retired, parse, parse_all, schema_free, Tag, TagRefusal,
    OPERATOR_WORDS, RETIRED_TAG,
};

use crate::rules::{StoreRules, StoreRulesExt};

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

#[cfg(test)]
#[path = "tags.test.rs"]
mod tests;
