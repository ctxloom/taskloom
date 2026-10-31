//! The store's own tag rules (docs/plans/self-hosted-schema.md).
//!
//! The rules are tagma config tags on the store's schema stream
//! ([`tasks::schema`]), seeded with [`SEEDED_SCHEMA`] when a store is
//! written first (and on the first write to a store with no schema stream),
//! changed only by a person ([`crate::PermitService::declare`], `permit`,
//! `revoke`, `schema import-config`), and interpreted ONLY by tagma: every
//! add or tag is judged by `tagma_core::Rules::validate` (refused on an
//! `Introduced` finding, `tasks-application-tag-rules`; warned on an
//! `Existing` one, [`crate::Written::warnings`]), and `taskloom schema
//! check` is `tagma_core::Rules::check`. No file layer (home, work root,
//! `TASKLOOM_CONFIG_TAG_SCHEMA`, `--config-set`) is read for them: a layer
//! still setting one of the old keys is refused
//! ([`ports::Configuration::moved`]).
//!
//! taskloom keeps only what is its own: storing and seeding the tags, the
//! `taskloom.*` namespace (`taskloom.priority_fn`, `taskloom.decay_fn`:
//! ranking, not constraint, so never tagma's: taskloom's own evaluator,
//! task-priority, reads them), the `semver` comparator it registers with
//! tagma (`Rules::register_type`), `admit`'s reserved-namespace and
//! operator-word checks, which tagma does not make (its `Tag::parse` takes
//! `and` as an ordinary key), and the scalar collapse a retag sends
//! resolved ([`StoreRules::is_scalar`]): tagma applies its §8 collapse
//! inside `validate`, but exposes no call that answers it.
//!
//! A read (a listing, lint, `schema show`) never writes: a store with no
//! schema stream is read as if seeded.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use angzarr_embedded::next::v1::{command_page, Command, CommandBook, CommandPage, Cover};
use angzarr_embedded::next::{Payload, ReadStore, ReadUnit, Registry, Stream, StreamRef};
use angzarr_embedded_diagnostic::{Arg, Diagnostic, Report};
use ports::{Clock, CommandIds, Configuration, Delay, Jitter, ProjectIdentity, TaskStores};
use tasks::schema::{
    schema_root, Declare, SchemaAggregate, SchemaCommand, SchemaState, SCHEMA_DOMAIN,
};

pub use tagma_core::{Code, Finding, Origin};

use crate::retry::{self, Outcome};
use crate::semver::{Semver, SEMVER};
use crate::{failed, rejected, AppError, Project, Service, Shown, Tag};

/// The namespace taskloom's own schema tags are in: ranking formulas,
/// which tagma's `check` would otherwise call an unknown facet. A task may
/// carry no tag in it, as none in `tagma`.
pub const TASKLOOM_NAMESPACE: &str = "taskloom";

/// tagma's namespace: its facets, every one a config tag.
pub(crate) const TAGMA_NAMESPACE: &str = "tagma";

/// The formula facets, taskloom's own: `taskloom.<facet>:<target>=<formula>`.
pub(crate) const PRIORITY_FN: &str = "taskloom.priority_fn";
pub(crate) const DECAY_FN: &str = "taskloom.decay_fn";

/// What a store's schema stream is seeded with when the store is made: the
/// `triage:*` baseline that was `DEFAULT_TAG_SCHEMA`, in tagma's facets
/// (one tag per enum value, `lo..hi` ranges), the formulas in
/// `taskloom.*`, and one `area` and one `repo` a task
/// (`tagma.keys-per-item`, which was lint's `ONE_KEY`). No permit: the
/// world is open (tagma's implicit `"*:*"=true`) until a first permit.
pub const SEEDED_SCHEMA: [&str; 33] = [
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
];

/// Schema tags to add and remove, in tagma's grammar: `taskloom schema
/// declare --add/--remove`. Each must be in `tagma.*` or `taskloom.*`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Declaration {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// What a declare (or a permit, or a revoke) left on the schema stream:
/// its tags after, in tagma syntax, sorted, and whether they changed.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Declared {
    pub tags: Vec<String>,
    pub changed: bool,
}

/// Reading the store's rules: an agent may (`taskloom schema
/// show/export/check`). Changing them is [`crate::PermitService`]'s, a
/// person's.
pub trait SchemaService {
    /// The store's schema tags in tagma syntax, one a line, sorted: the
    /// work root's project's store, seeded first where it has no schema
    /// stream yet (a store made before this change).
    fn schema_export(
        &mut self,
        project: Option<String>,
    ) -> impl Future<Output = Result<Vec<String>, AppError>> + Send;

    /// tagma's `check` over the store's schema tags, every finding
    /// `Existing`.
    fn schema_check(
        &mut self,
        project: Option<String>,
    ) -> impl Future<Output = Result<Vec<Finding>, AppError>> + Send;
}

impl crate::Written {
    /// tagma's `Existing` findings on the task written: what was wrong
    /// before the write and still is. A write is never refused for these
    /// (an `Introduced` finding refuses it, `tasks-application-tag-rules`),
    /// and the edge prints them on stderr, or in the JSON document.
    pub fn warnings(&self) -> &[Finding] {
        &self.warnings
    }
}

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
    /// The rules `state` folds to: its tags once the stream is declared,
    /// the seed before.
    pub(crate) fn of(state: &SchemaState) -> StoreRules {
        match state.declared {
            true => StoreRules::of_tags(state.tags.iter().map(Tag::to_core)),
            false => StoreRules::default(),
        }
    }

    /// The rules `spelled` declare, and nothing else: for tests.
    #[cfg(test)]
    pub(crate) fn parsed(spelled: &[&str]) -> StoreRules {
        StoreRules::of_tags(spelled.iter().map(|s| tagma_core::Tag::parse(s).unwrap()))
    }

    fn of_tags(tags: impl Iterator<Item = tagma_core::Tag>) -> StoreRules {
        let mut tags: Vec<tagma_core::Tag> = tags.collect();
        tags.sort_by_key(ToString::to_string);
        tags.dedup();
        StoreRules { tags }
    }

    /// The rules with `tags` declared beside them.
    pub(crate) fn extend(&mut self, tags: &[tagma_core::Tag]) {
        *self = StoreRules::of_tags(self.tags.drain(..).chain(tags.iter().cloned()));
    }

    /// The tags, in tagma syntax, sorted: `taskloom schema export`.
    pub(crate) fn export(&self) -> Vec<String> {
        self.tags.iter().map(ToString::to_string).collect()
    }

    /// Whether the store holds the config tag `tag`.
    pub(crate) fn holds(&self, tag: &tagma_core::Tag) -> bool {
        self.tags.contains(tag)
    }

    /// tagma's rules over the tags, taskloom's `semver` comparator
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
    /// `range`), sorted, each once: what a first permit keeps admitted.
    pub(crate) fn targets(&self) -> Vec<String> {
        let exact = ["tagma.arity", "tagma.type", "tagma.enum", "tagma.range"];
        let targets: BTreeSet<String> = exact
            .iter()
            .flat_map(|facet| self.facet(facet))
            .map(|tag| tag.key.clone())
            .collect();
        targets.into_iter().collect()
    }

    /// The patterns `tagma.permit:<pattern>=true` declares, sorted: what
    /// `taskloom permits` lists.
    pub(crate) fn permits(&self) -> Vec<String> {
        self.facet("tagma.permit")
            .filter(|tag| tag.value.as_deref() == Some("true"))
            .map(|tag| tag.key.clone())
            .collect()
    }

    /// Whether the world is closed: `tagma.permit:"*:*"=false` declared.
    pub(crate) fn closed(&self) -> bool {
        self.holds(&closing())
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

    /// What `taskloom schema show` prints: each declared target and its
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

/// The tag that closes the world: `tagma.permit:"*:*"=false`.
pub(crate) fn closing() -> tagma_core::Tag {
    permit_tag("*:*", false)
}

/// `tagma.permit:<pattern>=<admitted>`.
pub(crate) fn permit_tag(pattern: &str, admitted: bool) -> tagma_core::Tag {
    tagma_core::Tag {
        namespace: Some("tagma.permit".to_string()),
        key: pattern.to_string(),
        value: Some(admitted.to_string()),
    }
}

/// The store's rules as `taskloom schema show` prints them: each target
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
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for RulesRefusal {
    fn code(&self) -> &str {
        "tasks-application-tag-rules"
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
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for RuleWarning {
    fn code(&self) -> &str {
        "tasks-application-tag-rule-warning"
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

/// The schema stream's cover.
fn schema_cover() -> Cover {
    Cover {
        domain: SCHEMA_DOMAIN.to_string(),
        root: Some(schema_root()),
        edition: None,
    }
}

/// `command` for the schema stream.
fn schema_book(command: SchemaCommand) -> CommandBook {
    CommandBook {
        cover: Some(schema_cover()),
        page: Some(CommandPage {
            body: Some(command_page::Body::Command(Command {
                payload: Some(command.to_any()),
            })),
            ..CommandPage::default()
        }),
        handling: None,
    }
}

/// The schema `store` holds, read in a read-only unit: no history, an
/// undeclared one.
pub(crate) async fn schema_in<St: ReadStore>(
    store: &St,
    registry: &Registry,
) -> Result<SchemaState, AppError> {
    let cover = schema_cover();
    let stream = StreamRef::try_from(&cover).map_err(failed)?;
    let mut reads = store.begin_read().await.map_err(failed)?;
    let slice = reads.read(stream, 0, None).await.map_err(failed)?;
    let replayed = Stream::<SchemaAggregate>::replay(cover, &slice.pages, registry)
        .map_err(|e| AppError::Failed(Report::of(&e)))?;
    Ok(replayed.state().state().clone())
}

/// `store`'s rules after seeding its schema stream, where it has no
/// history, with [`SEEDED_SCHEMA`]: a stream already declared is read and
/// nothing is sent. The seed is sent at the head read (empty), so two
/// writers seeding at once land it once, the second finding it there on
/// its retry.
pub(crate) async fn seed_in<St, C, D>(
    runner: &mut retry::Runner<'_, C, D>,
    store: &St,
) -> Result<StoreRules, AppError>
where
    St: angzarr_embedded::next::Store + ReadStore,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    let held = schema_in(store, runner.registry).await?;
    if held.declared {
        return Ok(StoreRules::of(&held));
    }
    let seed = schema_book(
        Declare {
            add: StoreRules::default()
                .tags
                .into_iter()
                .map(Tag::from)
                .collect(),
            remove: Vec::new(),
        }
        .into(),
    );
    let unseeded = |schema: &SchemaAggregate| (!schema.state().declared).then(|| seed.clone());
    let command_id = runner.clock.command_id();
    if let Some(Outcome::Refused(rejection)) = runner
        .run_shaped_on::<SchemaAggregate, _, _>(store, &seed, &command_id, Some(&unseeded))
        .await?
    {
        return Err(rejected(&rejection));
    }
    Ok(StoreRules::of(&schema_in(store, runner.registry).await?))
}

/// Each of `spelled` parsed as a schema tag: refused at the first that is
/// not a tag in tagma's grammar, or is blank.
fn parsed(spelled: &[String]) -> Result<Vec<Tag>, AppError> {
    crate::tags::parse_all(spelled).map_err(|r| crate::refused(&r))
}

impl<I, S, N, C, D, G: Configuration> Service<I, S, N, C, D, G> {
    /// Refused where a configuration layer still sets a key that moved
    /// into the store ([`ports::Configuration::moved`]).
    pub(crate) fn refuse_moved(&mut self) -> Result<(), AppError> {
        self.configuration.moved().map_err(AppError::Configuration)
    }
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    S: TaskStores,
{
    /// The rules of `project`'s store, read and never written: the seed
    /// where it has no store or no schema stream yet.
    pub(crate) async fn rules_of(&mut self, project: &Project) -> Result<StoreRules, AppError> {
        let Some(store) = self.stores.open_existing(project).await.map_err(failed)? else {
            return Ok(StoreRules::default());
        };
        Ok(StoreRules::of(&schema_in(store, &self.registry).await?))
    }
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    I: ProjectIdentity,
    S: TaskStores,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    /// `project`'s store's rules after seeding its schema stream, where it
    /// has no history ([`seed_in`]).
    pub(crate) async fn seeded(&mut self, project: &Project) -> Result<StoreRules, AppError> {
        let store = self.stores.open(project).await.map_err(failed)?;
        let mut runner = retry::Runner {
            registry: &self.registry,
            clock: &mut self.clock,
            delay: &mut self.delay,
            backoff: self.backoff,
            dead_letters: &self.dead_letters,
        };
        seed_in(&mut runner, store).await
    }

    /// `declare` on `project`'s store's schema stream, seeded first: the
    /// tags after it, and whether they changed.
    pub(crate) async fn declaring(
        &mut self,
        project: &Project,
        declare: Declare,
    ) -> Result<Declared, AppError> {
        tasks::schema::check(&declare).map_err(|r| rejected(&r))?;
        let before = self.seeded(project).await?;
        let store = self.stores.open(project).await.map_err(failed)?;
        let book = schema_book(declare.into());
        let command_id = self.clock.command_id();
        let mut runner = retry::Runner {
            registry: &self.registry,
            clock: &mut self.clock,
            delay: &mut self.delay,
            backoff: self.backoff,
            dead_letters: &self.dead_letters,
        };
        let unshaped: Option<&fn(&SchemaAggregate) -> Option<CommandBook>> = None;
        if let Some(Outcome::Refused(rejection)) = runner
            .run_shaped_on::<SchemaAggregate, _, _>(store, &book, &command_id, unshaped)
            .await?
        {
            return Err(rejected(&rejection));
        }
        let after = StoreRules::of(&schema_in(store, &self.registry).await?);
        Ok(Declared {
            changed: after != before,
            tags: after.export(),
        })
    }

    /// A person's [`Declaration`], parsed, on the work root's project's
    /// store.
    pub(crate) async fn declared(
        &mut self,
        declaration: Declaration,
    ) -> Result<Declared, AppError> {
        let declare = Declare {
            add: parsed(&declaration.add)?,
            remove: parsed(&declaration.remove)?,
        };
        tasks::schema::check(&declare).map_err(|r| rejected(&r))?;
        let project = self.identity.for_write(None).map_err(AppError::Identity)?;
        self.declaring(&project, declare).await
    }
}

impl<I, S, N, C, D, G> Service<I, S, N, C, D, G>
where
    I: ProjectIdentity,
    S: TaskStores,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
    G: Configuration,
{
    /// The rules of the store `pinned` (or the work root) names, refused
    /// first where a layer still sets a moved key; the seed where there is
    /// no project or no store.
    pub(crate) async fn rules_for(&mut self, pinned: Option<&str>) -> Result<StoreRules, AppError> {
        self.refuse_moved()?;
        match self.identity.for_read(pinned).map_err(AppError::Identity)? {
            Some(project) => self.rules_of(&project).await,
            None => Ok(StoreRules::default()),
        }
    }
}

impl<I, S, N, C, D, G> SchemaService for Service<I, S, N, C, D, G>
where
    I: ProjectIdentity + Send,
    S: TaskStores + Send,
    N: Send,
    C: Clock + CommandIds + Send,
    D: Delay + Jitter + Send,
    G: Configuration + Send,
{
    async fn schema_export(&mut self, project: Option<String>) -> Result<Vec<String>, AppError> {
        self.refuse_moved()?;
        let Some(found) = self
            .identity
            .for_read(project.as_deref())
            .map_err(AppError::Identity)?
        else {
            return Ok(StoreRules::default().export());
        };
        if self
            .stores
            .open_existing(&found)
            .await
            .map_err(failed)?
            .is_none()
        {
            return Ok(StoreRules::default().export());
        }
        Ok(self.seeded(&found).await?.export())
    }

    async fn schema_check(&mut self, project: Option<String>) -> Result<Vec<Finding>, AppError> {
        Ok(self.rules_for(project.as_deref()).await?.check())
    }
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

#[cfg(test)]
#[path = "store_schema.test.rs"]
mod tests;
