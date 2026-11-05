//! The templates and `schema init`'s pure step.

use tagma_core::Tag;

use super::*;
use crate::rules::{StoreRules, SEEDED_SCHEMA};

fn rules(spelled: &[&str]) -> StoreRules {
    StoreRules::parsed(spelled)
}

fn spelled(tags: &[Tag]) -> Vec<String> {
    tags.iter().map(ToString::to_string).collect()
}

fn permits(schema: &[&str]) -> Vec<String> {
    schema
        .iter()
        .filter_map(|s| s.strip_prefix("tagma.permit:"))
        .map(|s| s.trim_matches('"').to_string())
        .collect()
}

#[test]
fn test_the_default_seed_is_the_general_set() {
    assert_eq!(SEEDED_SCHEMA, GENERAL_SCHEMA);
    assert_eq!(Template::default(), Template::General);
}

#[test]
fn test_every_template_tag_is_tagma_grammar_and_unique() {
    for template in [Template::General, Template::Ctxloom] {
        let tags = wanted(template, &[]);
        assert_eq!(tags.len(), template.schema().len());
        let mut sorted = spelled(&tags);
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            tags.len(),
            "{}: no tag twice",
            template.name()
        );
        let unpermitted: Vec<String> = StoreRules::of_tags(tags.into_iter())
            .check()
            .iter()
            .map(|finding| finding.tag.to_string())
            .collect();
        // The general set permits `area:` itself: clean. The ctxloom one
        // leaves `area` and `repo` for the project to permit.
        let expected: &[&str] = match template {
            Template::General => &[],
            Template::Ctxloom => &["tagma.keys-per-item:area=1", "tagma.keys-per-item:repo=1"],
        };
        assert_eq!(unpermitted, expected, "{}", template.name());
    }
}

#[test]
fn test_template_names() {
    assert_eq!(Template::General.name(), "general");
    assert_eq!(Template::Ctxloom.name(), "ctxloom");
}

#[test]
fn test_the_general_set_is_exactly_the_rulings() {
    let permitted = permits(&GENERAL_SCHEMA);
    let mut expected = vec![
        "triage:level",
        "triage:kind",
        "triage:effort",
        "triage:blocks-release",
        "triage:verdict",
        "triage:security",
        "triage:data-loss",
        "triage:crashes",
        "triage:no-workaround",
        "triage:regression",
        "triage:silent-no-op",
        "triage:flaky",
        "human:decision",
        "human:action",
        "human:authority",
        "human:verify",
        "human:prerequisite",
        "human:voice",
        "relates:*",
        "touches:*",
        "queue:*",
        "area:*",
        "icebox",
        "someday-maybe",
        "review:*",
    ];
    expected.sort_unstable();
    let mut got: Vec<&str> = permitted.iter().map(String::as_str).collect();
    got.sort_unstable();
    assert_eq!(got, expected);
    let all = GENERAL_SCHEMA.join("\n");
    for out in [
        "exposed",
        "blind-gate",
        "audited",
        "landed",
        "repo",
        "platform",
        "sig",
        "decay_fn",
    ] {
        assert!(!all.contains(out), "{out} is out of the general default");
    }
    assert!(all.contains("taskloom.priority_fn"));
    assert!(all.contains("tagma.keys-per-item:area=1"));
}

#[test]
fn test_the_ctxloom_template_is_the_old_seed_and_permits_no_area() {
    assert_eq!(CTXLOOM_SCHEMA.len(), 67);
    let permitted = permits(&CTXLOOM_SCHEMA);
    assert!(!permitted.iter().any(|p| p.starts_with("area:")));
    assert!(!permitted.iter().any(|p| p.starts_with("repo:")));
    assert!(permitted.contains(&"landed:*".to_string()));
    assert!(permitted.contains(&"platform:linux".to_string()));
    assert!(CTXLOOM_SCHEMA
        .iter()
        .any(|s| s.starts_with("taskloom.decay_fn")));
}

#[test]
fn test_areas_close_the_namespace_to_exactly_those_keys() {
    let open = wanted(Template::General, &[]);
    assert!(spelled(&open).contains(&r#"tagma.permit:"area:*""#.to_string()));
    let areas = ["area:cli".to_string(), "area:docs".to_string()];
    let closed = spelled(&wanted(Template::General, &areas));
    assert!(!closed.contains(&r#"tagma.permit:"area:*""#.to_string()));
    assert!(closed.contains(&r#"tagma.permit:"area:cli""#.to_string()));
    assert!(closed.contains(&r#"tagma.permit:"area:docs""#.to_string()));
    assert_eq!(closed.len(), GENERAL_SCHEMA.len() - 1 + 2);
    // The ctxloom template has no open area permit to replace: the keys
    // are added to it.
    let ctxloom = spelled(&wanted(Template::Ctxloom, &areas));
    assert_eq!(ctxloom.len(), CTXLOOM_SCHEMA.len() + 2);
}

#[test]
fn test_missing_from_nothing_is_everything() {
    let all = wanted(Template::General, &[]);
    assert_eq!(
        missing(&StoreRules::of_tags(std::iter::empty()), &all),
        Ok(all)
    );
}

#[test]
fn test_missing_is_only_what_is_not_held_and_a_second_run_finds_none() {
    let all = wanted(Template::General, &[]);
    let held = StoreRules::of_tags(all.iter().cloned());
    assert_eq!(missing(&held, &all), Ok(Vec::new()));
    let partial = StoreRules::of_tags(all.iter().skip(3).cloned());
    let found = missing(&partial, &all).unwrap();
    assert_eq!(found, all[..3].to_vec());
}

#[test]
fn test_a_store_permitting_area_keys_is_not_opened() {
    let held = rules(&[r#"tagma.permit:"area:cli""#]);
    let found = missing(&held, &wanted(Template::General, &[])).unwrap();
    assert!(!spelled(&found).contains(&r#"tagma.permit:"area:*""#.to_string()));
    assert_eq!(found.len(), GENERAL_SCHEMA.len() - 1);
}

#[test]
fn test_a_store_with_area_open_is_not_closed() {
    let held = rules(&[r#"tagma.permit:"area:*""#]);
    let areas = ["area:cli".to_string()];
    let conflict = missing(&held, &wanted(Template::General, &areas)).unwrap_err();
    assert_eq!(conflict.held, vec![r#"tagma.permit:"area:*""#.to_string()]);
}

#[test]
fn test_more_area_keys_may_be_added_to_a_closed_set() {
    let held = rules(&[r#"tagma.permit:"area:cli""#]);
    let areas = ["area:cli".to_string(), "area:docs".to_string()];
    let found = missing(&held, &wanted(Template::General, &areas)).unwrap();
    assert!(spelled(&found).contains(&r#"tagma.permit:"area:docs""#.to_string()));
    assert!(!spelled(&found).contains(&r#"tagma.permit:"area:cli""#.to_string()));
}

#[test]
fn test_another_value_for_a_single_valued_target_is_a_conflict() {
    for held in [
        r#"tagma.range:"triage:level"=1..9"#,
        r#"tagma.arity:"triage:level"=many"#,
        r#"tagma.type:"triage:blocks-release"=string"#,
        r#"tagma.keys-per-item:area=2"#,
        r#"taskloom.priority_fn:"triage:kind"="1""#,
    ] {
        let conflict = missing(&rules(&[held]), &wanted(Template::General, &[])).unwrap_err();
        assert_eq!(
            conflict.held,
            vec![Tag::parse(held).unwrap().to_string()],
            "{held}"
        );
    }
}

#[test]
fn test_more_enum_members_are_not_a_conflict() {
    let held = rules(&[r#"tagma.enum:"triage:kind"=spike"#]);
    let found = missing(&held, &wanted(Template::General, &[])).unwrap();
    assert_eq!(found.len(), GENERAL_SCHEMA.len());
}

#[test]
fn test_the_general_template_over_the_ctxloom_one_conflicts_on_the_formula_alone() {
    let held = StoreRules::of_tags(wanted(Template::Ctxloom, &[]).into_iter());
    let conflict = missing(&held, &wanted(Template::General, &[])).unwrap_err();
    assert_eq!(conflict.held.len(), 1);
    assert!(conflict.held[0].starts_with("taskloom.priority_fn"));
}

#[test]
fn test_the_ctxloom_template_adds_to_the_general_default_but_for_the_formula() {
    let held = StoreRules::of_tags(wanted(Template::General, &[]).into_iter());
    let conflict = missing(&held, &wanted(Template::Ctxloom, &[])).unwrap_err();
    assert_eq!(conflict.held.len(), 1);
}

#[test]
fn test_a_permit_of_another_namespace_does_not_stand_for_an_area_permit() {
    // Valued or not, a permit that is not an area's leaves `area:*` to add.
    for held in [
        r#"tagma.permit:"lang:*""#,
        r#"tagma.permit:"human:action""#,
        r#"tagma.permit:"lang:*"=true"#,
        r#"tagma.permit:"area:cli"=true"#,
    ] {
        let found = missing(&rules(&[held]), &wanted(Template::General, &[])).unwrap();
        assert!(
            spelled(&found).contains(&r#"tagma.permit:"area:*""#.to_string()),
            "{held}"
        );
    }
}
