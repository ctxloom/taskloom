//! Lint: a read-time sweep of every task's tags against its store's rules,
//! judged by tagma (`Rules::validate` with no change: every finding one the
//! task already carries). A write is already refused what it would
//! introduce, so a violation here is data that did not come in through a
//! write: an import, or a rule declared since the tag was written. Lint
//! never blocks a write.
//!
//! Each violated rule on a task is one violation, naming tagma's code, the
//! tags it is about and its args. A closed row's tags are its history: of
//! a Done or Archived task, a tag no permit admits (`not-permitted`) and a
//! namespace over its keys-per-item limit (`too-many-keys`) are not
//! reported; every other rule holds whatever the status. And, against the
//! schema itself, a value-qualified placeholder in a `taskloom.priority_fn`
//! or `taskloom.decay_fn` (`{{triage:kind=foo}}`) naming a value its
//! target's enum does not admit, as tagma's `not-in-enum` says. A stored
//! tag that does not parse is not checked for: every stored tag is a parsed
//! one, so none can.

use std::collections::BTreeMap;
use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};
use tagma_core::Code;

use crate::read::Scan;
use crate::store_schema::{spelled_args, StoreRules};
use crate::view::is_terminal;
use task_priority::placeholders;

/// The subject of a violation in the schema rather than in a task, sorting
/// before every harp id.
pub const SCHEMA_SUBJECT: &str = "(schema)";

/// What a violation is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Finding {
    /// A rule of the store's a task breaks, as tagma reports it: its code
    /// (`rule`), the task's tags it is about (`, `-separated), and its
    /// args (`name=value, ...`).
    Rule {
        rule: String,
        tags: String,
        args: String,
    },
    /// A formula's placeholder naming a value its target's enum does not
    /// declare.
    FormulaEnum {
        facet: String,
        target: String,
        placeholder: String,
        reference: String,
        value: String,
        members: String,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for Finding {
    fn code(&self) -> &str {
        match self {
            Finding::Rule { .. } => "tasks-application-lint-rule",
            Finding::FormulaEnum { .. } => "tasks-application-lint-formula-enum",
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        let s = |v: &String| Arg::from(v.as_str());
        match self {
            Finding::Rule { rule, tags, args } => {
                vec![("rule", s(rule)), ("tags", s(tags)), ("args", s(args))]
            }
            Finding::FormulaEnum {
                facet,
                target,
                placeholder,
                reference,
                value,
                members,
            } => vec![
                ("facet", s(facet)),
                ("target", s(target)),
                ("placeholder", s(placeholder)),
                ("reference", s(reference)),
                ("value", s(value)),
                ("members", s(members)),
            ],
        }
    }
}

/// One violation: what, and of which task in which project, or of the
/// schema ([`SCHEMA_SUBJECT`], no project).
#[derive(Clone, PartialEq, Debug)]
pub struct Violation {
    pub project: Option<String>,
    pub harp_id: String,
    pub finding: Finding,
}

/// What a lint found, and how many targets it checked: an enum or range
/// declared. None checked means a clean sweep proves nothing.
#[derive(Clone, PartialEq, Debug)]
pub struct LintReport {
    pub reach: crate::Reach,
    pub violations: Vec<Violation>,
    pub checked_targets: usize,
}

/// The violations among the rows `scan` reached, each judged by its own
/// store's rules, and in the rules the scan is held to
/// ([`Scan::rules`]), sorted by project, subject and key and args, each
/// once; and how many targets those rules gave an enum or range.
pub(crate) fn lint(scan: &Scan) -> (Vec<Violation>, usize) {
    let held = scan.rules();
    let mut found: Vec<Violation> = formula_findings(&held)
        .into_iter()
        .map(|finding| Violation {
            project: None,
            harp_id: SCHEMA_SUBJECT.to_string(),
            finding,
        })
        .collect();
    for row in &scan.rows {
        let rules = scan.rules_of(&row.project.id);
        let closed = is_terminal(&row.task.status);
        let findings = rule_findings(&rules, &row.task.tags, closed);
        found.extend(findings.into_iter().map(|finding| Violation {
            project: Some(row.project.id.clone()),
            harp_id: row.task.harp_id.clone(),
            finding,
        }));
    }
    let mut keyed: Vec<(String, Violation)> = found
        .into_iter()
        .map(|v| (v.finding.to_string(), v))
        .collect();
    keyed.sort_by(|(a, x), (b, y)| (&x.project, &x.harp_id, a).cmp(&(&y.project, &y.harp_id, b)));
    keyed.dedup_by(|(a, x), (b, y)| (&x.project, &x.harp_id, &*a) == (&y.project, &y.harp_id, &*b));
    (
        keyed.into_iter().map(|(_, v)| v).collect(),
        held.checked_targets(),
    )
}

/// What tagma finds of `tags` under `rules`, one finding a violated rule:
/// those of one code and args, its tags together. A `closed` row's
/// permits and keys-per-item are not judged.
fn rule_findings(rules: &StoreRules, tags: &[crate::Tag], closed: bool) -> Vec<Finding> {
    let mut grouped: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for finding in rules.validate(tags, &[], &[]) {
        if closed && matches!(finding.code, Code::NotPermitted | Code::TooManyKeys) {
            continue;
        }
        grouped
            .entry((
                finding.code.as_str().to_string(),
                spelled_args(&finding.args),
            ))
            .or_default()
            .push(finding.tag.to_string());
    }
    grouped
        .into_iter()
        .map(|((rule, args), tags)| Finding::Rule {
            rule,
            tags: tags.join(", "),
            args,
        })
        .collect()
}

/// The formulas' value-qualified placeholders whose value their target's
/// enum does not admit, as tagma judges the tag they name. `target=*` is a presence test, not a value.
fn formula_findings(rules: &StoreRules) -> Vec<Finding> {
    let (priority_fns, decay_fns) = rules.formulas();
    let facets = [("priority_fn", priority_fns), ("decay_fn", decay_fns)];
    let mut found = Vec::new();
    for (facet, formulas) in facets {
        for (target, formula) in &formulas {
            for placeholder in placeholders(formula) {
                let Some((reference, value)) = placeholder.split_once('=') else {
                    continue;
                };
                if !reference.contains(':') || value == "*" {
                    continue;
                }
                let Some(members) = not_in_enum(rules, reference, value) else {
                    continue;
                };
                found.push(Finding::FormulaEnum {
                    facet: facet.to_string(),
                    target: target.clone(),
                    placeholder: format!("{{{{{placeholder}}}}}"),
                    reference: reference.to_string(),
                    value: value.to_string(),
                    members,
                });
            }
        }
    }
    found
}

/// The values `reference` admits, `, `-separated, when its enum does not
/// admit `value` (tagma's `not-in-enum` of `reference=value`); `None` when
/// it does, or declares no enum, or `reference=value` is no tag.
fn not_in_enum(rules: &StoreRules, reference: &str, value: &str) -> Option<String> {
    let tag = tagma_core::Tag::parse(&format!("{reference}={value}")).ok()?;
    let finding = rules
        .validate(&[], &[tag.into()], &[])
        .into_iter()
        .find(|f| f.code == Code::NotInEnum)?;
    Some(
        finding
            .args
            .iter()
            .map(|(_, member)| member.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// One of every finding, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn samples() -> Vec<Finding> {
    let x = || "x".to_string();
    vec![
        Finding::Rule {
            rule: "too-many-keys".to_string(),
            tags: "area:cli, area:core".to_string(),
            args: "namespace=area, limit=1, keys=cli, keys=core".to_string(),
        },
        Finding::FormulaEnum {
            facet: x(),
            target: x(),
            placeholder: x(),
            reference: x(),
            value: x(),
            members: x(),
        },
    ]
}

#[cfg(test)]
#[path = "lint.test.rs"]
mod tests;
