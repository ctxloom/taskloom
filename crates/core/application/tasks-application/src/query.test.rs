//! Tag queries: tagma-core held to the reference answers.
//!
//! `fixtures/tag-queries/cases.tsv` holds the cases; `go.tsv` is the
//! reference answer to each, written by
//! `scripts/compare-go-tag-queries.sh`. Every query answers as the
//! reference does; both read postfix only, so an infix case is refused.

use std::collections::{BTreeMap, BTreeSet};

use angzarr_embedded_diagnostic::Diagnostic;

use super::{check, select, QueryError, QUERY_GRAMMAR};
use crate::schema::TagSchema;
use crate::tags::parse;
use crate::Tag;

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
    let schema = TagSchema::parse(&scenario.schema).expect("the schema parses");
    let candidates = scenario
        .tasks
        .iter()
        .filter(|(_, status, _)| scenario.statuses.is_empty() || scenario.statuses.contains(status))
        .map(|(harp, _, tags)| (harp.as_str(), tags.as_slice()));
    select(query, candidates, &schema)
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
    assert_eq!(error.code(), "tasks-application-tag-query");
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
        assert_eq!(error.code(), "tasks-application-tag-query");
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
        [("a", &tags[..]), ("b", &[][..])],
        &TagSchema::default(),
    );
    assert_eq!(selected.unwrap().len(), 2);
}

// The schema's type declarations are indexed, but never answer a query:
// the item holding them is no candidate.
#[test]
fn test_the_type_declarations_are_never_selected() {
    let schema = TagSchema::parse([r#"tagma.type:"triage:x"=semver"#]).unwrap();
    let selected = select(Some("*:*"), [("a", &[][..])], &schema).unwrap();
    assert_eq!(selected, BTreeSet::from(["a".to_string()]));
    let selected = select(Some("tagma.type:*"), [("a", &[][..])], &schema).unwrap();
    assert!(selected.is_empty());
}
