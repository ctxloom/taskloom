//! Lint over rows and schemas.

use crate::english::English;
use std::path::PathBuf;

use angzarr_embedded_diagnostic::Diagnostic;
use ports::Timestamp;

use super::{lint, Finding, Violation, SCHEMA_SUBJECT};
use crate::view::{Row, Task};
use std::collections::BTreeMap;

use crate::read::{Reach, Scan};
use crate::store_schema::StoreRules;
use crate::Project;

fn project(id: &str) -> Project {
    Project {
        id: id.to_string(),
        root: None,
        store: PathBuf::new(),
    }
}

/// The task `harp` in `project`, holding `tags`.
fn row(project_id: &str, harp: &str, tags: &[&str]) -> Row {
    Row {
        project: project(project_id),
        task: Task {
            harp_id: harp.to_string(),
            text: "x".to_string(),
            status: "Done".to_string(),
            checked: true,
            text_hash: String::new(),
            trigger: String::new(),
            origin_session: String::new(),
            tags: tags
                .iter()
                .map(|t| crate::parse_tag(t).unwrap().unwrap())
                .collect(),
            created_at: Timestamp::default(),
        },
    }
}

fn schema(declarations: &[&str]) -> StoreRules {
    StoreRules::parsed(declarations)
}

/// `rows` read from the project `p`, whose store holds `rules`.
fn scan(rows: &[Row], rules: &StoreRules) -> Scan {
    Scan {
        reach: Reach::Project(project("p")),
        rows: rows.to_vec(),
        rules: BTreeMap::from([("p".to_string(), rules.clone())]),
    }
}

/// Each violation as `(project, subject, English)`.
fn said(violations: &[Violation]) -> Vec<(Option<&str>, &str, String)> {
    violations
        .iter()
        .map(|v| {
            (
                v.project.as_deref(),
                v.harp_id.as_str(),
                v.finding.english(),
            )
        })
        .collect()
}

#[test]
fn test_a_value_outside_its_enum_is_a_violation_whatever_the_tasks_status() {
    let schema = schema(&[r#"tagma.enum:"area:x"=a"#, r#"tagma.enum:"area:x"=b"#]);
    let (found, checked) = lint(&scan(
        &[row("p", "done-task", &["area:x=c", "area:x=a"])],
        &schema,
    ));
    assert_eq!(
        said(&found),
        [(
            Some("p"),
            "done-task",
            "area:x=c: not-in-enum (allowed=a, allowed=b)".to_string()
        )]
    );
    assert_eq!(checked, 1);
    assert_eq!(found[0].finding.code(), "tasks-application-lint-rule");
}

#[test]
fn test_a_range_targets_value_must_be_comparable_and_within_it() {
    let schema = schema(&[r#"tagma.range:"area:n"=1..5"#]);
    let rows = [
        row("p", "a-task", &["area:n=9"]),
        row("p", "b-task", &["area:n=NaN"]),
        row("p", "c-task", &["area:n=lots"]),
        row("p", "d-task", &["area:n=1", "area:n=5", "area:n=2.5"]),
    ];
    let (found, checked) = lint(&scan(&rows, &schema));
    assert_eq!(
        said(&found),
        [
            (
                Some("p"),
                "a-task",
                "area:n=9: out-of-range (range=1..5)".to_string()
            ),
            (
                Some("p"),
                "b-task",
                "area:n=NaN: not-comparable (range=1..5)".to_string()
            ),
            (
                Some("p"),
                "c-task",
                "area:n=lots: not-comparable (range=1..5)".to_string()
            ),
        ]
    );
    assert_eq!(checked, 1);
}

// One violation a broken rule, its tags together: two values of a scalar
// target are one violation, not two.
#[test]
fn test_a_scalar_target_holding_two_values_is_one_violation() {
    let schema = schema(&[r#"tagma.arity:"area:s"=scalar"#]);
    let rows = [
        row(
            "p",
            "two",
            &["area:s=1", "area:s=2", "area:t=1", "area:t=2"],
        ),
        row("p", "one", &["area:s=1"]),
    ];
    let (found, checked) = lint(&scan(&rows, &schema));
    assert_eq!(
        said(&found),
        [(
            Some("p"),
            "two",
            "area:s=1, area:s=2: arity-exceeded (arity=scalar)".to_string()
        )]
    );
    assert_eq!(checked, 0, "arity is no enum or range");
}

// A closed row's tags are its history: its permits and its keys are not
// judged, every other rule is.
#[test]
fn test_a_closed_rows_permits_and_keys_are_left_alone() {
    let schema = schema(&[
        r#"tagma.permit:"*:*"=false"#,
        "tagma.keys-per-item:area=1",
        r#"tagma.enum:"x:k"=a"#,
        r#"tagma.permit:"x:*"=true"#,
        r#"tagma.permit:"area:*"=true"#,
    ]);
    let mut open = row("p", "open", &["lang:en", "area:a", "area:b", "x:k=b"]);
    open.task.status = "To Do".to_string();
    let closed = row("p", "closed", &["lang:en", "area:a", "area:b", "x:k=b"]);
    let (found, _) = lint(&scan(&[open, closed], &schema));
    let codes: Vec<(&str, String)> = found
        .iter()
        .map(|v| (v.harp_id.as_str(), v.finding.english()))
        .collect();
    assert_eq!(
        codes,
        [
            ("closed", "x:k=b: not-in-enum (allowed=a)".to_string()),
            ("open", "x:k=b: not-in-enum (allowed=a)".to_string()),
            ("open", "lang:en: not-permitted (-)".to_string()),
            (
                "open",
                "area:a, area:b: too-many-keys (namespace=area, limit=1, keys=a, keys=b)"
                    .to_string()
            ),
        ],
        "by task, then by the finding's key and args"
    );
}

// A formula naming a value its target's enum lacks is the schema's
// defect, reported once.
#[test]
fn test_a_formula_naming_a_value_no_enum_declares_is_the_schemas_violation() {
    let schema = schema(&[
        r#"tagma.enum:"area:k"=a"#,
        r#"tagma.enum:"area:k"=b"#,
        r#"taskloom.priority_fn:"area:k"="{{area:k=a}} + {{ area:k=zz }} + {{area:k=zz}} + {{area:k=*}} + {{area:other=q}} + {{age_days}}""#,
        r#"taskloom.decay_fn:"area:k"="{{area:k=yy}}""#,
    ]);
    let (found, _) = lint(&scan(&[], &schema));
    assert_eq!(
        said(&found),
        [
            (
                None,
                SCHEMA_SUBJECT,
                "decay_fn on area:k references the placeholder {{area:k=yy}}, but yy is not one of area:k's declared enum values: a, b".to_string()
            ),
            (
                None,
                SCHEMA_SUBJECT,
                "priority_fn on area:k references the placeholder {{area:k=zz}}, but zz is not one of area:k's declared enum values: a, b".to_string()
            ),
        ],
        "sorted by English, the repeat once"
    );
}

#[test]
fn test_the_seeded_schemas_formulas_name_only_declared_values() {
    let (found, checked) = lint(&scan(&[], &StoreRules::default()));
    assert!(found.is_empty(), "{:?}", said(&found));
    assert!(checked > 0);
}

// Each row is judged by its own store's rules.
#[test]
fn test_violations_sort_by_project_then_task_each_by_its_stores_rules() {
    let strict = schema(&[r#"tagma.enum:"area:x"=a"#, r#"tagma.range:"area:x"=0..1"#]);
    let rows = [
        row("zulu", "a-task", &["area:x=b"]),
        row("alpha", "z-task", &["area:x=b"]),
        row("open", "o-task", &["area:x=b"]),
    ];
    let every = Scan {
        reach: Reach::Every {
            projects: 3,
            fallback: None,
        },
        rows: rows.to_vec(),
        rules: BTreeMap::from([
            ("zulu".to_string(), strict.clone()),
            ("alpha".to_string(), strict.clone()),
            ("open".to_string(), schema(&[])),
        ]),
    };
    let (found, _) = lint(&every);
    let order: Vec<(Option<&str>, &str)> = found
        .iter()
        .map(|v| (v.project.as_deref(), v.harp_id.as_str()))
        .collect();
    assert_eq!(
        order,
        [
            (Some("alpha"), "z-task"),
            (Some("alpha"), "z-task"),
            (Some("zulu"), "a-task"),
            (Some("zulu"), "a-task")
        ]
    );
    let (found, checked) = lint(&scan(&rows[..1], &schema(&[])));
    assert_eq!((found, checked), (Vec::new(), 0));
}

#[test]
fn test_each_finding_names_its_args() {
    let finding = Finding::FormulaEnum {
        facet: "priority_fn".to_string(),
        target: "t".to_string(),
        placeholder: "{{r=v}}".to_string(),
        reference: "r".to_string(),
        value: "v".to_string(),
        members: "a".to_string(),
    };
    let names: Vec<&str> = finding.args().into_iter().map(|(n, _)| n).collect();
    assert_eq!(
        names,
        [
            "facet",
            "target",
            "placeholder",
            "reference",
            "value",
            "members"
        ]
    );
}
