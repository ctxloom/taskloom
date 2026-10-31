//! Tag queries: tagma-core's engines over a listing's tasks.
//!
//! A query is postfix only (`urgent/release/and`, `urgent/not`): tags
//! first and the operator after, tokens separated by `/`. A query postfix
//! cannot read is refused, whatever else it might read as. The operators
//! `and`, `or` and `not` are matched in any case.
//!
//! The index holds only the tasks the listing admits (its candidates), so
//! `not` complements within them, not within every task. Each candidate
//! carries an inert presence tag besides its own, so a task with no tag
//! still takes part (`urgent/not` holds it). A target the schema declares
//! `type=<name>` is ordered by the comparator registered under that name:
//! SemVer precedence for `semver`, the one taskloom registers.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};
use tagma_core::Index;

use crate::semver::{Semver, SEMVER};
use crate::store_schema::StoreRules;
use crate::Tag;

/// The namespace of the presence tag every candidate carries. A task may
/// hold a tag in it too, which changes nothing: the presence tag is on
/// every candidate already.
const PRESENCE_NAMESPACE: &str = "taskloom.internal";

/// The item the schema's type declarations are indexed under: no harp id,
/// since a harp has no colon.
const TYPE_CONFIG_ITEM: &str = "taskloom.internal:type-config";

/// How to write a query, said with every malformed one.
pub const QUERY_GRAMMAR: &str = "queries are postfix, tags first and the operator after (human:decision/area:cli/and, human:decision/not), and an and/or/not needs enough operands before it";

/// A malformed query.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QueryError {
    pub query: String,
    /// Why postfix cannot read it.
    pub postfix: String,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for QueryError {
    fn code(&self) -> &'static str {
        "tasks-application-tag-query"
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        vec![
            ("query", self.query.as_str().into()),
            ("postfix", self.postfix.as_str().into()),
            ("grammar", QUERY_GRAMMAR.into()),
        ]
    }
}

/// The ids of `candidates` (`(harp id, tags)`) that `query` selects under
/// the store's `rules` (its `tagma.type` declarations), or why the query is malformed. No query selects every
/// candidate; a blank one is refused before it gets here
/// ([`crate::AppError::Blank`]), and tagma judges one that reaches it.
pub fn select<'a>(
    query: Option<&str>,
    candidates: impl IntoIterator<Item = (&'a str, &'a [Tag])>,
    rules: &StoreRules,
) -> Result<BTreeSet<String>, QueryError> {
    let mut index = Index::new();
    index.register_type(SEMVER, Arc::new(Semver));
    // Only the types: an index reads the other facets too (arity collapses
    // a candidate's values, hide narrows what a query sees), and a query
    // is over the tags as stored.
    let types = rules.types();
    if !types.is_empty() {
        index.add_item(TYPE_CONFIG_ITEM, types);
    }
    let mut ids = BTreeSet::new();
    for (id, tags) in candidates {
        let mut tags: Vec<tagma_core::Tag> = tags.iter().map(Tag::to_core).collect();
        tags.push(presence());
        index.add_item(id, tags);
        ids.insert(id.to_string());
    }
    let Some(query) = query else {
        return Ok(ids);
    };
    let matched = index.query_postfix(query).map_err(|postfix| QueryError {
        query: query.to_string(),
        postfix,
    })?;
    Ok(matched.into_iter().filter(|id| ids.contains(id)).collect())
}

/// Refuse `query` if it is malformed, whatever it would be run over.
pub fn check(query: Option<&str>) -> Result<(), QueryError> {
    select(query, [], &StoreRules::default()).map(|_| ())
}

/// The tag every candidate carries, so that it takes part in the query.
fn presence() -> tagma_core::Tag {
    tagma_core::Tag {
        namespace: Some(PRESENCE_NAMESPACE.to_string()),
        key: "candidate".to_string(),
        value: None,
    }
}

#[cfg(test)]
#[path = "query.test.rs"]
mod tests;
