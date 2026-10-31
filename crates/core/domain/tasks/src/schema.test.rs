//! RED (red/self-hosted-schema): the schema stream's decide and apply.

use angzarr_embedded::next::ids;

use super::{
    apply_schema, declared, schema_root, Declare, SchemaRejection, SchemaRetagged, SchemaState,
    SCHEMA_DOMAIN, SCHEMA_STREAM,
};
use crate::{Tag, TASK_DOMAIN};

fn tags(spelled: &[&str]) -> Vec<Tag> {
    spelled
        .iter()
        .map(|s| tagma_core::Tag::parse(s).expect("a valid tag").into())
        .collect()
}

fn spelled(tags: &[Tag]) -> Vec<String> {
    tags.iter().map(Tag::to_string).collect()
}

fn held(spelled: &[&str]) -> SchemaState {
    SchemaState {
        tags: tags(spelled),
        declared: true,
    }
}

#[test]
fn test_the_schema_stream_is_one_fixed_root_outside_the_tasks_domain() {
    assert_ne!(SCHEMA_DOMAIN, TASK_DOMAIN);
    let derived = ids::compute_root(SCHEMA_DOMAIN, SCHEMA_STREAM).unwrap();
    assert_eq!(schema_root(), derived);
    assert_eq!(schema_root(), schema_root(), "one root per store");
}

#[test]
fn test_a_declare_records_the_whole_tag_set_after_it_normalized() {
    let held = held(&[r#"tagma.enum:"triage:kind"=defect"#]);
    let declare = Declare {
        add: tags(&[
            r#"tagma.enum:"triage:kind"=chore"#,
            r#"tagma.permit:"*:*"=false"#,
        ]),
        remove: vec![],
    };
    let event = declared(&held, &declare).unwrap().expect("a change");
    assert_eq!(
        spelled(&event.tags),
        [
            r#"tagma.enum:"triage:kind"=chore"#,
            r#"tagma.enum:"triage:kind"=defect"#,
            r#"tagma.permit:"*:*"=false"#,
        ]
    );
}

#[test]
fn test_a_declare_removes_after_it_adds_as_a_task_retag_does() {
    let held = held(&[r#"tagma.enum:"triage:kind"=defect"#]);
    let both = tags(&[r#"tagma.enum:"triage:kind"=chore"#]);
    let declare = Declare {
        add: both.clone(),
        remove: [both, tags(&[r#"tagma.enum:"triage:kind"=defect"#])].concat(),
    };
    let event = declared(&held, &declare).unwrap().expect("a change");
    assert!(event.tags.is_empty(), "{:?}", spelled(&event.tags));
}

#[test]
fn test_a_declare_that_changes_nothing_records_nothing() {
    let held = held(&[r#"tagma.keys-per-item:area=1"#]);
    let declare = Declare {
        add: tags(&[r#"tagma.keys-per-item:area=1"#]),
        remove: tags(&[r#"tagma.range:"x:y"=1..5"#]),
    };
    assert_eq!(declared(&held, &declare).unwrap(), None);
}

#[test]
fn test_a_declare_of_nothing_is_refused() {
    let refused = declared(&SchemaState::default(), &Declare::default()).unwrap_err();
    assert_eq!(refused, SchemaRejection::TagRequired);
    assert_eq!(
        angzarr_embedded_diagnostic::Diagnostic::code(&refused),
        "tasks-schema-tag-required"
    );
}

// The stream holds rules, never a task's tags: only `tagma.*` and
// `taskloom.*` (and their dot-descendants) are declared.
#[test]
fn test_a_declare_of_a_tag_outside_the_config_namespaces_is_refused() {
    for tag in ["area:cli", "urgent", "tagmax:x=1", "taskloomish:y"] {
        let declare = Declare {
            add: tags(&[tag]),
            remove: vec![],
        };
        let refused = declared(&SchemaState::default(), &declare).unwrap_err();
        assert_eq!(
            refused,
            SchemaRejection::NotConfig {
                tag: tag.to_string()
            },
            "{tag}"
        );
    }
    for tag in [
        r#"tagma.permit:"area:*"=true"#,
        r#"taskloom.priority_fn:"triage:kind"="1""#,
        "tagma.sub.facet:x=1",
    ] {
        let declare = Declare {
            add: tags(&[tag]),
            remove: vec![],
        };
        assert!(declared(&SchemaState::default(), &declare).is_ok(), "{tag}");
    }
}

// A declare is the store's to judge (tagma's `check`, `taskloom schema
// check`), not decide's: an unknown facet or a malformed value is recorded.
#[test]
fn test_decide_records_a_malformed_config_tag_for_check_to_report() {
    let declare = Declare {
        add: tags(&[
            r#"tagma.arty:"x:y"=scalar"#,
            r#"tagma.range:"x:y"=nonsense"#,
        ]),
        remove: vec![],
    };
    let event = declared(&SchemaState::default(), &declare)
        .unwrap()
        .expect("recorded");
    assert_eq!(event.tags.len(), 2);
}

#[test]
fn test_apply_assigns_the_tag_set_and_marks_the_stream_declared() {
    let mut state = held(&["tagma.keys-per-item:area=1"]);
    state.declared = false;
    let event = SchemaRetagged {
        tags: tags(&["tagma.keys-per-item:repo=1"]),
    };
    apply_schema(&mut state, &event);
    assert_eq!(spelled(&state.tags), ["tagma.keys-per-item:repo=1"]);
    assert!(state.declared);
}

#[test]
fn test_a_removal_of_every_rule_still_leaves_the_stream_declared() {
    let held = held(&["tagma.keys-per-item:area=1"]);
    let declare = Declare {
        add: vec![],
        remove: tags(&["tagma.keys-per-item:area=1"]),
    };
    let event = declared(&held, &declare).unwrap().expect("a change");
    let mut state = held.clone();
    apply_schema(&mut state, &event);
    assert!(state.tags.is_empty());
    assert!(state.declared, "an emptied schema is not reseeded");
}
