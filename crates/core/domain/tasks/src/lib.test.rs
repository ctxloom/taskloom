//! The rules that need no stream: `check`, `belongs`, a tag's target; the
//! payloads on the wire; the catalog. The rules a stream decides are
//! `aggregate.test.rs`'s, through angzarr-embedded's kernel.

use angzarr_embedded::next::{ids, Payload, PayloadError, Timestamp};
use angzarr_embedded_diagnostic::Diagnostic;

use crate::{
    check, AddTask, EditText, RemovalReason, RemoveTask, Retag, SetStatus, StatusChanged, Tag,
    TaskCommand, TaskEvent, TaskRejection, DEFERRED, TO_DO,
};

const HARP: &str = "swift-amber";

fn at(seconds: i64) -> Timestamp {
    Timestamp { seconds, nanos: 0 }
}

fn add(text: &str) -> AddTask {
    AddTask {
        harp_id: HARP.to_string(),
        text: text.to_string(),
        status: None,
        trigger: None,
        tags: Vec::new(),
        origin_session: Some("calm-otter".to_string()),
        defers: DEFERRED.to_string(),
        initial: TO_DO.to_string(),
    }
}

/// A status change: each of `status` and `trigger` left out when "".
fn status(status: &str, trigger: &str) -> SetStatus {
    let given = |s: &str| (!s.is_empty()).then(|| s.to_string());
    SetStatus {
        status: given(status),
        trigger: given(trigger),
        defers: DEFERRED.to_string(),
        initial: TO_DO.to_string(),
    }
}

fn retag(add: &[&str], remove: &[&str]) -> Retag {
    Retag {
        add: tags(add),
        remove: tags(remove),
    }
}

/// Each of `spelled`, parsed as tagma parses a tag.
fn tags(spelled: &[&str]) -> Vec<Tag> {
    spelled
        .iter()
        .map(|s| tagma_core::Tag::parse(s).expect("a valid tag").into())
        .collect()
}

#[test]
fn test_an_add_whose_root_cannot_be_derived_says_why() {
    let root = ids::compute_root("tasks", HARP).unwrap();
    assert_eq!(
        crate::belongs("bad:domain", HARP, Some(&root.value[..])),
        Err(TaskRejection::NoRoot {
            harp_id: HARP.to_string(),
            cause: "a domain never contains ':'".to_string(),
        })
    );
    assert_eq!(crate::belongs("tasks", HARP, Some(&root.value[..])), Ok(()));
    assert_eq!(
        crate::belongs("tasks", HARP, None),
        Err(TaskRejection::NotItsStream {
            harp_id: HARP.to_string()
        }),
        "a stream with no root is not its stream"
    );
}

#[test]
fn test_check_passes_what_only_the_stream_can_refuse() {
    let passed: Vec<TaskCommand> = vec![
        add("Ship it").into(),
        AddTask {
            harp_id: String::new(),
            status: Some(DEFERRED.to_string()),
            trigger: Some("the release".to_string()),
            ..add("Revisit")
        }
        .into(),
        status(DEFERRED, "").into(),
        EditText {
            text: "x".to_string(),
        }
        .into(),
        retag(&["x"], &[]).into(),
        retag(&[], &["x"]).into(),
        RemoveTask {
            reason: RemovalReason::Removed,
        }
        .into(),
    ];
    for command in passed {
        assert_eq!(check(&command), Ok(()), "{command:?}");
    }
}

#[test]
fn test_an_event_round_trips_through_the_wire() {
    let event = TaskEvent::StatusChanged(StatusChanged {
        status: DEFERRED.to_string(),
        trigger: "after the release".to_string(),
        at: at(5),
        deferred_since: Some(at(5)),
        prior_status: String::new(),
    });
    assert_eq!(TaskEvent::from_any(&event.to_any()), Ok(event));
}

#[test]
fn test_every_rejection_has_a_catalog_message_with_exactly_its_args() {
    use fluent_syntax::ast::{Entry, Expression, InlineExpression, PatternElement};

    let ftl = crate::EN_US.1;
    let resource = fluent_syntax::parser::parse(ftl).expect("the catalog parses");
    let samples = crate::diagnostic_samples();
    // Both ways: every catalog message has a sample, and every sample a
    // message, so a missing or empty sample list fails too.
    let mut catalog: Vec<&str> = resource
        .body
        .iter()
        .filter_map(|e| match e {
            Entry::Message(m) => Some(m.id.name),
            _ => None,
        })
        .collect();
    catalog.sort_unstable();
    let mut sampled: Vec<&str> = samples.iter().map(|s| s.code()).collect();
    sampled.sort_unstable();
    assert_eq!(catalog, sampled, "catalog messages and samples");
    for sample in samples {
        // Display is the key and args, never English (decision 14.37).
        assert_eq!(
            sample.to_string(),
            angzarr_embedded_diagnostic::Report::of(&*sample).to_string(),
            "{}",
            sample.code()
        );
        let message = resource
            .body
            .iter()
            .find_map(|e| match e {
                Entry::Message(m) if m.id.name == sample.code() => Some(m),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no message for {}", sample.code()));
        let mut used: Vec<&str> = message
            .value
            .iter()
            .flat_map(|p| &p.elements)
            .filter_map(|el| match el {
                PatternElement::Placeable {
                    expression: Expression::Inline(InlineExpression::VariableReference { id }),
                } => Some(id.name),
                _ => None,
            })
            .collect();
        used.sort_unstable();
        let mut args: Vec<&str> = sample.args().iter().map(|(n, _)| *n).collect();
        args.sort_unstable();
        assert_eq!(used, args, "{}", sample.code());
    }
}

#[test]
fn test_a_tags_target_is_its_namespace_and_key() {
    let tag = |ns: Option<&str>, key: &str| Tag {
        namespace: ns.map(str::to_string),
        key: key.to_string(),
        value: Some("v".to_string()),
    };
    assert_eq!(tag(Some("triage"), "kind").target(), "triage:kind");
    assert_eq!(tag(None, "urgent").target(), "urgent");
}

#[test]
fn test_a_removal_without_a_reason_is_an_invalid_payload() {
    let unspecified = angzarr_embedded::next::pack(&tasks_proto::v1::RemoveTask { reason: 0 });
    let Err(PayloadError::Invalid { reason, .. }) = TaskCommand::from_any(&unspecified) else {
        panic!("an unspecified reason is no domain value");
    };
    assert_eq!(reason.code(), "angzarr-embedded-next-wire-unknown-enum");
}

#[test]
fn test_a_payload_of_another_type_is_unknown() {
    let other = angzarr_embedded::next::pack(&tasks_proto::v1::TaskState::default());
    assert_eq!(
        TaskCommand::from_any(&other),
        Err(PayloadError::UnknownType {
            type_url: "/ctxloom.tasks.v1.TaskState".to_string()
        })
    );
    assert!(TaskEvent::from_any(&other).is_err());
}

#[test]
fn test_every_command_round_trips_through_the_wire() {
    let commands: Vec<TaskCommand> = vec![
        add("Ship it").into(),
        status("Done", "").into(),
        EditText {
            text: "x".to_string(),
        }
        .into(),
        retag(&["x"], &["y"]).into(),
        RemoveTask {
            reason: RemovalReason::Pruned,
        }
        .into(),
    ];
    for command in commands {
        assert_eq!(
            TaskCommand::from_any(&command.to_any()),
            Ok(command.clone())
        );
    }
    assert_eq!(TaskCommand::type_urls().len(), 5);
    assert_eq!(TaskEvent::type_urls().len(), 5);
}

#[test]
fn test_a_rejection_displays_as_its_key_and_args() {
    assert_eq!(
        TaskRejection::TaskExists {
            harp_id: HARP.to_string()
        }
        .to_string(),
        "tasks-task-exists harp_id=swift-amber"
    );
    assert_eq!(
        TaskRejection::TriggerRequired.to_string(),
        "tasks-trigger-required"
    );
}

#[test]
fn test_a_task_state_round_trips_through_its_wire_form() {
    let state = crate::TaskState {
        harp_id: HARP.to_string(),
        text: "Ship it".to_string(),
        status: DEFERRED.to_string(),
        trigger: "after the release".to_string(),
        origin_session: "calm-otter".to_string(),
        tags: tags(&["urgent", "area:x=1"]),
        created_at: Timestamp {
            seconds: 5,
            nanos: 0,
        },
        status_set_at: Timestamp {
            seconds: 6,
            nanos: 0,
        },
        deferred_since: Some(Timestamp {
            seconds: 6,
            nanos: 0,
        }),
        removed: true,
        prior_status: TO_DO.to_string(),
    };
    let wire = tasks_proto::v1::TaskState::from(state.clone());
    assert_eq!(wire.harp_id, HARP);
    assert_eq!(crate::TaskState::try_from(wire.clone()), Ok(state));
    let missing = tasks_proto::v1::TaskState {
        created_at: None,
        ..wire
    };
    assert_eq!(
        crate::TaskState::try_from(missing),
        Err(angzarr_embedded::next::WireError::Missing {
            message: "ctxloom.tasks.v1.TaskState",
            field: "created_at"
        })
    );
}

// The schema stream's payloads go on the wire as the tasks' do, each
// under its own type url and back whole.
#[test]
fn test_the_schema_payloads_round_trip_under_their_own_type_urls() {
    use crate::{Declare, SchemaCommand, SchemaEvent, SchemaRetagged};

    let tag = |s: &str| Tag::from(tagma_core::Tag::parse(s).unwrap());
    let event = SchemaEvent::SchemaRetagged(SchemaRetagged {
        tags: vec![tag("tagma.keys-per-item:area=1")],
    });
    let any = event.to_any();
    assert_eq!(any.type_url, "/ctxloom.tasks.v1.SchemaRetagged");
    assert_eq!(SchemaEvent::from_any(&any), Ok(event));
    let command = SchemaCommand::Declare(Declare {
        add: vec![tag("tagma.keys-per-item:repo=1")],
        remove: Vec::new(),
    });
    assert_eq!(SchemaCommand::from_any(&command.to_any()), Ok(command));
    assert_eq!(SchemaCommand::type_urls(), ["/ctxloom.tasks.v1.Declare"]);
    assert_eq!(
        SchemaEvent::type_urls(),
        ["/ctxloom.tasks.v1.SchemaRetagged"]
    );
}
