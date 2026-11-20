//! The scalar collapse a retag sends resolved.

use super::{collapse, parse, Tag};
use crate::rules::StoreRules;

fn tags(spelled: &[&str]) -> Vec<Tag> {
    spelled.iter().map(|s| parse(s).unwrap().unwrap()).collect()
}

/// `triage:type` declared scalar, in a store permitting every tag.
fn scalar() -> StoreRules {
    StoreRules::parsed(&[
        r#"tagma.arity:"triage:type"=scalar"#,
        r#"tagma.permit:"*:*""#,
    ])
}

#[test]
fn test_a_new_scalar_value_displaces_the_held_one() {
    let (add, displaced) = collapse(
        &scalar(),
        &tags(&["triage:type=bug", "other"]),
        tags(&["triage:type=feature"]),
    );
    assert_eq!(add, tags(&["triage:type=feature"]));
    assert_eq!(displaced, tags(&["triage:type=bug"]));
}

#[test]
fn test_the_same_scalar_value_displaces_nothing_however_spelled() {
    let (add, displaced) = collapse(
        &scalar(),
        &tags(&["triage:type=bug"]),
        tags(&["triage:type=\"bug\""]),
    );
    assert_eq!(add, tags(&["triage:type=bug"]));
    assert!(displaced.is_empty());
}

// tagma's own collapse, inside validate, keeps the last value too: the
// two never disagree about what a write leaves (SPEC §8, §13).
#[test]
fn test_only_the_last_of_several_scalar_values_is_added() {
    let (add, displaced) = collapse(
        &scalar(),
        &[],
        tags(&["triage:type=a", "x", "triage:type=b", "triage:type=c"]),
    );
    assert_eq!(add, tags(&["x", "triage:type=c"]));
    assert!(displaced.is_empty());
    assert!(scalar().validate(&[], &add, &[]).is_empty());
}

#[test]
fn test_an_undeclared_target_keeps_every_value() {
    let given = tags(&["area:x=1", "area:x=2", "type=a", "type=b"]);
    let (add, displaced) = collapse(&scalar(), &tags(&["area:x=0"]), given.clone());
    assert_eq!(add, given);
    assert!(displaced.is_empty());
    let (add, displaced) = collapse(
        &StoreRules::parsed(&[]),
        &tags(&["triage:type=a"]),
        tags(&["triage:type=b"]),
    );
    assert_eq!(add, tags(&["triage:type=b"]));
    assert!(displaced.is_empty(), "no arity, no collapse");
}

#[test]
fn test_a_set_arity_beside_a_scalar_one_is_scalar_the_restrictive_reading() {
    let rules = StoreRules::parsed(&[
        r#"tagma.arity:"triage:type"=scalar"#,
        r#"tagma.arity:"triage:type"=set"#,
    ]);
    let (_, displaced) = collapse(&rules, &tags(&["triage:type=a"]), tags(&["triage:type=b"]));
    assert_eq!(displaced, tags(&["triage:type=a"]));
    let set = StoreRules::parsed(&[r#"tagma.arity:"triage:type"=set"#]);
    let (_, displaced) = collapse(&set, &tags(&["triage:type=a"]), tags(&["triage:type=b"]));
    assert!(displaced.is_empty());
}

#[test]
fn test_a_valueless_scalar_tag_displaces_a_valued_one() {
    let (_, displaced) = collapse(&scalar(), &tags(&["triage:type=a"]), tags(&["triage:type"]));
    assert_eq!(displaced, tags(&["triage:type=a"]));
}
