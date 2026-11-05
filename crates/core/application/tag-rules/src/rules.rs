//! The store's own tag rules, as read (docs/plans/self-hosted-schema.md):
//! tagma config tags, which the store's schema stream holds
//! (`tasks-application`'s `schema`) and ONLY tagma interprets. Every add or tag is
//! judged by `tagma_core::Rules::validate` ([`judged`]: refused on an
//! `Introduced` finding, a [`RulesRefusal`]; warned on an
//! `Existing` one, `tasks-application`'s `Written::warnings`), and `taskloom schema
//! check` is `tagma_core::Rules::check`. Pure: what the store holds is
//! read elsewhere, and handed in.
//!
//! taskloom keeps only what is its own: the seed ([`SEEDED_SCHEMA`]: every
//! store an allowlist from when it is made, F6), the
//! `taskloom.*` namespace (`taskloom.priority_fn`, `taskloom.decay_fn`:
//! ranking, not constraint, so never tagma's: taskloom's own evaluator,
//! task-priority, reads them), the `semver` comparator it registers with
//! tagma (`Rules::register_type`), and the scalar collapse a retag sends
//! resolved ([`StoreRules::is_scalar`]): tagma applies its §8 collapse
//! inside `validate`, but exposes no call that answers it. `admit`'s
//! reserved-namespace and operator-word checks, which tagma does not make
//! (its `Tag::parse` takes `and` as an ordinary key), are [`crate::tags`]'.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use coded_diagnostic::{code, Arg, Diagnostic};

pub use tagma_core::{Code, Finding, Origin};

use tasks::schema::SchemaState;
use tasks::Tag;

use tagma_core::semver::{Semver, SEMVER};

use crate::template::{Template, CTXLOOM_SCHEMA, GENERAL_SCHEMA};

/// The namespace taskloom's own schema tags are in: ranking formulas,
/// which tagma's `check` would otherwise call an unknown facet. A task may
/// carry no tag in it, as none in `tagma`.
pub const TASKLOOM_NAMESPACE: &str = "taskloom";

/// tagma's namespace: its facets, every one a config tag.
pub(crate) const TAGMA_NAMESPACE: &str = "tagma";

/// tagma's permit facet: `tagma.permit:<pattern>`, a presence (F6).
pub const PERMIT: &str = "tagma.permit";

/// What a store's schema stream is seeded with when the store is made: the
/// GENERAL default set ([`crate::template::GENERAL_SCHEMA`]), small and
/// project-neutral. The ctxloom project's own set is the named template
/// [`crate::template::CTXLOOM_SCHEMA`]. Every store is an allowlist from
/// when it is made (the owner's ruling F6, 2026-11-02).
pub const SEEDED_SCHEMA: [&str; GENERAL_SCHEMA.len()] = GENERAL_SCHEMA;

/// A store's rules, as read: its schema tags (the seed, where its stream
/// has none), in tagma's type, sorted by their canonical spelling.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StoreRules {
    tags: Vec<tagma_core::Tag>,
}

impl Default for StoreRules {
    /// The seeded rules: what a store with no schema stream is read as.
    fn default() -> Self {
        StoreRules::of_template(Template::default())
    }
}

impl StoreRules {
    /// The rules `template` declares, as they are before any is changed.
    pub fn of_template(template: Template) -> StoreRules {
        StoreRules::of_tags(
            template
                .schema()
                .iter()
                .map(|spelled| crate::template::parsed(spelled)),
        )
    }

    /// The rules `state` folds to, as every store is read: the seed where
    /// the stream has no history; else its tags, a store's from before
    /// every store was an allowlist read as one ([`StoreRules::allowlisted`]).
    pub fn of(state: &SchemaState) -> StoreRules {
        match state.declared {
            true => StoreRules::held(state).allowlisted(),
            false => StoreRules::default(),
        }
    }

    /// The tags `state` holds, as it holds them: what [`StoreRules::of`]
    /// reads, before it is read.
    pub fn held(state: &SchemaState) -> StoreRules {
        StoreRules::of_tags(state.tags.iter().map(Tag::to_core))
    }

    /// These rules as an allowlist's (F6): as they are where a permit is
    /// declared as a presence; else, from before permits were presences,
    /// each `tagma.permit` carrying a value dropped, its `=true` patterns
    /// kept as presences (its `"*:*"=false` closed the store; every store
    /// is closed now), and the permits every store was seeded with then
    /// declared: the ctxloom template's ([`CTXLOOM_SCHEMA`]), pinned, not
    /// the default of now.
    fn allowlisted(self) -> StoreRules {
        if self.facet(PERMIT).any(|tag| tag.value.is_none()) {
            return self;
        }
        let (valued, rules): (Vec<_>, Vec<_>) = self
            .tags
            .into_iter()
            .partition(|tag| tag.namespace.as_deref() == Some(PERMIT));
        let kept = valued
            .into_iter()
            .filter(|tag| tag.value.as_deref() == Some("true"))
            .map(|tag| permit_tag(&tag.key));
        // The permits of the set every store was seeded with then (the
        // ctxloom template), not the default of now: a store from before
        // F6 keeps the vocabulary it was made with.
        let seeded = CTXLOOM_SCHEMA
            .iter()
            .map(|spelled| crate::template::parsed(spelled))
            .filter(|tag| tag.namespace.as_deref() == Some(PERMIT))
            .collect::<Vec<_>>();
        StoreRules::of_tags(rules.into_iter().chain(kept).chain(seeded))
    }

    /// The rules `spelled` declare, and nothing else: for tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn parsed(spelled: &[&str]) -> StoreRules {
        StoreRules::of_tags(spelled.iter().map(|s| tagma_core::Tag::parse(s).unwrap()))
    }

    /// The rules `tags` declare, sorted and each once.
    pub fn of_tags(tags: impl Iterator<Item = tagma_core::Tag>) -> StoreRules {
        let mut tags: Vec<tagma_core::Tag> = tags.collect();
        tags.sort_by_key(ToString::to_string);
        tags.dedup();
        StoreRules { tags }
    }

    /// The tags, sorted by their canonical spelling.
    pub fn tags(&self) -> &[tagma_core::Tag] {
        &self.tags
    }

    /// The tags, in tagma syntax, sorted: `taskloom manage schema export`.
    pub fn export(&self) -> Vec<String> {
        self.tags.iter().map(ToString::to_string).collect()
    }

    /// Whether the store holds the config tag `tag`.
    pub fn holds(&self, tag: &tagma_core::Tag) -> bool {
        self.tags.contains(tag)
    }

    /// tagma's rules over the tags, its `semver` comparator
    /// registered.
    pub fn rules(&self) -> tagma_core::Rules {
        let mut rules = tagma_core::Rules::from_tags(&self.tags);
        rules.register_type(SEMVER, Arc::new(Semver));
        rules
    }

    /// tagma's `check` over the tags.
    pub fn check(&self) -> Vec<Finding> {
        self.rules().check()
    }

    /// tagma's findings on a task holding `current`, were `add` added and
    /// then `remove` removed, as a retag applies them: a tag in both ends
    /// up removed. tagma applies a change's removes before its adds, so it
    /// is handed the adds the removes do not take back.
    pub fn validate(&self, current: &[Tag], add: &[Tag], remove: &[Tag]) -> Vec<Finding> {
        let core = |tags: &[Tag]| tags.iter().map(Tag::to_core).collect::<Vec<_>>();
        let remove = core(remove);
        let change = tagma_core::Change {
            add: core(add)
                .into_iter()
                .filter(|tag| !remove.contains(tag))
                .collect(),
            remove,
        };
        self.rules().validate(&core(current), &change)
    }

    /// The `tagma.type` declarations, which a tag query orders by.
    pub fn types(&self) -> Vec<tagma_core::Tag> {
        self.facet("tagma.type").cloned().collect()
    }

    /// The tags of the facet `namespace`.
    pub fn facet<'a>(&'a self, namespace: &'a str) -> impl Iterator<Item = &'a tagma_core::Tag> {
        self.tags
            .iter()
            .filter(move |tag| tag.namespace.as_deref() == Some(namespace))
    }

    /// How many targets an `enum` or `range` is declared for: what lint
    /// says it checked.
    pub fn checked_targets(&self) -> usize {
        let targets: BTreeSet<&str> = ["tagma.enum", "tagma.range"]
            .iter()
            .flat_map(|facet| self.facet(facet))
            .map(|tag| tag.key.as_str())
            .collect();
        targets.len()
    }
}

/// `tagma.permit:<pattern>`: the permit of `pattern`, a presence.
pub fn permit_tag(pattern: &str) -> tagma_core::Tag {
    tagma_core::Tag {
        namespace: Some(PERMIT.to_string()),
        key: pattern.to_string(),
        value: None,
    }
}

/// A write tagma's rules refuse: the findings the write would introduce,
/// each its code, its tag and its args (`not-in-enum area:x=b
/// (allowed=a)`), `; `-separated.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RulesRefusal {
    pub findings: String,
}

impl fmt::Display for RulesRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl Diagnostic for RulesRefusal {
    fn code(&self) -> &str {
        code!("tag-rules")
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        vec![("findings", self.findings.as_str().into())]
    }
}

/// An `Existing` finding on a task written, as the edge says it on stderr.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RuleWarning(pub Finding);

impl fmt::Display for RuleWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl Diagnostic for RuleWarning {
    fn code(&self) -> &str {
        code!("tag-rule-warning")
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        vec![
            ("rule", self.0.code.as_str().into()),
            ("tag", self.0.tag.to_string().into()),
            ("args", spelled_args(&self.0.args).into()),
        ]
    }
}

/// `args` as `name=value, ...`; none, `-`.
pub fn spelled_args(args: &[(String, String)]) -> String {
    match args.is_empty() {
        true => "-".to_string(),
        false => args
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// `finding` as a refusal spells it: code, tag, and its args in brackets.
fn spelled(finding: &Finding) -> String {
    match finding.args.is_empty() {
        true => format!("{} {}", finding.code.as_str(), finding.tag),
        false => format!(
            "{} {} ({})",
            finding.code.as_str(),
            finding.tag,
            spelled_args(&finding.args)
        ),
    }
}

/// `findings` split: the write refused on any `Introduced` one, else its
/// `Existing` ones, to warn of.
pub fn judged(findings: Vec<Finding>) -> Result<Vec<Finding>, RulesRefusal> {
    let (introduced, existing): (Vec<Finding>, Vec<Finding>) = findings
        .into_iter()
        .partition(|f| f.origin == Origin::Introduced);
    if introduced.is_empty() {
        return Ok(existing);
    }
    let findings = introduced
        .iter()
        .map(spelled)
        .collect::<Vec<_>>()
        .join("; ");
    Err(RulesRefusal { findings })
}

/// One of every diagnostic, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(RulesRefusal {
            findings: "not-in-enum area:x=b (allowed=a)".to_string(),
        }),
        Box::new(RuleWarning(Finding {
            code: tagma_core::Code::TooManyKeys,
            tag: tagma_core::Tag::parse("lang:en").unwrap(),
            args: vec![
                ("namespace".to_string(), "lang".to_string()),
                ("limit".to_string(), "1".to_string()),
            ],
            origin: Origin::Existing,
        })),
    ]
}

#[cfg(test)]
#[path = "rules.test.rs"]
mod tests;
