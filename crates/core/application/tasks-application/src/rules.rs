//! The store's own tag rules, read by tasks-application: `tag-rules`' pure
//! [`StoreRules`] (what tagma judges), and what only a use case reads of
//! them: the scalar targets a retag collapses by, the formulas ranking
//! reads, the permits and targets listed, the summary shown and the tags
//! a person sees ([`StoreRulesExt`]).

use std::collections::{BTreeMap, BTreeSet};

pub use tag_rules::rules::{
    judged, permit_tag, spelled_args, Code, Finding, Origin, RuleWarning, RulesRefusal, StoreRules,
    PERMIT, SEEDED_SCHEMA, TASKLOOM_NAMESPACE,
};

use crate::{Shown, Tag};

/// The formula facets, taskloom's own: `taskloom.<facet>:<target>=<formula>`.
pub(crate) const PRIORITY_FN: &str = "taskloom.priority_fn";
pub(crate) const DECAY_FN: &str = "taskloom.decay_fn";

/// What a use case reads of [`StoreRules`] beyond tagma's judgement.
pub(crate) trait StoreRulesExt {
    /// What a person is shown: tagma's display hiding over the tags.
    fn shown(&self) -> Shown;

    /// Whether `tag`'s target is declared `scalar`, the restrictive
    /// reading winning: the one thing taskloom reads of a rule itself, to
    /// send a retag with the value a new one displaces already removed
    /// (decide never reads the schema). tagma's own collapse, inside
    /// `validate`, judges the same set.
    fn is_scalar(&self, tag: &Tag) -> bool;

    /// The formulas, by target: `taskloom.priority_fn`'s and
    /// `taskloom.decay_fn`'s, which taskloom's evaluator (task-priority)
    /// ranks by. Of two for one target, the last by spelling.
    fn formulas(&self) -> (BTreeMap<String, String>, BTreeMap<String, String>);

    /// The exact targets the rules constrain (`arity`, `type`, `enum`,
    /// `range`), sorted, each once: what `taskloom manage schema permits list` lists beside
    /// the permits.
    fn targets(&self) -> Vec<String>;

    /// The patterns `tagma.permit:<pattern>` declares, sorted: what
    /// `taskloom manage schema permits list` lists.
    fn permits(&self) -> Vec<String>;

    /// What `taskloom manage schema show` prints: each declared target and its
    /// facets, as the tags spell them. Display only; tagma judges.
    fn summary(&self) -> SchemaSummary;
}

impl StoreRulesExt for StoreRules {
    fn shown(&self) -> Shown {
        let tags: Vec<Tag> = self.tags().iter().cloned().map(Tag::from).collect();
        Shown::of(&tags)
    }

    fn is_scalar(&self, tag: &Tag) -> bool {
        let target = tag.target();
        self.facet("tagma.arity")
            .any(|arity| arity.key == target && arity.value.as_deref() == Some("scalar"))
    }

    fn formulas(&self) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let of = |facet| {
            self.facet(facet)
                .filter_map(|tag| Some((tag.key.clone(), tag.value.clone()?)))
                .collect()
        };
        (of(PRIORITY_FN), of(DECAY_FN))
    }

    fn targets(&self) -> Vec<String> {
        let exact = ["tagma.arity", "tagma.type", "tagma.enum", "tagma.range"];
        let targets: BTreeSet<String> = exact
            .iter()
            .flat_map(|facet| self.facet(facet))
            .map(|tag| tag.key.clone())
            .collect();
        targets.into_iter().collect()
    }

    fn permits(&self) -> Vec<String> {
        self.facet(PERMIT)
            .filter(|tag| tag.value.is_none())
            .map(|tag| tag.key.clone())
            .collect()
    }

    fn summary(&self) -> SchemaSummary {
        let mut targets: BTreeMap<String, TargetSummary> = BTreeMap::new();
        for tag in self.tags() {
            let (Some(facet), Some(value)) = (tag.namespace.as_deref(), tag.value.clone()) else {
                continue;
            };
            let known = [
                "tagma.arity",
                "tagma.enum",
                "tagma.range",
                "tagma.type",
                PRIORITY_FN,
                DECAY_FN,
            ];
            if !known.contains(&facet) {
                continue;
            }
            let summary = targets
                .entry(tag.key.clone())
                .or_insert_with(|| TargetSummary {
                    target: tag.key.clone(),
                    ..TargetSummary::default()
                });
            match facet {
                "tagma.arity" => summary.scalar |= value == "scalar",
                "tagma.enum" => summary.members.push(value),
                "tagma.range" => summary.ranges.push(value),
                "tagma.type" => summary.kind = value,
                PRIORITY_FN => summary.priority_fn = value,
                _ => summary.decay_fn = value,
            }
        }
        SchemaSummary {
            targets: targets.into_values().collect(),
        }
    }
}

/// The store's rules as `taskloom manage schema show` prints them: each target
/// a facet is declared for, by target.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SchemaSummary {
    pub targets: Vec<TargetSummary>,
}

/// One target's facets, as declared: an undeclared one empty.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TargetSummary {
    pub target: String,
    /// Declared `tagma.arity` `scalar`.
    pub scalar: bool,
    /// Its `tagma.enum` values, sorted.
    pub members: Vec<String>,
    /// Its `tagma.range` declarations, as spelled (`1..5`), sorted.
    pub ranges: Vec<String>,
    /// Its `tagma.type`.
    pub kind: String,
    pub priority_fn: String,
    pub decay_fn: String,
}
