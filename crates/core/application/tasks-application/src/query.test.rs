//! Tag queries: tagma-core held to the reference answers.
//!
//! `fixtures/tag-queries/cases.tsv` holds the cases; `go.tsv` is the
//! reference answer to each, written by
//! `scripts/compare-go-tag-queries.sh`. Every query answers as the
//! reference does; both read postfix only, so an infix case is refused.

use std::collections::{BTreeMap, BTreeSet};

use coded_diagnostic::{code, Diagnostic};

use ports::Timestamp;

use super::{check, select, QueryRefusal, QUERY_GRAMMAR};
use crate::rules::StoreRules;
use crate::tags::parse;
use crate::virtual_tags::Facts;
use crate::Tag;

/// The instant the derived tags are read at.
const NOW: Timestamp = Timestamp {
    seconds: 1_700_000_000,
    nanos: 0,
};

const CASES: &str = include_str!("../fixtures/tag-queries/cases.tsv");
const GO: &str = include_str!("../fixtures/tag-queries/go.tsv");

#[derive(Default)]
struct Scenario {
    schema: Vec<String>,
    statuses: Vec<String>,
    /// `(harp, status, tags)`.
    tasks: Vec<(String, String, Vec<Tag>)>,
    queries: Vec<String>,
}

fn scenarios() -> BTreeMap<String, Scenario> {
    let mut all: BTreeMap<String, Scenario> = BTreeMap::new();
    let mut current = String::new();
    for line in CASES
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields[0] == "scenario" {
            current = fields[1].to_string();
            all.insert(current.clone(), Scenario::default());
            continue;
        }
        let scenario = all.get_mut(&current).expect("a scenario first");
        match fields[0] {
            "schema" => scenario.schema.push(fields[1].to_string()),
            "status" => scenario.statuses.push(fields[1].to_string()),
            "task" => scenario.tasks.push((
                fields[1].to_string(),
                fields[2].to_string(),
                fields[3..]
                    .iter()
                    .map(|t| parse(t).expect("a tag").expect("parses"))
                    .collect(),
            )),
            "query" => scenario
                .queries
                .push(fields.get(1).copied().unwrap_or_default().to_string()),
            other => panic!("unknown line {other}"),
        }
    }
    all
}

/// The reference answer to each `(scenario, query)`: the ids, or `None`
/// for an error.
fn go() -> BTreeMap<(String, String), Option<BTreeSet<String>>> {
    GO.lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let answer = match fields[2] {
                "ok" => Some(
                    fields[3]
                        .split(',')
                        .filter(|id| !id.is_empty())
                        .map(str::to_string)
                        .collect(),
                ),
                _ => None,
            };
            ((fields[0].to_string(), fields[1].to_string()), answer)
        })
        .collect()
}

/// `query` run over `scenario`'s tasks. A case's empty query is Go's
/// spelling of no query, which here is `None`: an empty one given is
/// refused before it reaches [`select`] (`AppError::Blank`).
fn run(scenario: &Scenario, query: &str) -> Result<BTreeSet<String>, QueryRefusal> {
    let query = Some(query).filter(|q| !q.is_empty());
    let spelled: Vec<&str> = scenario.schema.iter().map(String::as_str).collect();
    let schema = StoreRules::parsed(&spelled);
    let candidates = scenario
        .tasks
        .iter()
        .filter(|(_, status, _)| scenario.statuses.is_empty() || scenario.statuses.contains(status))
        .map(|(harp, status, tags)| Facts {
            status,
            ..Facts::bare(harp, tags)
        });
    select(query, candidates, NOW, &schema)
}

#[test]
fn test_every_query_answers_as_go_taskloom_answers() {
    let go = go();
    let mut compared = 0;
    let mut differences = Vec::new();
    for (name, scenario) in scenarios() {
        for query in &scenario.queries {
            let key = (name.clone(), query.clone());
            let ours = run(&scenario, query).ok();
            let theirs = go[&key].clone();
            compared += 1;
            if ours != theirs {
                differences.push(format!("{name} {query:?}: ours {ours:?}, Go's {theirs:?}"));
            }
        }
    }
    assert_eq!(differences, Vec::<String>::new());
    assert_eq!(compared, go.len(), "every case Go answered");
}

fn postfix_text(error: &QueryRefusal) -> &str {
    match error {
        QueryRefusal::Malformed(e) => &e.postfix,
        other => panic!("{other:?}"),
    }
}

// A malformed query fails loud, saying how to write one.
#[test]
fn test_a_malformed_query_is_refused_with_the_grammar() {
    let error = check(Some("and")).unwrap_err();
    assert_eq!(error.code(), code!("tag-query"));
    assert!(error.to_string().contains(QUERY_GRAMMAR), "{error}");
    assert!(error.to_string().contains("and"), "{error}");
    let names: Vec<&str> = error.args().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["query", "postfix", "grammar"]);
    assert!(!postfix_text(&error).is_empty());
    assert_eq!(check(None), Ok(()));
    assert_eq!(check(Some("urgent/release/and")), Ok(()));
}

// Queries are postfix only: an infix one is a malformed postfix query,
// refused with postfix's reason and the postfix grammar, no infix hint.
#[test]
fn test_an_infix_query_is_refused_as_malformed_postfix() {
    for query in [
        "urgent and release",
        "not urgent",
        "(urgent or release) and not level=3",
    ] {
        let error = check(Some(query)).unwrap_err();
        assert_eq!(error.code(), code!("tag-query"));
        let postfix = tagma_core::Index::new().query_postfix(query).unwrap_err();
        assert_eq!(postfix_text(&error), postfix, "{query}");
        let shown = error.to_string();
        assert!(shown.contains(QUERY_GRAMMAR), "{shown}");
        assert!(!shown.contains("infix"), "{shown}");
    }
    assert!(!QUERY_GRAMMAR.contains("infix"), "{QUERY_GRAMMAR}");
}

#[test]
fn test_no_query_selects_every_candidate() {
    let tags = [parse("x").unwrap().unwrap()];
    let selected = select(
        None,
        [Facts::bare("a", &tags), Facts::bare("b", &[])],
        NOW,
        &StoreRules::parsed(&[]),
    );
    assert_eq!(selected.unwrap().len(), 2);
}

// The schema's type declarations are indexed, but never answer a query:
// the item holding them is no candidate.
#[test]
fn test_the_type_declarations_are_never_selected() {
    let schema = StoreRules::parsed(&[r#"tagma.type:"triage:x"=semver"#]);
    let selected = select(Some("*:*"), [Facts::bare("a", &[])], NOW, &schema).unwrap();
    assert_eq!(selected, BTreeSet::from(["a".to_string()]));
    let selected = select(Some("tagma.type:*"), [Facts::bare("a", &[])], NOW, &schema).unwrap();
    assert!(selected.is_empty());
}

// ------------------------------------------------------------ derived tags

/// `seconds` before NOW.
fn ago(seconds: i64) -> Option<Timestamp> {
    Some(Timestamp {
        seconds: NOW.seconds - seconds,
        nanos: 0,
    })
}

/// Which of `old` (deferred 2 days, exactly), `fresh` (deferred a second
/// short of 1 day), `tagged` (as `fresh`, tagged area:cli) and `awake` (not
/// deferred) `query` selects.
fn deferrals(query: &str) -> Vec<String> {
    let cli = [parse("area:cli").unwrap().unwrap()];
    let deferred = |id, tags, since| Facts {
        deferred_since: since,
        ..Facts::bare(id, tags)
    };
    let candidates = [
        deferred("old", &[], ago(2 * 86_400)),
        deferred("fresh", &[], ago(86_399)),
        deferred("tagged", &cli, ago(86_399)),
        deferred("awake", &[], None),
    ];
    let selected = select(Some(query), candidates, NOW, &StoreRules::default()).unwrap();
    selected.into_iter().collect()
}

#[test]
fn test_a_deferred_task_carries_the_whole_seconds_since_it_was_deferred() {
    assert_eq!(
        deferrals("taskloom:deferred"),
        ["fresh", "old", "tagged"],
        "present on every deferred task, never on the one that is not"
    );
    assert_eq!(
        deferrals("taskloom:deferred>=2d/taskloom:deferred<=2d/and"),
        ["old"],
        "exactly two days"
    );
    assert_eq!(
        deferrals("taskloom:deferred>=86399s/taskloom:deferred<=86399s/and"),
        ["fresh", "tagged"],
        "a second short of a day"
    );
}

#[test]
fn test_deferred_orders_by_duration_and_composes_with_tags() {
    assert_eq!(deferrals("taskloom:deferred>=1d"), ["old"]);
    assert_eq!(
        deferrals("taskloom:deferred>23h"),
        ["fresh", "old", "tagged"]
    );
    assert_eq!(deferrals("taskloom:deferred<1d"), ["fresh", "tagged"]);
    assert_eq!(deferrals("taskloom:deferred<=1d/area:cli/and"), ["tagged"]);
    assert_eq!(deferrals("taskloom:deferred/not"), ["awake"]);
}

#[test]
fn test_a_task_deferred_in_the_future_has_been_deferred_no_time() {
    let ahead = Some(Timestamp {
        seconds: NOW.seconds + 7200,
        nanos: 0,
    });
    let selected = select(
        Some("taskloom:deferred<1s"),
        [Facts {
            deferred_since: ahead,
            ..Facts::bare("skewed", &[])
        }],
        NOW,
        &StoreRules::default(),
    )
    .unwrap();
    assert_eq!(selected.len(), 1);
}

// ------------------------------------------------------------ virtual tags

/// A task at the fixed instant, with whatever `shape` gives it.
fn hours_ago(hours: i64) -> Timestamp {
    ago(hours * 3600).unwrap()
}

fn shaped<'a>(id: &'a str, shape: impl FnOnce(&mut Facts<'a>)) -> Facts<'a> {
    let mut facts = Facts::bare(id, &[]);
    facts.created_at = hours_ago(10);
    facts.status_set_at = hours_ago(3);
    shape(&mut facts);
    facts
}

fn chosen(query: &str, candidates: Vec<Facts<'_>>) -> Vec<String> {
    select(Some(query), candidates, NOW, &StoreRules::default())
        .unwrap()
        .into_iter()
        .collect()
}

fn population() -> Vec<Facts<'static>> {
    vec![
        shaped("plain", |_| {}),
        shaped("parked", |f| {
            f.status = "Deferred";
            f.trigger = "after release";
            f.deferred_since = Some(hours_ago(50));
            f.status_set_at = hours_ago(50);
        }),
        shaped("done", |f| {
            f.status = "Done";
            f.checked = true;
            f.origin_session = "sess-9";
            f.priority = Some(7.0);
        }),
        shaped("working", |f| {
            f.status = "In Progress";
            f.origin_session = "sess-1";
            f.priority = Some(2.5);
            f.created_at = hours_ago(100);
        }),
    ]
}

#[test]
fn test_a_query_filters_on_each_virtual_tag() {
    let q = |query: &str| chosen(query, population());
    assert_eq!(q("taskloom:status=Deferred"), ["parked"]);
    assert_eq!(
        q("taskloom:status!=Deferred/taskloom:checked/not/and"),
        ["plain", "working"]
    );
    assert_eq!(q("taskloom:in-status>=50h"), ["parked"]);
    assert_eq!(q("taskloom:in-status<4h"), ["done", "plain", "working"]);
    assert_eq!(q("taskloom:age>=100h"), ["working"]);
    assert_eq!(q("taskloom:has-trigger"), ["parked"]);
    assert_eq!(q("taskloom:checked"), ["done"]);
    assert_eq!(q("taskloom:origin=sess-1"), ["working"]);
    assert_eq!(q("taskloom:origin=+"), ["done", "working"]);
    assert_eq!(q("taskloom:priority>=3"), ["done"]);
    assert_eq!(q("taskloom:priority<3"), ["working"]);
    assert_eq!(q("taskloom:priority=2.5"), ["working"]);
}

#[test]
fn test_a_status_with_a_space_is_matched_quoted() {
    assert_eq!(
        chosen(r#"taskloom:status="In Progress""#, population()),
        ["working"]
    );
}

#[test]
fn test_virtual_tags_compose_with_stored_tags_and_each_other() {
    let cli = [parse("area:cli").unwrap().unwrap()];
    let mut candidates = population();
    candidates.push(Facts {
        status: "Deferred",
        trigger: "t",
        deferred_since: Some(hours_ago(3)),
        ..Facts::bare("tagged", &cli)
    });
    assert_eq!(
        chosen("taskloom:has-trigger/area:cli/and", candidates.clone()),
        ["tagged"]
    );
    assert_eq!(
        chosen(
            "taskloom:deferred>=3h/taskloom:in-status>=50h/and",
            candidates
        ),
        ["parked"]
    );
}

#[test]
fn test_a_tag_stored_in_the_taskloom_namespace_is_ignored() {
    let stored = [
        parse("taskloom:status=Done").unwrap().unwrap(),
        parse("taskloom:checked").unwrap().unwrap(),
    ];
    let candidates = || vec![Facts::bare("liar", &stored)];
    assert!(chosen("taskloom:status=Done", candidates()).is_empty());
    assert!(chosen("taskloom:checked", candidates()).is_empty());
    assert_eq!(chosen("taskloom:status=\"To Do\"", candidates()), ["liar"]);
    assert_eq!(
        chosen("taskloom:*", candidates()),
        ["liar"],
        "the derived status is there"
    );
}

#[test]
fn test_a_wildcard_namespace_does_not_ask_for_a_virtual_tag() {
    assert!(chosen("*:status", population()).is_empty());
}

#[test]
fn test_taskloom_internal_tags_are_not_virtual_tags() {
    assert_eq!(chosen("taskloom.internal:candidate", population()).len(), 4);
    assert!(chosen("taskloom.internal:status", population()).is_empty());
}
