//! The wire types carry the names annals routes by, and round-trip.

use super::v1;
use prost::{Message, Name};

// annals routes and upcasts by type URL, so each message must carry its
// own full name with angzarr's "/" prefix.
#[test]
fn test_type_url_names_each_message_by_full_name() {
    assert_eq!(v1::TaskAdded::type_url(), "/ctxloom.tasks.v1.TaskAdded");
    assert_eq!(v1::AddTask::type_url(), "/ctxloom.tasks.v1.AddTask");
    assert_eq!(v1::TaskState::type_url(), "/ctxloom.tasks.v1.TaskState");
}

#[test]
fn test_event_round_trips_through_its_encoding() {
    let tag = |key: &str| v1::Tag {
        namespace: None,
        key: key.to_string(),
        value: None,
    };
    let event = v1::Retagged {
        tags: vec![tag("later"), tag("urgent")],
    };
    let decoded = v1::Retagged::decode(event.encode_to_vec().as_slice()).expect("decode");
    assert_eq!(decoded, event);
}

// The tags are tagma's own message, not a copy of it.
#[test]
fn test_tags_are_tagmas_message() {
    let tag = tagma_proto::v1::Tag {
        namespace: Some("geo".to_string()),
        key: "lat".to_string(),
        value: Some("57.64".to_string()),
    };
    let state = v1::TaskState {
        tags: vec![tag.clone()],
        ..v1::TaskState::default()
    };
    let decoded = v1::TaskState::decode(state.encode_to_vec().as_slice()).expect("decode");
    assert_eq!(decoded.tags, vec![tag]);
}

// A store export expands the stored payloads by name: the set holds every
// message the tasks store keeps, and the files they import.
#[test]
fn test_the_descriptor_set_holds_every_stored_message_and_its_imports() {
    let set = prost_types::FileDescriptorSet::decode(super::FILE_DESCRIPTOR_SET).expect("decodes");
    let names: Vec<String> = set
        .file
        .iter()
        .filter(|f| f.package() == "ctxloom.tasks.v1")
        .flat_map(|f| f.message_type.iter().map(|m| m.name().to_string()))
        .collect();
    for stored in [
        "TaskAdded",
        "StatusChanged",
        "TextEdited",
        "Retagged",
        "TaskRemoved",
        "SchemaRetagged",
        "TaskState",
        "SchemaState",
    ] {
        assert!(names.iter().any(|n| n == stored), "{stored} in {names:?}");
    }
    let files: Vec<&str> = set.file.iter().map(|f| f.name()).collect();
    assert!(files.contains(&"tagma/v1/tag.proto"), "{files:?}");
}
