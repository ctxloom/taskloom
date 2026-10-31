//! RED (red/self-hosted-schema): the schema read model.

use tasks::schema::SchemaRetagged;
use tasks::Tag;

use super::SchemaView;

fn retagged(spelled: &[&str]) -> SchemaRetagged {
    SchemaRetagged {
        tags: spelled
            .iter()
            .map(|s| Tag::from(tagma_core::Tag::parse(s).unwrap()))
            .collect(),
    }
}

#[test]
fn test_the_view_holds_the_last_projected_tag_set_whole() {
    let mut view = SchemaView::new();
    view.project(0, &retagged(&["tagma.keys-per-item:area=1"]));
    view.project(1, &retagged(&["tagma.keys-per-item:repo=1"]));
    assert_eq!(view.export(), ["tagma.keys-per-item:repo=1"]);
}

#[test]
fn test_an_event_already_projected_changes_nothing() {
    let mut view = SchemaView::new();
    view.project(0, &retagged(&["tagma.keys-per-item:area=1"]));
    view.project(1, &retagged(&["tagma.keys-per-item:repo=1"]));
    view.project(0, &retagged(&["tagma.keys-per-item:area=1"]));
    assert_eq!(view.export(), ["tagma.keys-per-item:repo=1"]);
}

// Export is tagma syntax, one config tag a line, so it reads back with
// tagma's own parser and nothing of taskloom's.
#[test]
fn test_export_is_tagma_syntax_sorted_and_reads_back_through_tagma() {
    let mut view = SchemaView::new();
    view.project(
        0,
        &retagged(&[
            r#"tagma.range:"triage:level"=1..5"#,
            r#"tagma.enum:"triage:kind"=defect"#,
            r#"taskloom.priority_fn:"triage:kind"="1 + {{triage:effort}}""#,
        ]),
    );
    let lines = view.export();
    let mut sorted = lines.clone();
    sorted.sort();
    assert_eq!(lines, sorted);
    let reread: Vec<tagma_core::Tag> = lines
        .iter()
        .map(|l| tagma_core::Tag::parse(l).unwrap())
        .collect();
    assert_eq!(reread, view.tags());
}

// The view hands tagma its tags and tagma interprets them: no rule is
// read by taskloom code.
#[test]
fn test_the_views_tags_are_what_tagmas_rules_are_built_from() {
    let mut view = SchemaView::new();
    view.project(0, &retagged(&["tagma.keys-per-item:area=1"]));
    let rules = tagma_core::Rules::from_tags(&view.tags());
    let current: Vec<tagma_core::Tag> = vec![tagma_core::Tag::parse("area:cli").unwrap()];
    let change = tagma_core::Change {
        add: vec![tagma_core::Tag::parse("area:core").unwrap()],
        remove: vec![],
    };
    let findings = rules.validate(&current, &change);
    assert!(findings.iter().any(|f| f.code.as_str() == "too-many-keys"));
}
