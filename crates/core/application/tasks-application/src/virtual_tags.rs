//! Virtual tags: a task's metadata, read in a tag query as tags in the
//! reserved `taskloom` namespace (`taskloom:in-status>=2d`), never
//! stored.
//!
//! The namespace is the store's alone: a write may not put a tag in it
//! (`tag-rules`' reserved namespaces), and a query ignores one an old
//! store still holds, so a task's own tag never stands in for the fact.
//! Each virtual tag is derived from the task and the read's instant when
//! the parsed query names it (`taskloom:<name>`), and not otherwise: a read
//! that names none pays for none. [`VIRTUAL_TAGS`] is the table
//! `taskloom manage schema virtual list` prints.
//!
//! The duration tags (`deferred`, `in-status`, `age`) hold the whole
//! seconds since, and a query compares them with a duration or a date
//! (`>=2d`), resolved to seconds before tagma evaluates the query
//! ([`resolve_durations`]); a bare number is refused.

use std::collections::BTreeSet;

use ports::Timestamp;
use tasks::TaskState;

use crate::rules::TASKLOOM_NAMESPACE;
use crate::view::{is_terminal, Task};
use coded_diagnostic::Arg;
use duration_text::DurationError;

/// Time since the task last entered Deferred; only while deferred.
pub const DEFERRED_FOR: &str = "deferred";
/// Time since the task's status last changed (its add counts).
pub const IN_STATUS: &str = "in-status";
/// Time since the task was created.
pub const AGE: &str = "age";
/// The task's status name.
pub const STATUS: &str = "status";
/// Present while the task has a revive trigger.
pub const HAS_TRIGGER: &str = "has-trigger";
/// Present while the task's checkbox is set (its status is terminal).
pub const CHECKED: &str = "checked";
/// The session that created the task, when one did.
pub const ORIGIN: &str = "origin";
/// The task's derived priority, when the store's rules compute one.
pub const PRIORITY: &str = "priority";

/// One virtual tag, as `manage schema virtual list` describes it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VirtualTag {
    pub name: &'static str,
    /// `duration` (compared with `>` `>=` `<` `<=` to a duration or a date,
    /// never a bare number and never `=` `!=` `~`: `>=2d`, exact on whole
    /// seconds), `number` (compared by value), `text` (`=`) or `flag`
    /// (present or absent, no value).
    pub kind: &'static str,
    pub meaning: &'static str,
}

/// Every virtual tag, in the order they are listed.
pub const VIRTUAL_TAGS: [VirtualTag; 8] = [
    VirtualTag {
        name: DEFERRED_FOR,
        kind: "duration",
        meaning: "time since the task was deferred (>=2d); absent, so matching neither >= nor <, unless it is Deferred",
    },
    VirtualTag {
        name: IN_STATUS,
        kind: "duration",
        meaning: "time since the status last changed (<1w; an add counts); what prune --older-than measures",
    },
    VirtualTag {
        name: AGE,
        kind: "duration",
        meaning: "time since the task was created (>=720h); what {{age_days}} measures, in days",
    },
    VirtualTag {
        name: STATUS,
        kind: "text",
        meaning: "the status name (taskloom:status=Deferred); --status holds the same set, and --status alone takes the classes @open and @terminal (taskloom:status=@terminal is refused)",
    },
    VirtualTag {
        name: HAS_TRIGGER,
        kind: "flag",
        meaning: "present while the task has a revive trigger",
    },
    VirtualTag {
        name: CHECKED,
        kind: "flag",
        meaning: "present while the checkbox is set: the status is Done or Archived",
    },
    VirtualTag {
        name: ORIGIN,
        kind: "text",
        meaning: "the session that created the task; absent when none did",
    },
    VirtualTag {
        name: PRIORITY,
        kind: "number",
        meaning: "the derived priority the store's formulas rank the task at; absent when the store declares no priority_fn",
    },
];

/// Which virtual tags a query names.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Mentioned(BTreeSet<&'static str>);

impl Mentioned {
    /// Whether the query names `name`.
    pub fn has(&self, name: &str) -> bool {
        self.0.contains(name)
    }
}

/// The virtual tags `query` names: each `taskloom:<name>` atom, and every
/// virtual tag for `taskloom:*`. A name is read up to the operator that
/// follows it; an atom of another namespace (`taskloom.internal:...`) or
/// one only ending in `taskloom` names none. A wildcard namespace
/// (`*:status`) names none either: only an atom naming `taskloom:` reaches
/// a virtual tag.
pub fn mentioned(query: Option<&str>) -> Mentioned {
    let query = query.unwrap_or_default();
    let prefix = format!("{TASKLOOM_NAMESPACE}:");
    let mut found = BTreeSet::new();
    for (at, _) in query.match_indices(&prefix) {
        let before = query[..at].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || "._-*".contains(c)) {
            continue;
        }
        let rest = &query[at + prefix.len()..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if name.is_empty() && rest.starts_with('*') {
            found.extend(VIRTUAL_TAGS.iter().map(|v| v.name));
        } else if let Some(known) = VIRTUAL_TAGS.iter().find(|v| v.name == name) {
            found.insert(known.name);
        }
    }
    Mentioned(found)
}

/// What a task's virtual tags are derived from.
#[derive(Clone, Debug)]
pub struct Facts<'a> {
    pub id: &'a str,
    pub tags: &'a [crate::Tag],
    pub status: &'a str,
    pub trigger: &'a str,
    pub checked: bool,
    pub origin_session: &'a str,
    pub created_at: Timestamp,
    pub status_set_at: Timestamp,
    pub deferred_since: Option<Timestamp>,
    /// Filled by the caller only when the query names `priority`.
    pub priority: Option<f64>,
}

impl<'a> Facts<'a> {
    /// A task as read.
    pub fn of_task(task: &'a Task, id: &'a str) -> Facts<'a> {
        Facts {
            id,
            tags: &task.tags,
            status: &task.status,
            trigger: &task.trigger,
            checked: task.checked,
            origin_session: &task.origin_session,
            created_at: task.created_at,
            status_set_at: task.status_set_at,
            deferred_since: task.deferred_since,
            priority: None,
        }
    }

    /// A task as the store folds it (a prune reads these).
    pub fn of_state(state: &'a TaskState) -> Facts<'a> {
        Facts {
            id: &state.harp_id,
            tags: &state.tags,
            status: &state.status,
            trigger: &state.trigger,
            checked: is_terminal(&state.status),
            origin_session: &state.origin_session,
            created_at: state.created_at,
            status_set_at: state.status_set_at,
            deferred_since: state.deferred_since.filter(|_| !state.trigger.is_empty()),
            priority: None,
        }
    }
}

#[cfg(test)]
impl<'a> Facts<'a> {
    /// A To Do task `id` holding `tags`, with nothing else to say.
    pub(crate) fn bare(id: &'a str, tags: &'a [crate::Tag]) -> Facts<'a> {
        Facts {
            id,
            tags,
            status: "To Do",
            trigger: "",
            checked: false,
            origin_session: "",
            created_at: Timestamp::default(),
            status_set_at: Timestamp::default(),
            deferred_since: None,
            priority: None,
        }
    }
}

/// The virtual tags `mentioned` names, derived from `facts` at `now`, and
/// no others.
pub(crate) fn derive(
    facts: &Facts<'_>,
    mentioned: &Mentioned,
    now: Timestamp,
) -> Vec<tagma_core::Tag> {
    let tag = |key: &str, value: Option<String>| tagma_core::Tag {
        namespace: Some(TASKLOOM_NAMESPACE.to_string()),
        key: key.to_string(),
        value,
    };
    let hours = |since: Timestamp| Some(whole_seconds(since, now).to_string());
    let measured = |at: Timestamp| (at != Timestamp::default()).then_some(at);
    let mut tags = Vec::new();
    if mentioned.has(DEFERRED_FOR) {
        tags.extend(
            facts
                .deferred_since
                .map(|since| tag(DEFERRED_FOR, hours(since))),
        );
    }
    if mentioned.has(IN_STATUS) {
        tags.extend(measured(facts.status_set_at).map(|at| tag(IN_STATUS, hours(at))));
    }
    if mentioned.has(AGE) {
        tags.extend(measured(facts.created_at).map(|at| tag(AGE, hours(at))));
    }
    if mentioned.has(STATUS) {
        tags.push(tag(STATUS, Some(facts.status.to_string())));
    }
    if mentioned.has(HAS_TRIGGER) && !facts.trigger.is_empty() {
        tags.push(tag(HAS_TRIGGER, None));
    }
    if mentioned.has(CHECKED) && facts.checked {
        tags.push(tag(CHECKED, None));
    }
    if mentioned.has(ORIGIN) && !facts.origin_session.is_empty() {
        tags.push(tag(ORIGIN, Some(facts.origin_session.to_string())));
    }
    if mentioned.has(PRIORITY) {
        tags.extend(facts.priority.map(|p| tag(PRIORITY, Some(p.to_string()))));
    }
    tags
}

/// Whether a status last changed (`status_set_at`) before `cut`: what
/// `prune --older-than` selects by, to the nanosecond; `in-status` is
/// the whole seconds of the same span.
pub(crate) fn changed_before(status_set_at: Timestamp, cut: Timestamp) -> bool {
    (status_set_at.seconds, status_set_at.nanos) < (cut.seconds, cut.nanos)
}

/// The whole seconds from `since` to `now` (a fraction is dropped): none
/// for an instant after it. What a duration virtual tag's value is.
pub fn whole_seconds(since: Timestamp, now: Timestamp) -> i64 {
    task_priority::seconds_since(since, now).floor() as i64
}

/// Whether `name` is a duration virtual tag.
pub fn is_duration(name: &str) -> bool {
    VIRTUAL_TAGS
        .iter()
        .any(|v| v.name == name && v.kind == "duration")
}

/// Why a duration virtual tag's atom is refused.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Why {
    /// `=`, `!=` or `~`: a span has no one second to equal.
    Operator,
    /// The literal is not a duration or a date, and why.
    Literal(DurationError),
}

impl Why {
    fn arg(&self) -> Arg {
        match self {
            Why::Operator => "operator".into(),
            Why::Literal(cause) => Arg::nested(cause),
        }
    }
}

/// A duration virtual tag's atom that cannot be read as a comparison.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DurationRefusal {
    pub query: String,
    /// The atom as written: `taskloom:age>=30`.
    pub atom: String,
    pub why: Why,
}

/// The units a duration is written in, as a refusal lists them.
pub const DURATION_UNITS: &str =
    "w, d, h, m, s, chained longest first and each once (2w, 1d12h, 1.5h), or a date (2026-01-01)";

impl std::fmt::Display for DurationRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl coded_diagnostic::Diagnostic for DurationRefusal {
    fn code(&self) -> &str {
        coded_diagnostic::code!("tag-query-duration")
    }

    fn args(&self) -> Vec<(&str, coded_diagnostic::Arg)> {
        vec![
            ("query", self.query.as_str().into()),
            ("atom", self.atom.as_str().into()),
            // `atom` when the query is the atom alone, so the message
            // names it once.
            ("scope", scope(&self.query, &self.atom).into()),
            ("why", self.why.arg()),
            ("units", DURATION_UNITS.into()),
        ]
    }
}

/// An atom in the reserved `taskloom` namespace whose key is no virtual
/// tag: a typo (`taskloom:deferd`) would otherwise match nothing, quietly.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnknownVirtualTag {
    pub query: String,
    /// The atom as written: `taskloom:deferd>=1d`.
    pub atom: String,
    /// Its key, read up to the operator: `deferd`.
    pub name: String,
}

impl std::fmt::Display for UnknownVirtualTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl coded_diagnostic::Diagnostic for UnknownVirtualTag {
    fn code(&self) -> &str {
        coded_diagnostic::code!("tag-query-unknown-virtual")
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        let valid: Vec<&str> = VIRTUAL_TAGS.iter().map(|v| v.name).collect();
        vec![
            ("query", self.query.as_str().into()),
            ("atom", self.atom.as_str().into()),
            ("scope", scope(&self.query, &self.atom).into()),
            ("name", self.name.as_str().into()),
            ("valid", valid.join(", ").into()),
        ]
    }
}

/// Whether a refusal's atom is the whole query (`atom`) or one of its
/// tokens (`query`), so the message names a lone atom once.
fn scope(query: &str, atom: &str) -> &'static str {
    if query.trim() == atom {
        "atom"
    } else {
        "query"
    }
}

/// Refuse the first atom of `query` in the `taskloom` namespace whose key
/// is not a virtual tag. The wildcard (`taskloom:*`), a quoted or empty
/// key and every other namespace (`taskloom.internal:`) are left to tagma.
pub(crate) fn check_names(query: &str) -> Result<(), UnknownVirtualTag> {
    let prefix = format!("{TASKLOOM_NAMESPACE}:");
    for token in split_tokens(query) {
        let Some(rest) = token.trim().strip_prefix(&prefix) else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !name.is_empty() && !VIRTUAL_TAGS.iter().any(|v| v.name == name) {
            return Err(UnknownVirtualTag {
                query: query.to_string(),
                atom: token.trim().to_string(),
                name,
            });
        }
    }
    Ok(())
}

/// `query` with each duration virtual tag's atom (`taskloom:age>=2d`)
/// resolved to the whole seconds it asks of the tag's value, read at
/// `now` (to the second); every other token is left as written. Or why an
/// atom is refused:
///
/// - the one rule: `>`, `>=`, `<` and `<=` compare the elapsed whole
///   seconds with the literal's exactly (`>=2d` is at least 172800 s; a
///   future instant is 0 s elapsed). A task without the tag (never
///   deferred) matches none of them.
/// - the literal is read by `duration-text`'s grammar: units `w` (7 days), `d`
///   (24 hours), `h`, `m` and `s`, chained longest first and each once
///   (`1h30m`), fractions allowed (`1.5h`, rounded to the nearest
///   second, a half up), or a date (the time since its midnight UTC);
///   `0d` is zero, a bare number (`0` too) is refused.
/// - `=`, `!=` and `~` are refused: ask `>=` and `<` (a pair for a
///   window: `taskloom:age>=1d/taskloom:age<2d/and`).
///
/// A presence atom (`taskloom:age`, no operator) is left as written.
pub(crate) fn resolve_durations(query: &str, now: Timestamp) -> Result<String, DurationRefusal> {
    let prefix = format!("{TASKLOOM_NAMESPACE}:");
    let mut out = Vec::new();
    for token in split_tokens(query) {
        out.push(resolve_token(query, token, &prefix, now)?);
    }
    Ok(out.join("/"))
}

/// `query` split at the `/` outside double quotes.
fn split_tokens(query: &str) -> Vec<&str> {
    let (mut tokens, mut start, mut quoted) = (Vec::new(), 0, false);
    for (at, c) in query.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '/' if !quoted => {
                tokens.push(&query[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    tokens.push(&query[start..]);
    tokens
}

fn resolve_token(
    query: &str,
    token: &str,
    prefix: &str,
    now: Timestamp,
) -> Result<String, DurationRefusal> {
    let Some(rest) = token.trim().strip_prefix(prefix) else {
        return Ok(token.to_string());
    };
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let after = &rest[name.len()..];
    let op = [">=", "<=", "!=", ">", "<", "=", "~"]
        .into_iter()
        .find(|op| after.starts_with(op));
    let (true, Some(op)) = (is_duration(&name), op) else {
        return Ok(token.to_string());
    };
    let refuse = |why| DurationRefusal {
        query: query.to_string(),
        atom: token.trim().to_string(),
        why,
    };
    if matches!(op, "=" | "!=" | "~") {
        return Err(refuse(Why::Operator));
    }
    let seconds =
        elapsed_seconds(&after[op.len()..], now).map_err(|cause| refuse(Why::Literal(cause)))?;
    Ok(format!("{prefix}{name}{}", rule(op, seconds)))
}

/// The whole seconds from what `literal` names to `now`: the span itself,
/// or the time since a date's midnight (never negative). The core holds
/// no `SystemTime`, so a date is the calendar's and a span the shared
/// grammar's, which reads no clock.
fn elapsed_seconds(literal: &str, now: Timestamp) -> Result<i64, DurationError> {
    let text = literal.trim();
    if let Some(midnight) = crate::calendar::date(text) {
        return Ok((now.seconds - midnight.seconds).max(0));
    }
    let span = duration_text::parse(text)?;
    i64::try_from(span.as_secs()).map_err(|_| DurationError::OutOfRange {
        text: text.to_string(),
    })
}

/// The comparison of a task's elapsed whole seconds `E` with a literal
/// of `seconds`, exact: the atom on the tag's seconds that `op seconds`
/// stands for. The one place the boundary semantics live.
fn rule(op: &str, seconds: i64) -> String {
    match op {
        "<=" => format!("<{}", seconds + 1),
        op => format!("{op}{seconds}"),
    }
}

/// A refusal of each code, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn samples() -> Vec<Box<dyn coded_diagnostic::Diagnostic>> {
    vec![
        Box::new(DurationRefusal {
            query: "area:cli/taskloom:age>=30/and".to_string(),
            atom: "taskloom:age>=30".to_string(),
            why: Why::Literal(DurationError::Malformed {
                text: "30".to_string(),
                ext_err_txt: String::new(),
            }),
        }),
        Box::new(UnknownVirtualTag {
            query: "area:cli/taskloom:deferd/and".to_string(),
            atom: "taskloom:deferd".to_string(),
            name: "deferd".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "virtual_tags.test.rs"]
mod tests;
