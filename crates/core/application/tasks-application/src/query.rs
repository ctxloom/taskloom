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
//!
//! A task's metadata is read as virtual tags in the reserved `taskloom`
//! namespace (`taskloom:in-status>=48`), derived when the query names
//! them and never stored ([`crate::virtual_tags`]).

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use coded_diagnostic::{code, Arg, Diagnostic};
use ports::Timestamp;
use tagma_core::Index;

use crate::rules::{StoreRules, TASKLOOM_NAMESPACE};
use crate::virtual_tags::{self, DurationRefusal, Facts, UnknownVirtualTag};
use crate::Tag;
use tagma_core::semver::{Semver, SEMVER};

/// The namespace of the presence tag every candidate carries. A task may
/// hold a tag in it too, which changes nothing: the presence tag is on
/// every candidate already.
const PRESENCE_NAMESPACE: &str = "taskloom.internal";

/// The item the schema's type declarations are indexed under: no harp id,
/// since a harp has no colon.
const TYPE_CONFIG_ITEM: &str = "taskloom.internal:type-config";

/// How to write a query, said with every malformed one.
pub const QUERY_GRAMMAR: &str = "queries are postfix, tags first and the operator after (human:decision/area:cli/and, human:decision/not), and an and/or/not needs enough operands before it";

/// A query that is refused: malformed, or with a duration virtual tag's
/// atom it cannot order, or an atom in the `taskloom` namespace that names
/// no virtual tag.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum QueryRefusal {
    Malformed(QueryError),
    Duration(DurationRefusal),
    UnknownVirtual(UnknownVirtualTag),
}

impl fmt::Display for QueryRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl Diagnostic for QueryRefusal {
    fn code(&self) -> &str {
        match self {
            QueryRefusal::Malformed(e) => e.code(),
            QueryRefusal::Duration(e) => e.code(),
            QueryRefusal::UnknownVirtual(e) => e.code(),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            QueryRefusal::Malformed(e) => e.args(),
            QueryRefusal::Duration(e) => e.args(),
            QueryRefusal::UnknownVirtual(e) => e.args(),
        }
    }
}

/// A malformed query.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QueryError {
    pub query: String,
    /// Why postfix cannot read it.
    pub postfix: String,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl Diagnostic for QueryError {
    fn code(&self) -> &str {
        code!("tag-query")
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        vec![
            ("query", self.query.as_str().into()),
            ("postfix", self.postfix.as_str().into()),
            ("grammar", QUERY_GRAMMAR.into()),
        ]
    }
}

/// The ids of `candidates` that `query` selects, read at `now` (what the
/// virtual tags measure to), under the store's `rules` (its `tagma.type`
/// declarations), or why the query is malformed. No query selects every
/// candidate; a blank one is refused before it gets here
/// ([`crate::AppError::Blank`]), and tagma judges one that reaches it.
///
/// Each candidate is indexed with its stored tags, less any it holds in
/// the `taskloom` namespace (an old store's: the namespace is the virtual
/// tags', and a write may not fill it), and the virtual tags the query
/// names ([`crate::virtual_tags`]), and no others.
pub fn select<'a>(
    query: Option<&str>,
    candidates: impl IntoIterator<Item = Facts<'a>>,
    now: Timestamp,
    rules: &StoreRules,
) -> Result<BTreeSet<String>, QueryRefusal> {
    let mut index = Index::new();
    index.register_type(SEMVER, Arc::new(Semver));
    // Only the types: an index reads the other facets too (arity collapses
    // a candidate's values, hide narrows what a query sees), and a query
    // is over the tags as stored.
    if let Some(query) = query {
        virtual_tags::check_names(query).map_err(QueryRefusal::UnknownVirtual)?;
    }
    let named = virtual_tags::mentioned(query);
    // A duration literal is resolved to the seconds it asks of the tag.
    let resolved = query
        .map(|query| virtual_tags::resolve_durations(query, now))
        .transpose()
        .map_err(QueryRefusal::Duration)?;
    let types = rules.types();
    if !types.is_empty() {
        index.add_item(TYPE_CONFIG_ITEM, types);
    }
    let mut ids = BTreeSet::new();
    for facts in candidates {
        let mut tags: Vec<tagma_core::Tag> = facts
            .tags
            .iter()
            .map(Tag::to_core)
            .filter(|tag| tag.namespace.as_deref() != Some(TASKLOOM_NAMESPACE))
            .collect();
        tags.push(presence());
        tags.extend(virtual_tags::derive(&facts, &named, now));
        index.add_item(facts.id, tags);
        ids.insert(facts.id.to_string());
    }
    let (Some(query), Some(resolved)) = (query, resolved) else {
        return Ok(ids);
    };
    let matched = index.query_postfix(&resolved).map_err(|postfix| {
        QueryRefusal::Malformed(QueryError {
            query: query.to_string(),
            postfix,
        })
    })?;
    Ok(matched.into_iter().filter(|id| ids.contains(id)).collect())
}

/// Refuse `query` if it is malformed, whatever it would be run over.
pub fn check(query: Option<&str>) -> Result<(), QueryRefusal> {
    select(query, [], Timestamp::default(), &StoreRules::default()).map(|_| ())
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
