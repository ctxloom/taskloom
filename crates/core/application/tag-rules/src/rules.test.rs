//! The store's rules as read: the default, a template, and the pre-F6
//! migration, pinned to the ctxloom template's permits.

use tasks::schema::SchemaState;
use tasks::Tag;

use super::*;
use crate::template::{Template, CTXLOOM_SCHEMA, GENERAL_SCHEMA};

fn state(declared: bool, spelled: &[&str]) -> SchemaState {
    SchemaState {
        tags: spelled
            .iter()
            .map(|s| Tag::from(tagma_core::Tag::parse(s).unwrap()))
            .collect(),
        declared,
    }
}

fn sorted(spelled: &[&str]) -> Vec<String> {
    StoreRules::parsed(spelled).export()
}

fn ctxloom_rules_without_permits() -> Vec<&'static str> {
    CTXLOOM_SCHEMA
        .iter()
        .copied()
        .filter(|spelled| !spelled.starts_with("tagma.permit:"))
        .collect()
}

fn ctxloom_permits() -> Vec<&'static str> {
    CTXLOOM_SCHEMA
        .iter()
        .copied()
        .filter(|spelled| spelled.starts_with("tagma.permit:"))
        .collect()
}

#[test]
fn test_the_default_rules_are_the_general_template() {
    assert_eq!(StoreRules::default().export(), sorted(&GENERAL_SCHEMA));
    assert_eq!(
        StoreRules::default(),
        StoreRules::of_template(Template::General)
    );
    assert_eq!(
        StoreRules::of_template(Template::Ctxloom).export(),
        sorted(&CTXLOOM_SCHEMA)
    );
    assert_eq!(SEEDED_SCHEMA, GENERAL_SCHEMA);
}

#[test]
fn test_a_stream_with_no_history_is_read_as_the_default_and_one_with_history_as_held() {
    assert_eq!(
        StoreRules::of(&state(false, &[])),
        StoreRules::of_template(Template::General)
    );
    let held = StoreRules::of(&state(true, &CTXLOOM_SCHEMA));
    assert_eq!(held.export(), sorted(&CTXLOOM_SCHEMA), "not the default");
    assert_eq!(
        StoreRules::of(&state(true, &GENERAL_SCHEMA)).export(),
        sorted(&GENERAL_SCHEMA)
    );
}

// A store holding no permit as a presence is from before F6: it is read
// with the permits every store was then seeded with (the ctxloom
// template's), never the default's, its `=true` permits kept as
// presences and its other valued ones dropped.
#[test]
fn test_a_store_from_before_f6_is_read_with_the_ctxloom_templates_permits() {
    let mut old = ctxloom_rules_without_permits();
    old.extend([
        r#"tagma.permit:"*:*"=false"#,
        r#"tagma.permit:"area:cli"=true"#,
        r#"tagma.permit:"lang:*"=true"#,
        r#"tagma.permit:"zone:*"=maybe"#,
    ]);
    let read = StoreRules::of(&state(true, &old));
    let mut expected = ctxloom_rules_without_permits();
    expected.extend(ctxloom_permits());
    expected.extend([r#"tagma.permit:"area:cli""#, r#"tagma.permit:"lang:*""#]);
    assert_eq!(read.export(), sorted(&expected));
    let exported = read.export();
    assert!(!exported.iter().any(|t| t.contains("zone")), "{exported:?}");
    assert!(
        !exported.contains(&r#"tagma.permit:"area:*""#.to_string()),
        "the default's permits are not added to an old store"
    );
}

#[test]
fn test_a_store_with_a_presence_permit_is_not_migrated() {
    let mut held = ctxloom_rules_without_permits();
    held.push(r#"tagma.permit:"lang:*""#);
    held.push(r#"tagma.permit:"zone:*"=true"#);
    let read = StoreRules::of(&state(true, &held));
    assert_eq!(read.export(), sorted(&held), "read as it holds them");
}
