//! Virtual tags: which a query names, and what each is derived to.

use ports::Timestamp;

use super::{
    derive, mentioned, resolve_durations, whole_seconds, DurationRefusal, Facts, Mentioned, Why,
    AGE, CHECKED, DEFERRED_FOR, HAS_TRIGGER, IN_STATUS, ORIGIN, PRIORITY, STATUS, VIRTUAL_TAGS,
};
use duration_text::DurationError;

const NOW: Timestamp = Timestamp {
    seconds: 1_700_000_000,
    nanos: 0,
};

fn ago(seconds: i64) -> Timestamp {
    Timestamp {
        seconds: NOW.seconds - seconds,
        nanos: 0,
    }
}

fn names(query: &str) -> Vec<&'static str> {
    let found = mentioned(Some(query));
    VIRTUAL_TAGS
        .iter()
        .map(|v| v.name)
        .filter(|name| found.has(name))
        .collect()
}

#[test]
fn test_a_query_names_the_virtual_tags_it_has_an_atom_for() {
    assert_eq!(names("taskloom:status=Done"), [STATUS]);
    assert_eq!(
        names("taskloom:priority>=3/area:cli/and"),
        [PRIORITY],
        "an atom among others"
    );
    assert_eq!(
        names("taskloom:in-status>=24/taskloom:age<5/or/taskloom:has-trigger/not/and"),
        [IN_STATUS, AGE, HAS_TRIGGER]
    );
    assert_eq!(names("taskloom:checked"), [CHECKED]);
    assert_eq!(names("taskloom:origin~abc"), [ORIGIN]);
    assert_eq!(names("taskloom:deferred=+"), [DEFERRED_FOR]);
}

#[test]
fn test_a_name_ends_at_the_operator_after_it() {
    for query in [
        "taskloom:status=Done",
        "taskloom:status!=Done",
        "taskloom:status~D",
        "taskloom:status",
        "taskloom:status/not",
        "taskloom:status>=1",
        "taskloom:status<1",
        "taskloom:status>1",
        "taskloom:status<=1",
    ] {
        assert_eq!(names(query), [STATUS], "{query}");
    }
    assert_eq!(
        names("taskloom:in-status>=1"),
        [IN_STATUS],
        "not status as well: hours is part of the name"
    );
}

#[test]
fn test_a_query_without_one_names_none() {
    assert_eq!(mentioned(None), Mentioned::default());
    for query in [
        "human:decision",
        "urgent/not",
        "taskloom.internal:candidate",
        "taskloom.priority_fn:x",
        "xtaskloom:status",
        "my.taskloom:status",
        "taskloom:nope",
        "taskloom",
        "*:status",
        "area:taskloom",
    ] {
        assert!(names(query).is_empty(), "{query}");
    }
}

#[test]
fn test_taskloom_star_names_every_virtual_tag() {
    assert_eq!(names("taskloom:*").len(), VIRTUAL_TAGS.len());
    assert_eq!(names("taskloom:*/not").len(), VIRTUAL_TAGS.len());
}

#[test]
fn test_a_tag_after_a_slash_or_a_quote_is_still_named() {
    assert_eq!(names("a:b/taskloom:origin=x/and"), [ORIGIN]);
    assert_eq!(names("area:cli/taskloom:checked/not/and"), [CHECKED]);
}

fn facts() -> Facts<'static> {
    Facts {
        id: "t",
        tags: &[],
        status: "Deferred",
        trigger: "after release",
        checked: true,
        origin_session: "sess-1",
        created_at: ago(3 * 86_400 + 5),
        status_set_at: ago(7200 + 59),
        deferred_since: Some(ago(90_000)),
        priority: Some(2.5),
    }
}

fn all() -> Mentioned {
    mentioned(Some("taskloom:*"))
}

fn spelled(tags: &[tagma_core::Tag]) -> Vec<String> {
    tags.iter().map(|t| t.to_string()).collect()
}

#[test]
fn test_each_virtual_tag_is_derived_from_the_facts() {
    assert_eq!(
        spelled(&derive(&facts(), &all(), NOW)),
        [
            "taskloom:deferred=90000",
            "taskloom:in-status=7259",
            "taskloom:age=259205",
            "taskloom:status=Deferred",
            "taskloom:has-trigger",
            "taskloom:checked",
            "taskloom:origin=sess-1",
            "taskloom:priority=2.5",
        ]
    );
}

#[test]
fn test_only_the_virtual_tags_a_query_names_are_derived() {
    let one = |query: &str| spelled(&derive(&facts(), &mentioned(Some(query)), NOW));
    assert_eq!(one("taskloom:status=x"), ["taskloom:status=Deferred"]);
    assert_eq!(one("taskloom:priority>1"), ["taskloom:priority=2.5"]);
    assert_eq!(
        one("taskloom:age>1/taskloom:checked/and"),
        ["taskloom:age=259205", "taskloom:checked"]
    );
    assert!(one("area:cli").is_empty(), "no atom, no tag");
}

#[test]
fn test_a_fact_a_task_lacks_derives_no_tag() {
    let bare = Facts {
        trigger: "",
        checked: false,
        origin_session: "",
        created_at: Timestamp::default(),
        status_set_at: Timestamp::default(),
        deferred_since: None,
        priority: None,
        ..facts()
    };
    assert_eq!(
        spelled(&derive(&bare, &all(), NOW)),
        ["taskloom:status=Deferred"],
        "no trigger, mark, origin or instant to measure from, and no priority computed"
    );
}

#[test]
fn test_elapsed_seconds_are_whole_and_never_negative() {
    assert_eq!(whole_seconds(ago(0), NOW), 0);
    assert_eq!(whole_seconds(ago(59), NOW), 59);
    assert_eq!(whole_seconds(ago(-7200), NOW), 0, "after now");
    let half = Timestamp {
        seconds: NOW.seconds - 5,
        nanos: 500_000_000,
    };
    assert_eq!(whole_seconds(half, NOW), 4, "the fraction is dropped");
}

#[test]
fn test_age_is_the_age_days_the_formulas_read_in_seconds() {
    let created = ago(5 * 86_400 + 7 * 3600 + 11);
    let seconds = whole_seconds(created, NOW);
    let days = task_priority::seconds_since(created, NOW) / 86_400.0;
    assert_eq!(seconds, (days * 86_400.0).round() as i64);
    assert_eq!(seconds, 457_211);
}

// ------------------------------------------------------------ durations

const MINUTE: i64 = 60;
const HOUR: i64 = 3600;
const DAY: i64 = 86_400;
const WEEK: i64 = 7 * DAY;

/// Whether a task `elapsed` seconds in its status is selected by
/// `taskloom:in-status<op><literal>`, read at NOW.
fn holds(op: &str, literal: &str, elapsed: i64) -> bool {
    let query = format!("taskloom:in-status{op}{literal}");
    let facts = Facts {
        status_set_at: ago(elapsed),
        ..facts()
    };
    let selected = crate::query::select(Some(&query), [facts], NOW, &Default::default());
    !selected.expect(&query).is_empty()
}

/// Each unit with its length in seconds.
const UNITS: [(&str, i64); 5] = [
    ("s", 1),
    ("m", MINUTE),
    ("h", HOUR),
    ("d", DAY),
    ("w", WEEK),
];

// The comparison is exact on the elapsed whole seconds (see `rule`).
#[test]
fn test_gte_and_gt_compare_the_elapsed_seconds_exactly() {
    for (unit, u) in UNITS {
        let lit = format!("2{unit}");
        assert!(holds(">=", &lit, 2 * u), "{lit}: exactly");
        assert!(!holds(">=", &lit, 2 * u - 1), "{lit}: a second short");
        assert!(holds(">", &lit, 2 * u + 1), "{lit}: a second over");
        assert!(!holds(">", &lit, 2 * u), "{lit}: exactly is not more");
    }
    assert!(
        !holds(">=", "2d", 47 * HOUR + 59 * MINUTE),
        "47h59m is not 2d"
    );
}

#[test]
fn test_lte_and_lt_compare_the_elapsed_seconds_exactly() {
    for (unit, u) in UNITS {
        let lit = format!("2{unit}");
        assert!(holds("<=", &lit, 2 * u), "{lit}: exactly");
        assert!(!holds("<=", &lit, 2 * u + 1), "{lit}: a second over");
        assert!(holds("<", &lit, 2 * u - 1), "{lit}: a second short");
        assert!(!holds("<", &lit, 2 * u), "{lit}: exactly is not less");
    }
    assert!(
        holds("<", "2d", 47 * HOUR + 59 * MINUTE),
        "47h59m is under 2d"
    );
}

#[test]
fn test_zero_is_a_duration_with_a_unit_and_every_operator_reads_it() {
    for (unit, _) in UNITS {
        let lit = format!("0{unit}");
        assert!(holds(">=", &lit, 0), "{lit}");
        assert!(!holds(">", &lit, 0), "{lit}");
        assert!(holds(">", &lit, 1), "{lit}");
        assert!(!holds("<", &lit, 0), "{lit}: nothing is less than nothing");
        assert!(holds("<=", &lit, 0), "{lit}");
        assert!(!holds("<=", &lit, 1), "{lit}");
    }
}

#[test]
fn test_the_literal_may_be_a_chain_a_fraction_or_a_date() {
    assert!(holds(">=", "1h30m", 5400) && !holds(">=", "1h30m", 5399));
    assert!(holds(">=", "1.5h", 5400) && !holds(">=", "1.5h", 5399));
    assert!(
        holds(">=", "1w", 604_800) && !holds(">=", "1w", 604_799),
        "a week is 7 days"
    );
    assert!(
        holds(">=", "1d", 86_400) && !holds(">=", "1d", 86_399),
        "a day is 24 hours"
    );
    // NOW is 2023-11-14T22:13:20Z: 80000 s past its midnight.
    assert!(holds(">=", "2023-11-14", 80_000));
    assert!(!holds(">=", "2023-11-14", 79_999));
    assert!(holds("<", "2023-11-14", 79_999));
    assert!(!holds("<", "2023-11-14", 80_000));
}

#[test]
fn test_a_task_without_the_tag_matches_neither_gte_nor_lt() {
    // Never deferred: the tag is absent.
    let never = Facts {
        deferred_since: None,
        ..facts()
    };
    for query in [
        "taskloom:deferred>=0s",
        "taskloom:deferred<1w",
        "taskloom:deferred>0s",
        "taskloom:deferred<=1w",
    ] {
        let selected = crate::query::select(Some(query), [never.clone()], NOW, &Default::default());
        assert!(selected.unwrap().is_empty(), "{query}");
    }
    let present = [Facts {
        deferred_since: Some(ago(5)),
        ..facts()
    }];
    let selected = crate::query::select(
        Some("taskloom:deferred>=0s"),
        present,
        NOW,
        &Default::default(),
    );
    assert_eq!(selected.unwrap().len(), 1);
}

#[test]
fn test_a_future_instant_has_been_zero_seconds() {
    for (op, literal, expected) in [(">=", "0s", true), ("<", "1s", true), (">", "0s", false)] {
        assert_eq!(holds(op, literal, -7200), expected, "{op}{literal}");
    }
}

#[test]
fn test_equals_not_equals_and_match_are_refused_pointing_at_ranges() {
    for name in ["deferred", "in-status", "age"] {
        for atom in ["=2d", "!=2d", "~2d", "=+", "=*", "=0d", "=30"] {
            let query = format!("taskloom:{name}{atom}");
            let error = crate::query::select(Some(&query), [facts()], NOW, &Default::default())
                .unwrap_err();
            let crate::query::QueryRefusal::Duration(refused) = error else {
                panic!("{query}: {error:?}")
            };
            assert_eq!(refused.why, Why::Operator, "{query}");
            assert_eq!(refused.atom, query);
        }
    }
}

#[test]
fn test_a_bare_number_is_refused_for_every_duration_tag_and_operator() {
    for name in ["deferred", "in-status", "age"] {
        for op in [">", ">=", "<", "<="] {
            for literal in ["30", "0", "1.5", "-3"] {
                let query = format!("taskloom:{name}{op}{literal}");
                let error = crate::query::select(Some(&query), [facts()], NOW, &Default::default())
                    .unwrap_err();
                let crate::query::QueryRefusal::Duration(refused) = error else {
                    panic!("{query}: {error:?}")
                };
                assert!(
                    matches!(
                        &refused.why,
                        Why::Literal(
                            DurationError::Malformed { .. } | DurationError::Negative { .. }
                        )
                    ),
                    "{query}: {refused:?}"
                );
                assert_eq!(refused.atom, query);
                assert_eq!(refused.query, query);
            }
        }
    }
}

#[test]
fn test_a_literal_that_is_no_duration_is_refused_naming_the_atom() {
    for literal in ["soon", "d", "2x", "2d3", "2023-13-45", "1mo", "1.5d"] {
        let query = format!("area:cli/taskloom:age>={literal}/and");
        let error =
            crate::query::select(Some(&query), [facts()], NOW, &Default::default()).unwrap_err();
        let crate::query::QueryRefusal::Duration(refused) = error else {
            panic!("{literal}: {error:?}")
        };
        assert!(matches!(refused.why, Why::Literal(_)), "{literal}");
        assert_eq!(refused.atom, format!("taskloom:age>={literal}"));
    }
}

#[test]
fn test_the_refusal_is_coded_with_its_args() {
    use coded_diagnostic::Diagnostic;
    let refused = DurationRefusal {
        query: "q".to_string(),
        atom: "taskloom:age>=30".to_string(),
        why: Why::Literal(DurationError::Malformed {
            text: "30".to_string(),
            ext_err_txt: String::new(),
        }),
    };
    assert_eq!(
        refused.code(),
        coded_diagnostic::code!("tag-query-duration")
    );
    let names: Vec<&str> = refused.args().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["query", "atom", "scope", "why", "units"]);
    assert!(refused.to_string().contains("taskloom:age>=30"));
}

#[test]
fn test_presence_and_other_tokens_are_left_as_written() {
    for query in [
        "taskloom:age",
        "area:cli/taskloom:in-status/and",
        "taskloom:status=Done",
        r#"x:"a/b"/taskloom:status=Done/and"#,
    ] {
        assert_eq!(
            resolve_durations(query, NOW).as_deref(),
            Ok(query),
            "{query}"
        );
    }
}

#[test]
fn test_only_a_duration_tags_atom_is_resolved() {
    assert_eq!(
        resolve_durations("taskloom:status>=2d", NOW).as_deref(),
        Ok("taskloom:status>=2d"),
        "status is text"
    );
    assert_eq!(
        resolve_durations("taskloom:age>=2d/area:cli/and", NOW).as_deref(),
        Ok("taskloom:age>=172800/area:cli/and")
    );
    assert_eq!(
        resolve_durations("taskloom:age<=1d/taskloom:age>12h/and", NOW).as_deref(),
        Ok("taskloom:age<86401/taskloom:age>43200/and")
    );
}

#[test]
fn test_the_refusal_says_why_in_its_args() {
    use coded_diagnostic::Diagnostic;
    let why = |refused: &DurationRefusal| {
        let args = refused.args();
        let (_, arg) = args.iter().find(|(name, _)| *name == "why").unwrap();
        format!("{arg:?}")
    };
    let operator = resolve_durations("taskloom:age=2d", NOW).unwrap_err();
    assert!(why(&operator).contains("operator"), "{}", why(&operator));
    let bare = resolve_durations("taskloom:age>=30", NOW).unwrap_err();
    let args = bare.args();
    let (_, arg) = args.iter().find(|(name, _)| *name == "why").unwrap();
    let coded_diagnostic::Arg::Nested(cause) = arg else {
        panic!("the cause travels as a nested diagnostic: {arg:?}")
    };
    assert_eq!(
        cause.code(),
        DurationError::Malformed {
            text: String::new(),
            ext_err_txt: String::new(),
        }
        .code()
    );
}

#[test]
fn test_a_malformed_duration_is_refused_in_the_causes_own_words() {
    use crate::english::English;
    let bare = resolve_durations("taskloom:age>=30", NOW).unwrap_err();
    let english = bare.english();
    assert!(english.contains("30 is not a span"), "{english}");
    assert!(!english.contains("duration-text-"), "{english}");
    let repeated = resolve_durations("taskloom:age>=1h1h", NOW).unwrap_err();
    assert!(
        repeated.english().contains("1h1h"),
        "{}",
        repeated.english()
    );
}

#[test]
fn test_a_query_that_is_the_atom_says_it_once() {
    use crate::english::English;
    let alone = resolve_durations("taskloom:age>=30", NOW).unwrap_err();
    assert!(
        alone
            .english()
            .starts_with("the atom taskloom:age>=30, which"),
        "{}",
        alone.english()
    );
    assert_eq!(alone.english().matches("taskloom:age>=30").count(), 1);
    let among = resolve_durations("area:cli/taskloom:age>=30/and", NOW).unwrap_err();
    assert!(
        among.english().starts_with(
            "the tag query area:cli/taskloom:age>=30/and has the atom taskloom:age>=30, which"
        ),
        "{}",
        among.english()
    );
}

#[test]
fn test_a_slash_inside_quotes_does_not_split_a_token() {
    // The quoted value holds what looks like a refused atom: left alone.
    let query = r#"taskloom:origin="x/taskloom:age>=30"/and"#;
    assert_eq!(resolve_durations(query, NOW).unwrap(), query);
}

#[test]
fn test_a_slash_after_a_closed_quote_splits_again() {
    let resolved = resolve_durations(r#"taskloom:origin="a"/taskloom:age>=1h/and"#, NOW).unwrap();
    assert_eq!(resolved, r#"taskloom:origin="a"/taskloom:age>=3600/and"#);
}

#[test]
fn test_a_namespace_with_no_name_names_no_virtual_tag() {
    assert!(names("taskloom:").is_empty());
    assert!(names("taskloom:=x").is_empty());
    assert!(names("taskloom:/area:cli/and").is_empty());
}

#[test]
fn test_the_status_meaning_says_the_classes_are_for_the_status_flag_alone() {
    // `taskloom:status=@terminal` is a malformed token to tagma: only
    // `--status` takes the classes, so the table must not promise them here.
    let query = crate::query::select(
        Some("taskloom:status=@terminal"),
        [facts()],
        NOW,
        &Default::default(),
    );
    assert!(query.is_err(), "the premise: a class is no status value");
    let meaning = VIRTUAL_TAGS
        .iter()
        .find(|v| v.name == STATUS)
        .unwrap()
        .meaning;
    assert!(
        meaning.contains("--status alone takes the classes @open and @terminal"),
        "{meaning}"
    );
    assert!(!meaning.contains("classes too"), "{meaning}");
}

fn refused_unknown(query: &str) -> crate::query::QueryRefusal {
    crate::query::select(Some(query), [facts()], NOW, &Default::default()).unwrap_err()
}

#[test]
fn test_a_name_in_the_taskloom_namespace_that_is_no_virtual_tag_is_refused() {
    use crate::english::English;
    use coded_diagnostic::Diagnostic;
    for query in [
        "taskloom:foo",
        "taskloom:deferd>=1d",
        "taskloom:status-x=Done",
        "taskloom:Age>=1d",
        "area:cli/taskloom:nope=1/and",
        " taskloom:nope ",
    ] {
        let error = refused_unknown(query);
        assert_eq!(
            error.code(),
            coded_diagnostic::code!("tag-query-unknown-virtual"),
            "{query}"
        );
        let english = error.english();
        for known in VIRTUAL_TAGS {
            assert!(english.contains(known.name), "{query}: {english}");
        }
    }
    let alone = refused_unknown("taskloom:deferd").english();
    assert_eq!(
        alone,
        format!(
            "the atom taskloom:deferd is not a virtual tag: deferd is not one of the names the taskloom namespace holds ({})",
            VIRTUAL_TAGS.map(|v| v.name).join(", ")
        )
    );
    let among = refused_unknown("area:cli/taskloom:deferd>=1d/and").english();
    assert!(
        among.starts_with(
            "the tag query area:cli/taskloom:deferd>=1d/and has the atom taskloom:deferd>=1d, which is not a virtual tag: deferd is not one of"
        ),
        "{among}"
    );
}

#[test]
fn test_known_names_the_wildcard_and_other_namespaces_are_not_refused() {
    let select = |query: &str| {
        crate::query::select(Some(query), [facts()], NOW, &Default::default()).map(|ids| ids.len())
    };
    for known in VIRTUAL_TAGS {
        let query = format!("taskloom:{}", known.name);
        assert!(select(&query).is_ok(), "{query}");
    }
    assert_eq!(select("taskloom:*"), Ok(1));
    assert_eq!(select("taskloom.internal:candidate"), Ok(1));
    assert_eq!(select("taskloom.internal:type-config"), Ok(0));
    assert_eq!(select("taskloom.other:foo"), Ok(0));
    assert_eq!(select("area:taskloom"), Ok(0));
    assert_eq!(select("x:taskloom"), Ok(0));
    assert_eq!(select(r#"taskloom:origin="taskloom:foo""#), Ok(0));
    assert_eq!(select(r#"taskloom:origin="a/taskloom:foo"/not"#), Ok(1));
}

#[test]
fn test_a_stored_tag_in_the_namespace_is_ignored_and_a_known_name_still_reads_the_fact() {
    let stored = [crate::parse_tag("taskloom:checked").unwrap().unwrap()];
    let mut old = Facts::bare("old", &stored);
    old.checked = false;
    let selected =
        crate::query::select(Some("taskloom:checked"), [old], NOW, &Default::default()).unwrap();
    assert!(
        selected.is_empty(),
        "a stored taskloom:checked stands for nothing"
    );
}
