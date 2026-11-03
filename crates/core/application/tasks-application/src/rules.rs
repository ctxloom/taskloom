//! The store's own tag rules, as read (docs/plans/self-hosted-schema.md):
//! tagma config tags, which the store's schema stream holds
//! ([`crate::schema`]) and ONLY tagma interprets. Every add or tag is
//! judged by `tagma_core::Rules::validate` ([`judged`]: refused on an
//! `Introduced` finding, a [`RulesRefusal`]; warned on an
//! `Existing` one, [`crate::Written::warnings`]), and `taskloom schema
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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use angzarr_diagnostic::{code, Arg, Diagnostic, Report};

pub use tagma_core::{Code, Finding, Origin};

use tasks::schema::SchemaState;

use crate::{AppError, Shown, Tag};
use tagma_core::semver::{Semver, SEMVER};

/// The namespace taskloom's own schema tags are in: ranking formulas,
/// which tagma's `check` would otherwise call an unknown facet. A task may
/// carry no tag in it, as none in `tagma`.
pub const TASKLOOM_NAMESPACE: &str = "taskloom";

/// tagma's namespace: its facets, every one a config tag.
pub(crate) const TAGMA_NAMESPACE: &str = "tagma";

/// tagma's permit facet: `tagma.permit:<pattern>`, a presence (F6).
const PERMIT: &str = "tagma.permit";

/// The formula facets, taskloom's own: `taskloom.<facet>:<target>=<formula>`.
pub(crate) const PRIORITY_FN: &str = "taskloom.priority_fn";
pub(crate) const DECAY_FN: &str = "taskloom.decay_fn";

/// What a store's schema stream is seeded with when the store is made: the
/// `triage:*` baseline that was `DEFAULT_TAG_SCHEMA`, in tagma's facets
/// (one tag per enum value, `lo..hi` ranges), the formulas in
/// `taskloom.*`, and one `area` and one `repo` a task
/// (`tagma.keys-per-item`, which was lint's `ONE_KEY`); and its permits,
/// for every store is an allowlist from when it is made (the owner's
/// ruling F6, 2026-11-02): each target the seed constrains, and the core
/// vocabulary copied into every project (tag-vocabulary.proposal.md
/// section 2; rulings 13 and 14): the factual flags, `human:`'s keys,
/// `relates:`, `touches:`, `sig:`, `queue:` and `landed:` open, and
/// `platform:`'s keys (not `ctxloom:plan`, which plans retired). A
/// project's own (its `area:` and `repo:` keys) it permits itself.
pub const SEEDED_SCHEMA: [&str; 62] = [
    r#"tagma.arity:"triage:level"=scalar"#,
    r#"tagma.arity:"triage:verdict"=scalar"#,
    r#"tagma.arity:"triage:audited"=scalar"#,
    r#"tagma.arity:"triage:kind"=scalar"#,
    r#"tagma.arity:"triage:effort"=scalar"#,
    r#"tagma.arity:"triage:blocks-release"=scalar"#,
    r#"tagma.type:"triage:blocks-release"=semver"#,
    r#"tagma.enum:"triage:kind"=defect"#,
    r#"tagma.enum:"triage:kind"=capability"#,
    r#"tagma.enum:"triage:kind"=chore"#,
    r#"tagma.enum:"triage:exposed"=wire"#,
    r#"tagma.enum:"triage:exposed"=cli"#,
    r#"tagma.enum:"triage:exposed"=config"#,
    r#"tagma.enum:"triage:exposed"=on-disk"#,
    r#"tagma.enum:"triage:exposed"=api"#,
    r#"tagma.enum:"triage:verdict"=holds"#,
    r#"tagma.enum:"triage:verdict"=phantom"#,
    r#"tagma.enum:"triage:verdict"=obsolete"#,
    r#"tagma.enum:"triage:verdict"=partial"#,
    r#"tagma.enum:"triage:verdict"=unclear"#,
    r#"tagma.enum:"triage:blind-gate"=unit"#,
    r#"tagma.enum:"triage:blind-gate"=integration"#,
    r#"tagma.enum:"triage:blind-gate"=acceptance"#,
    r#"tagma.enum:"triage:blind-gate"=mutation"#,
    r#"tagma.enum:"triage:blind-gate"=lint"#,
    r#"tagma.enum:"triage:blind-gate"=ci"#,
    r#"tagma.range:"triage:level"=1..5"#,
    r#"tagma.range:"triage:audited"=20250101..20991231"#,
    r#"tagma.range:"triage:effort"=0..5"#,
    r#"tagma.keys-per-item:area=1"#,
    r#"tagma.keys-per-item:repo=1"#,
    r#"taskloom.decay_fn:"triage:kind"="{{triage:exposed=*}} > 0 ? 1 + {{age_days}}/({{age_days}}+90) : {{triage:blind-gate=*}} > 0 ? 1 + {{age_days}}/({{age_days}}+30) : {{triage:kind=capability}} > 0 ? 0.4 + 0.6 * 0.5 ** ({{age_days}}/120) : {{triage:kind=chore}} > 0 ? 0.5 + 0.5 * 0.5 ** ({{age_days}}/180) : 1""#,
    r#"taskloom.priority_fn:"triage:kind"="({{triage:level}} > 0 ? 2 ** (3 - {{triage:level}}) : 0.1) * {{age_factor}} * (1 + {{triage:blocks-release=*}}) / (1 + {{triage:effort}}/2)""#,
    r#"tagma.permit:"triage:audited""#,
    r#"tagma.permit:"triage:blind-gate""#,
    r#"tagma.permit:"triage:blocks-release""#,
    r#"tagma.permit:"triage:effort""#,
    r#"tagma.permit:"triage:exposed""#,
    r#"tagma.permit:"triage:kind""#,
    r#"tagma.permit:"triage:level""#,
    r#"tagma.permit:"triage:verdict""#,
    r#"tagma.permit:"triage:security""#,
    r#"tagma.permit:"triage:data-loss""#,
    r#"tagma.permit:"triage:crashes""#,
    r#"tagma.permit:"triage:no-workaround""#,
    r#"tagma.permit:"triage:regression""#,
    r#"tagma.permit:"triage:silent-no-op""#,
    r#"tagma.permit:"triage:flaky""#,
    r#"tagma.permit:"human:decision""#,
    r#"tagma.permit:"human:voice""#,
    r#"tagma.permit:"human:authority""#,
    r#"tagma.permit:"human:verify""#,
    r#"tagma.permit:"human:prerequisite""#,
    r#"tagma.permit:"human:action""#,
    r#"tagma.permit:"relates:*""#,
    r#"tagma.permit:"touches:*""#,
    r#"tagma.permit:"sig:*""#,
    r#"tagma.permit:"queue:*""#,
    r#"tagma.permit:"landed:*""#,
    r#"tagma.permit:"platform:linux""#,
    r#"tagma.permit:"platform:macos""#,
    r#"tagma.permit:"platform:windows""#,
];

/// A store's rules, as read: its schema tags (the seed, where its stream
/// has none), in tagma's type, sorted by their canonical spelling.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct StoreRules {
    tags: Vec<tagma_core::Tag>,
}

impl Default for StoreRules {
    /// The seeded rules: what a store with no schema stream is read as.
    fn default() -> Self {
        StoreRules::of_tags(SEEDED_SCHEMA.iter().map(|spelled| {
            tagma_core::Tag::parse(spelled).expect("the seeded schema is tagma's grammar")
        }))
    }
}

impl StoreRules {
    /// The rules `state` folds to, as every store is read: the seed where
    /// the stream has no history; else its tags, a store's from before
    /// every store was an allowlist read as one ([`StoreRules::allowlisted`]).
    pub(crate) fn of(state: &SchemaState) -> StoreRules {
        match state.declared {
            true => StoreRules::held(state).allowlisted(),
            false => StoreRules::default(),
        }
    }

    /// The tags `state` holds, as it holds them: what [`StoreRules::of`]
    /// reads, before it is read.
    pub(crate) fn held(state: &SchemaState) -> StoreRules {
        StoreRules::of_tags(state.tags.iter().map(Tag::to_core))
    }

    /// These rules as an allowlist's (F6): as they are where a permit is
    /// declared as a presence; else, from before permits were presences,
    /// each `tagma.permit` carrying a value dropped, its `=true` patterns
    /// kept as presences (its `"*:*"=false` closed the store; every store
    /// is closed now), and the seed's permits declared, as a store made
    /// now is.
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
        let seeded = StoreRules::default()
            .facet(PERMIT)
            .cloned()
            .collect::<Vec<_>>();
        StoreRules::of_tags(rules.into_iter().chain(kept).chain(seeded))
    }

    /// The rules `spelled` declare, and nothing else: for tests.
    #[cfg(test)]
    pub(crate) fn parsed(spelled: &[&str]) -> StoreRules {
        StoreRules::of_tags(spelled.iter().map(|s| tagma_core::Tag::parse(s).unwrap()))
    }

    /// The rules `tags` declare, sorted and each once.
    pub(crate) fn of_tags(tags: impl Iterator<Item = tagma_core::Tag>) -> StoreRules {
        let mut tags: Vec<tagma_core::Tag> = tags.collect();
        tags.sort_by_key(ToString::to_string);
        tags.dedup();
        StoreRules { tags }
    }

    /// The tags, sorted by their canonical spelling.
    pub(crate) fn tags(&self) -> &[tagma_core::Tag] {
        &self.tags
    }

    /// The tags, in tagma syntax, sorted: `taskloom manage schema export`.
    pub(crate) fn export(&self) -> Vec<String> {
        self.tags.iter().map(ToString::to_string).collect()
    }

    /// Whether the store holds the config tag `tag`.
    pub(crate) fn holds(&self, tag: &tagma_core::Tag) -> bool {
        self.tags.contains(tag)
    }

    /// tagma's rules over the tags, its `semver` comparator
    /// registered.
    pub(crate) fn rules(&self) -> tagma_core::Rules {
        let mut rules = tagma_core::Rules::from_tags(&self.tags);
        rules.register_type(SEMVER, Arc::new(Semver));
        rules
    }

    /// tagma's `check` over the tags.
    pub(crate) fn check(&self) -> Vec<Finding> {
        self.rules().check()
    }

    /// tagma's findings on a task holding `current`, were `add` added and
    /// then `remove` removed, as a retag applies them: a tag in both ends
    /// up removed. tagma applies a change's removes before its adds, so it
    /// is handed the adds the removes do not take back.
    pub(crate) fn validate(&self, current: &[Tag], add: &[Tag], remove: &[Tag]) -> Vec<Finding> {
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

    /// What a person is shown: tagma's display hiding over the tags.
    pub(crate) fn shown(&self) -> Shown {
        let tags: Vec<Tag> = self.tags.iter().cloned().map(Tag::from).collect();
        Shown::of(&tags)
    }

    /// The `tagma.type` declarations, which a tag query orders by.
    pub(crate) fn types(&self) -> Vec<tagma_core::Tag> {
        self.facet("tagma.type").cloned().collect()
    }

    /// The tags of the facet `namespace`.
    fn facet<'a>(&'a self, namespace: &'a str) -> impl Iterator<Item = &'a tagma_core::Tag> {
        self.tags
            .iter()
            .filter(move |tag| tag.namespace.as_deref() == Some(namespace))
    }

    /// Whether `tag`'s target is declared `scalar`, the restrictive
    /// reading winning: the one thing taskloom reads of a rule itself, to
    /// send a retag with the value a new one displaces already removed
    /// (decide never reads the schema). tagma's own collapse, inside
    /// `validate`, judges the same set.
    pub(crate) fn is_scalar(&self, tag: &Tag) -> bool {
        let target = tag.target();
        self.facet("tagma.arity")
            .any(|arity| arity.key == target && arity.value.as_deref() == Some("scalar"))
    }

    /// The formulas, by target: `taskloom.priority_fn`'s and
    /// `taskloom.decay_fn`'s, which taskloom's evaluator (task-priority)
    /// ranks by. Of two for one target, the last by spelling.
    pub(crate) fn formulas(&self) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let of = |facet| {
            self.facet(facet)
                .filter_map(|tag| Some((tag.key.clone(), tag.value.clone()?)))
                .collect()
        };
        (of(PRIORITY_FN), of(DECAY_FN))
    }

    /// The exact targets the rules constrain (`arity`, `type`, `enum`,
    /// `range`), sorted, each once: what `taskloom manage schema permits list` lists beside
    /// the permits.
    pub(crate) fn targets(&self) -> Vec<String> {
        let exact = ["tagma.arity", "tagma.type", "tagma.enum", "tagma.range"];
        let targets: BTreeSet<String> = exact
            .iter()
            .flat_map(|facet| self.facet(facet))
            .map(|tag| tag.key.clone())
            .collect();
        targets.into_iter().collect()
    }

    /// The patterns `tagma.permit:<pattern>` declares, sorted: what
    /// `taskloom manage schema permits list` lists.
    pub(crate) fn permits(&self) -> Vec<String> {
        self.facet(PERMIT)
            .filter(|tag| tag.value.is_none())
            .map(|tag| tag.key.clone())
            .collect()
    }

    /// How many targets an `enum` or `range` is declared for: what lint
    /// says it checked.
    pub(crate) fn checked_targets(&self) -> usize {
        let targets: BTreeSet<&str> = ["tagma.enum", "tagma.range"]
            .iter()
            .flat_map(|facet| self.facet(facet))
            .map(|tag| tag.key.as_str())
            .collect();
        targets.len()
    }

    /// What `taskloom manage schema show` prints: each declared target and its
    /// facets, as the tags spell them. Display only; tagma judges.
    pub(crate) fn summary(&self) -> SchemaSummary {
        let mut targets: BTreeMap<String, TargetSummary> = BTreeMap::new();
        for tag in &self.tags {
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

/// `tagma.permit:<pattern>`: the permit of `pattern`, a presence.
pub(crate) fn permit_tag(pattern: &str) -> tagma_core::Tag {
    tagma_core::Tag {
        namespace: Some(PERMIT.to_string()),
        key: pattern.to_string(),
        value: None,
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

/// A write tagma's rules refuse: the findings the write would introduce,
/// each its code, its tag and its args (`not-in-enum area:x=b
/// (allowed=a)`), `; `-separated.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RulesRefusal {
    pub findings: String,
}

impl fmt::Display for RulesRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
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
        angzarr_diagnostic::display(self, f)
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
pub(crate) fn spelled_args(args: &[(String, String)]) -> String {
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
pub(crate) fn judged(findings: Vec<Finding>) -> Result<Vec<Finding>, AppError> {
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
    Err(AppError::TagRefused(Report::of(&RulesRefusal { findings })))
}

/// One of every diagnostic, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn samples() -> Vec<Box<dyn Diagnostic>> {
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
