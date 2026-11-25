//! The named tag-rule templates a store can be made with, and what
//! `taskloom manage schema init` does with one: [`GENERAL_SCHEMA`] (the
//! default every new store is seeded with), [`CTXLOOM_SCHEMA`] (the ctxloom
//! project's own set, which was the default before the general one) and
//! the pure step from a template, and what a store holds, to the tags that
//! are missing ([`wanted`], [`missing`]). Pure: what a store holds is
//! handed in.

use crate::rules::{permit_tag, StoreRules, PERMIT};

/// The open `area:` permit: any key of the namespace.
const AREA_OPEN: &str = "area:*";

/// The facets a target holds ONE of: a different value declared for the
/// same target is a rule in conflict, not one more.
const SINGLE: [&str; 6] = [
    "tagma.arity",
    "tagma.type",
    "tagma.range",
    "tagma.keys-per-item",
    "taskloom.priority_fn",
    "taskloom.decay_fn",
];

/// The GENERAL default tag set, small and project-neutral (the owner's
/// ruling): `triage:level` (scalar 1..5), `kind` (scalar: defect,
/// capability, chore), `effort` (scalar 0..5), `blocks-release` (scalar
/// semver) and `verdict` (scalar: holds, obsolete, partial, phantom,
/// unclear); one `area` a task, its keys OPEN (a project that wants them
/// closed lists them: `manage schema init --area`); `relates:`, `touches:`
/// and `queue:` open; `human:`'s keys closed; the factual flags
/// closed. `taskloom.priority_fn` ranks by level, age and effort and there
/// is no `decay_fn`. Also in it, as the default schema has them (the
/// owner's rulings of 2026-11-12 and 2026-11-24): the parking flags
/// `icebox` and `someday-maybe`, and `review:` open with its
/// `review:illuminated` stamp (`YYYYMMDDHHMM`, a scalar held to its range). NOT in it: `triage:exposed`, `blind-gate`, `audited`,
/// `landed:`, `repo:`, `platform:`, `sig:` and the ctxloom decay formula.
pub const GENERAL_SCHEMA: [&str; 45] = [
    r#"tagma.arity:"triage:level"=scalar"#,
    r#"tagma.arity:"triage:kind"=scalar"#,
    r#"tagma.arity:"triage:effort"=scalar"#,
    r#"tagma.arity:"triage:blocks-release"=scalar"#,
    r#"tagma.arity:"triage:verdict"=scalar"#,
    r#"tagma.type:"triage:blocks-release"=semver"#,
    r#"tagma.enum:"triage:kind"=defect"#,
    r#"tagma.enum:"triage:kind"=capability"#,
    r#"tagma.enum:"triage:kind"=chore"#,
    r#"tagma.enum:"triage:verdict"=holds"#,
    r#"tagma.enum:"triage:verdict"=obsolete"#,
    r#"tagma.enum:"triage:verdict"=partial"#,
    r#"tagma.enum:"triage:verdict"=phantom"#,
    r#"tagma.enum:"triage:verdict"=unclear"#,
    r#"tagma.range:"triage:level"=1..5"#,
    r#"tagma.range:"triage:effort"=0..5"#,
    r#"tagma.keys-per-item:area=1"#,
    r#"taskloom.priority_fn:"triage:kind"="({{triage:level}} > 0 ? 2 ** (3 - {{triage:level}}) : 0.1) * (1 + {{age_days}}/({{age_days}}+90)) / (1 + {{triage:effort}}/2)""#,
    r#"tagma.permit:"triage:level""#,
    r#"tagma.permit:"triage:kind""#,
    r#"tagma.permit:"triage:effort""#,
    r#"tagma.permit:"triage:blocks-release""#,
    r#"tagma.permit:"triage:verdict""#,
    r#"tagma.permit:"triage:security""#,
    r#"tagma.permit:"triage:data-loss""#,
    r#"tagma.permit:"triage:crashes""#,
    r#"tagma.permit:"triage:no-workaround""#,
    r#"tagma.permit:"triage:regression""#,
    r#"tagma.permit:"triage:silent-no-op""#,
    r#"tagma.permit:"triage:flaky""#,
    r#"tagma.permit:"human:decision""#,
    r#"tagma.permit:"human:action""#,
    r#"tagma.permit:"human:authority""#,
    r#"tagma.permit:"human:verify""#,
    r#"tagma.permit:"human:prerequisite""#,
    r#"tagma.permit:"human:voice""#,
    r#"tagma.permit:"relates:*""#,
    r#"tagma.permit:"touches:*""#,
    r#"tagma.permit:"queue:*""#,
    r#"tagma.permit:"area:*""#,
    r#"tagma.arity:"review:illuminated"=scalar"#,
    r#"tagma.range:"review:illuminated"=202501010000..209912312359"#,
    r#"tagma.permit:icebox"#,
    r#"tagma.permit:someday-maybe"#,
    r#"tagma.permit:"review:*""#,
];

/// The ctxloom project's tag set: what every store was seeded with before
/// the general default, as the seed is today (the `triage:*` baseline in tagma's facets, the
/// decay formula, one `area` and one `repo` a task, and its permits: each
/// target it constrains and the core vocabulary copied into every
/// ctxloom project: the factual flags, `human:`'s keys, `relates:`,
/// `touches:`, `sig:`, `queue:` and `landed:` open, `platform:`'s keys).
/// A project's own `area:` and `repo:` keys it permits itself. A store
/// from before the general default is read as it was; this is that set.
pub const CTXLOOM_SCHEMA: [&str; 67] = [
    r#"tagma.arity:"triage:level"=scalar"#,
    r#"tagma.arity:"triage:verdict"=scalar"#,
    r#"tagma.arity:"triage:audited"=scalar"#,
    r#"tagma.arity:"triage:kind"=scalar"#,
    r#"tagma.arity:"triage:effort"=scalar"#,
    r#"tagma.arity:"triage:blocks-release"=scalar"#,
    r#"tagma.arity:"review:illuminated"=scalar"#,
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
    r#"tagma.range:"review:illuminated"=202501010000..209912312359"#,
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
    r#"tagma.permit:icebox"#,
    r#"tagma.permit:someday-maybe"#,
    r#"tagma.permit:"relates:*""#,
    r#"tagma.permit:"touches:*""#,
    r#"tagma.permit:"sig:*""#,
    r#"tagma.permit:"queue:*""#,
    r#"tagma.permit:"landed:*""#,
    r#"tagma.permit:"review:*""#,
    r#"tagma.permit:"platform:linux""#,
    r#"tagma.permit:"platform:macos""#,
    r#"tagma.permit:"platform:windows""#,
];

/// A named tag-rule template: `taskloom manage schema init --template`.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Template {
    /// [`GENERAL_SCHEMA`]: the default.
    #[default]
    General,
    /// [`CTXLOOM_SCHEMA`].
    Ctxloom,
}

impl Template {
    /// The name `--template` takes.
    pub fn name(self) -> &'static str {
        match self {
            Template::General => "general",
            Template::Ctxloom => "ctxloom",
        }
    }

    /// The template's tags, spelled in tagma's grammar.
    pub fn schema(self) -> &'static [&'static str] {
        match self {
            Template::General => &GENERAL_SCHEMA,
            Template::Ctxloom => &CTXLOOM_SCHEMA,
        }
    }
}

/// `spelled` in tagma's grammar, for a spelling in this module.
pub(crate) fn parsed(spelled: &str) -> tagma_core::Tag {
    tagma_core::Tag::parse(spelled).expect("a template is tagma's grammar")
}

/// The permit patterns of `rules` for the namespace `area`.
fn area_permits(rules: &StoreRules) -> Vec<&str> {
    rules
        .facet(PERMIT)
        .filter(|tag| tag.value.is_none() && tag.key.starts_with("area:"))
        .map(|tag| tag.key.as_str())
        .collect()
}

/// The tags `template` declares, its areas customized: given `areas`
/// (each a permit pattern, `area:cli`), `area:` is CLOSED to exactly those
/// keys, the template's open `area:*` permit replaced by one permit a key;
/// given none, the template's own.
pub fn wanted(template: Template, areas: &[String]) -> Vec<tagma_core::Tag> {
    let open = permit_tag(AREA_OPEN);
    let own = template.schema().iter().map(|spelled| parsed(spelled));
    match areas.is_empty() {
        true => own.collect(),
        false => own
            .filter(|tag| *tag != open)
            .chain(areas.iter().map(|area| permit_tag(area)))
            .collect(),
    }
}

/// Why a template cannot be added to what a store holds: adding it would
/// take something away, or declare a second rule for a target. `held` is
/// each tag the store holds in the way, spelled in tagma's grammar.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Conflict {
    pub held: Vec<String>,
}

/// Of `wanted`, the tags `held` lacks, so that declaring them only ADDS:
///
/// - a tag already held is not missing;
/// - the template's open `area:*` is left out where the store already
///   permits any `area:` (open, or closed to keys: closing is a person's);
/// - a store holding `area:*` meets a template closing `area:` to keys: a
///   conflict (it would take the open permit away);
/// - a tag for a target the store already gives a different value of one
///   of [`SINGLE`]'s facets (another range, another formula) is a conflict
///   naming those held tags.
pub fn missing(
    held: &StoreRules,
    wanted: &[tagma_core::Tag],
) -> Result<Vec<tagma_core::Tag>, Conflict> {
    let open = permit_tag(AREA_OPEN);
    let permits_area = !area_permits(held).is_empty();
    let closes_area = wanted.iter().any(|tag| {
        tag.namespace.as_deref() == Some(PERMIT) && tag.key.starts_with("area:") && *tag != open
    });
    let mut conflicts: Vec<String> = Vec::new();
    if closes_area && held.holds(&open) {
        conflicts.push(open.to_string());
    }
    let mut missing = Vec::new();
    for tag in wanted {
        if held.holds(tag) || (*tag == open && permits_area) {
            continue;
        }
        let single = tag
            .namespace
            .as_deref()
            .is_some_and(|facet| SINGLE.contains(&facet));
        let rivals: Vec<String> = held
            .tags()
            .iter()
            .filter(|rival| single && rival.namespace == tag.namespace && rival.key == tag.key)
            .map(ToString::to_string)
            .collect();
        match rivals.is_empty() {
            true => missing.push(tag.clone()),
            false => conflicts.extend(rivals),
        }
    }
    conflicts.sort();
    conflicts.dedup();
    match conflicts.is_empty() {
        true => Ok(missing),
        false => Err(Conflict { held: conflicts }),
    }
}

#[cfg(test)]
#[path = "template.test.rs"]
mod tests;
