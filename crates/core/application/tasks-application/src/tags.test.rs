//! Tag strings parsed at the edge of a write.

use super::{parse, parse_all, Tag, TagRefusal};
use crate::english::English;

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

fn tag(namespace: Option<&str>, key: &str, value: Option<&str>) -> Tag {
    Tag {
        namespace: namespace.map(str::to_string),
        key: key.to_string(),
        value: value.map(str::to_string),
    }
}

#[test]
fn test_a_tag_parses_into_its_components() {
    assert_eq!(
        parse(" geo:lat=57.64 "),
        Some(Ok(tag(Some("geo"), "lat", Some("57.64"))))
    );
    assert_eq!(parse("urgent"), Some(Ok(tag(None, "urgent", None))));
}

#[test]
fn test_an_empty_tag_is_no_tag() {
    assert_eq!(parse("   "), None);
    assert_eq!(parse(""), None);
}

#[test]
fn test_a_tag_outside_the_grammar_is_refused_with_why() {
    let Some(Err(TagRefusal::Unparseable { tag, detail })) = parse(" a b ") else {
        panic!("refused");
    };
    assert_eq!(tag, "a b", "trimmed");
    assert!(detail.contains("\"a b\""), "{detail}");
}

#[test]
fn test_every_tag_is_parsed_in_order() {
    assert_eq!(
        parse_all(&strings(&["b", "a=1"])),
        Ok(vec![tag(None, "b", None), tag(None, "a", Some("1"))])
    );
    assert!(matches!(
        parse_all(&strings(&["ok", "not/ok", "a b"])),
        Err(TagRefusal::Unparseable { tag, .. }) if tag == "not/ok"
    ));
}

// A tag given to a write is a tag given: blank, it is refused, never left
// out, so `--tag "$UNSET"` cannot write a task without the tag it meant.
#[test]
fn test_a_blank_tag_given_is_refused_not_left_out() {
    for blank in ["", "   "] {
        assert_eq!(
            parse_all(&strings(&["b", blank, "a=1"])),
            Err(TagRefusal::Blank)
        );
    }
}

#[test]
fn test_a_refusal_renders_its_english() {
    let refusal = TagRefusal::Unparseable {
        tag: "a b".to_string(),
        detail: "why".to_string(),
    };
    assert_eq!(
        refusal.english(),
        "the tag a b is not a tag in tagma's grammar: why"
    );
}

// ------------------------------------------------------------ admission
//
// What every task's tag must pass whatever the store's rules. What a
// value may be (enum, range, type), how many a target holds and whether a
// tag is permitted are tagma's to judge (store_schema.test.rs).

use super::{admit_all, collapse, schema_free};
use crate::store_schema::StoreRules;

fn refused(spelled: &str) -> Option<TagRefusal> {
    match parse(spelled)? {
        Err(refusal) => Some(refusal),
        Ok(tag) => schema_free(&tag).err(),
    }
}

#[test]
fn test_an_operator_word_is_refused_in_any_case() {
    for word in [
        "and", "AND", "And", "or", "OR", "not", "NOT", " and ", "\"and\"",
    ] {
        assert!(
            matches!(refused(word), Some(TagRefusal::OperatorWord { .. })),
            "{word}"
        );
    }
    // Only the bare word: with a namespace or a value, a query can match it.
    for fine in ["x:and", "and=1", "android", "cannot", "note", "north"] {
        assert_eq!(refused(fine), None, "{fine}");
    }
}

#[test]
fn test_a_tag_outside_the_grammar_is_refused() {
    for bad in ["urgent*", "+", "*", "foo/bar", "has space"] {
        assert!(
            matches!(refused(bad), Some(TagRefusal::Unparseable { .. })),
            "{bad}"
        );
    }
}

#[test]
fn test_ordinary_tags_are_admitted() {
    for tag in [
        "urgent",
        "release",
        "android",
        "cannot",
        "note",
        "north",
        "v1.2",
        "a-b_c",
        "release+",
        "1.0.0+build.5",
        "+1",
        "-1",
        "tagmatic:foo=bar",
        "taskloomish:foo",
        "triage:kind=sparkles",
    ] {
        assert_eq!(refused(tag), None, "{tag}");
    }
}

#[test]
fn test_the_tagma_and_taskloom_namespaces_are_the_schemas() {
    for (tag, root) in [
        (r#"tagma.arity:"triage:type"=scalar"#, "tagma"),
        ("tagma:foo", "tagma"),
        ("tagma.priority_fn:x=y", "tagma"),
        ("taskloom:x", "taskloom"),
        (r#"taskloom.priority_fn:"x:y"="1""#, "taskloom"),
    ] {
        let Some(TagRefusal::ReservedNamespace { namespace, .. }) = refused(tag) else {
            panic!("{tag} refused");
        };
        assert!(namespace.starts_with(root), "{namespace}");
    }
}

#[test]
fn test_admitting_every_tag_stops_at_the_first_refused() {
    let given = strings(&["ok", "tagma:x", "and"]);
    assert!(matches!(
        admit_all(&given),
        Err(TagRefusal::ReservedNamespace { .. })
    ));
    assert_eq!(
        admit_all(&strings(&["ok"])),
        Ok(vec![tag(None, "ok", None)])
    );
    assert_eq!(admit_all(&strings(&["ok", " "])), Err(TagRefusal::Blank));
}

// ------------------------------------------------------------ collapse

fn tags(spelled: &[&str]) -> Vec<Tag> {
    spelled.iter().map(|s| parse(s).unwrap().unwrap()).collect()
}

fn scalar() -> StoreRules {
    StoreRules::parsed(&[r#"tagma.arity:"triage:type"=scalar"#])
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
