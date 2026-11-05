//! Tag queries: tagma-core held to the reference answers.
//!
//! `fixtures/tag-queries/cases.tsv` holds the cases; `go.tsv` is the
//! reference answer to each, written by
//! `scripts/compare-go-tag-queries.sh`. Every query answers as the
//! reference does; both read postfix only, so an infix case is refused.

use std::collections::{BTreeMap, BTreeSet};

use coded_diagnostic::{code, Diagnostic};

use ports::Timestamp;

use super::{check, select, QueryError, QUERY_GRAMMAR};
use crate::rules::StoreRules;
use crate::tags::parse;
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
fn run(scenario: &Scenario, query: &str) -> Result<BTreeSet<String>, QueryError> {
    let query = Some(query).filter(|q| !q.is_empty());
    let spelled: Vec<&str> = scenario.schema.iter().map(String::as_str).collect();
    let schema = StoreRules::parsed(&spelled);
    let candidates = scenario
        .tasks
        .iter()
        .filter(|(_, status, _)| scenario.statuses.is_empty() || scenario.statuses.contains(status))
        .map(|(harp, _, tags)| (harp.as_str(), tags.as_slice(), None));
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

// A malformed query fails loud, saying how to write one.
#[test]
fn test_a_malformed_query_is_refused_with_the_grammar() {
    let error = check(Some("and")).unwrap_err();
    assert_eq!(error.code(), code!("tag-query"));
    assert!(error.to_string().contains(QUERY_GRAMMAR), "{error}");
    assert!(error.to_string().contains("and"), "{error}");
    let names: Vec<&str> = error.args().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["query", "postfix", "grammar"]);
    assert!(!error.postfix.is_empty());
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
        assert_eq!(error.postfix, postfix, "{query}");
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
        [("a", &tags[..], None), ("b", &[][..], None)],
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
    let selected = select(Some("*:*"), [("a", &[][..], None)], NOW, &schema).unwrap();
    assert_eq!(selected, BTreeSet::from(["a".to_string()]));
    let selected = select(Some("tagma.type:*"), [("a", &[][..], None)], NOW, &schema).unwrap();
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
    let candidates = [
        ("old", &[][..], ago(2 * 86_400)),
        ("fresh", &[][..], ago(86_399)),
        ("tagged", &cli[..], ago(86_399)),
        ("awake", &[][..], None),
    ];
    let selected = select(Some(query), candidates, NOW, &StoreRules::default()).unwrap();
    selected.into_iter().collect()
}

#[test]
fn test_a_deferred_task_carries_the_whole_hours_since_it_was_deferred() {
    assert_eq!(
        deferrals("taskloom.internal:deferred-hours=+"),
        ["fresh", "old", "tagged"],
        "present on every deferred task, never on the one that is not"
    );
    assert_eq!(
        deferrals("taskloom.internal:deferred-hours=48"),
        ["old"],
        "exactly two days"
    );
    assert_eq!(
        deferrals("taskloom.internal:deferred-hours=23"),
        ["fresh", "tagged"],
        "a second short of a day is 23 whole hours"
    );
}

#[test]
fn test_deferred_hours_order_by_number_and_compose_with_tags() {
    assert_eq!(deferrals("taskloom.internal:deferred-hours>=24"), ["old"]);
    assert_eq!(deferrals("taskloom.internal:deferred-hours>23"), ["old"]);
    assert_eq!(
        deferrals("taskloom.internal:deferred-hours<24"),
        ["fresh", "tagged"]
    );
    assert_eq!(
        deferrals("taskloom.internal:deferred-hours<=23/area:cli/and"),
        ["tagged"]
    );
    assert_eq!(
        deferrals("taskloom.internal:deferred-hours=+/not"),
        ["awake"]
    );
}

#[test]
fn test_a_task_deferred_in_the_future_has_been_deferred_no_hours() {
    let ahead = Some(Timestamp {
        seconds: NOW.seconds + 7200,
        nanos: 0,
    });
    let selected = select(
        Some("taskloom.internal:deferred-hours=0"),
        [("skewed", &[][..], ahead)],
        NOW,
        &StoreRules::default(),
    )
    .unwrap();
    assert_eq!(selected.len(), 1);
}
