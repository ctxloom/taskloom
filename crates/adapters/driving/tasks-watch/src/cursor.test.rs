//! The cursor's text: what it encodes, what it refuses on its own content.

use base64::Engine;
use serde_json::json;
use tasks_application::{Positions, StreamKey};
use uuid::Uuid;

use super::{encode, name, Cursor, BASE64, VERSION};
use crate::WatchError;

const ROOT: Uuid = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
const OTHER: Uuid = Uuid::from_u128(7);

fn main_stream(root: Uuid) -> StreamKey {
    StreamKey {
        edition: None,
        root,
    }
}

fn texted(json: &serde_json::Value) -> String {
    BASE64.encode(json.to_string())
}

fn parsed(json: &serde_json::Value) -> Result<Cursor, WatchError> {
    Cursor::parse(&texted(json))
}

#[test]
fn test_a_cursor_is_base64url_of_json_with_its_version_project_and_streams() {
    let positions: Positions = [(main_stream(ROOT), 3), (main_stream(OTHER), 1)].into();
    let text = encode("jolly-mute-magma", &positions);
    assert!(
        text.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "url-safe and unpadded: {text}"
    );
    let bytes = BASE64.decode(&text).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json,
        json!({
            "v": 1,
            "project": "jolly-mute-magma",
            "streams": {
                "tasks/01234567-89ab-cdef-0123-456789abcdef": 3,
                "tasks/00000000-0000-0000-0000-000000000007": 1,
            }
        })
    );
    // Key order is fixed, so the same position is the same text.
    assert_eq!(text, encode("jolly-mute-magma", &positions));
    assert_eq!(VERSION, 1);
}

#[test]
fn test_a_cursor_round_trips_with_an_edition() {
    let edition = StreamKey {
        edition: Some("draft".to_string()),
        root: ROOT,
    };
    assert_eq!(
        name(&edition),
        "tasks/01234567-89ab-cdef-0123-456789abcdef@draft"
    );
    let positions: Positions = [(main_stream(OTHER), 2), (edition, 9)].into();
    let cursor = Cursor::parse(&encode("p", &positions)).unwrap();
    assert_eq!(cursor.project, "p");
    assert_eq!(cursor.positions, positions);
}

#[test]
fn test_a_cursor_with_no_streams_is_valid() {
    let cursor = Cursor::parse(&encode("p", &Positions::new())).unwrap();
    assert!(cursor.positions.is_empty());
}

#[test]
fn test_padding_is_tolerated_on_the_way_in() {
    let text = encode("pp", &Positions::new());
    let padded = format!("{text}{}", "=".repeat((4 - text.len() % 4) % 4));
    assert_eq!(Cursor::parse(&padded), Cursor::parse(&text));
    assert!(Cursor::parse(&text).is_ok());
}

#[test]
fn test_what_is_not_a_cursor_is_malformed() {
    for bad in ["", "not a cursor", "%%%%", "42", "a.b.c", "e30"] {
        assert_eq!(
            Cursor::parse(bad),
            Err(WatchError::CursorMalformed),
            "{bad}"
        );
    }
    // Decodes, but is not JSON, or not an object.
    for bad in [
        texted(&json!(null)),
        BASE64.encode("{"),
        texted(&json!([1])),
    ] {
        assert_eq!(
            Cursor::parse(&bad),
            Err(WatchError::CursorMalformed),
            "{bad}"
        );
    }
}

#[test]
fn test_a_cursor_missing_a_member_or_with_a_wrong_type_is_malformed() {
    let streams = json!({});
    for bad in [
        json!({"project": "p", "streams": streams}),
        json!({"v": "1", "project": "p", "streams": streams}),
        json!({"v": 1, "streams": streams}),
        json!({"v": 1, "project": "p"}),
        json!({"v": 1, "project": 4, "streams": streams}),
        json!({"v": 1, "project": "p", "streams": [1]}),
        json!({"v": 1, "project": "p", "streams": {"tasks/01234567-89ab-cdef-0123-456789abcdef": "1"}}),
        json!({"v": 1, "project": "p", "streams": {"tasks/01234567-89ab-cdef-0123-456789abcdef": -1}}),
    ] {
        assert_eq!(parsed(&bad), Err(WatchError::CursorMalformed), "{bad}");
    }
}

#[test]
fn test_a_stream_that_is_no_task_streams_name_is_malformed() {
    for key in [
        "schema/01234567-89ab-cdef-0123-456789abcdef",
        "tasks",
        "tasks/",
        "tasks/not-a-uuid",
        "tasks/01234567-89ab-cdef-0123-456789abcdef@",
        "tasks@draft",
        "taskss/01234567-89ab-cdef-0123-456789abcdef",
        "01234567-89ab-cdef-0123-456789abcdef",
    ] {
        let bad = json!({"v": 1, "project": "p", "streams": {key: 1}});
        assert_eq!(parsed(&bad), Err(WatchError::CursorMalformed), "{key}");
    }
}

#[test]
fn test_a_version_other_than_the_supported_is_refused_naming_it_whatever_follows() {
    for found in [0, 2, 999] {
        // A later version may hold anything else: it is the version that
        // is refused, not its shape.
        let later = json!({"v": found, "somewhere": ["else"]});
        assert_eq!(
            parsed(&later),
            Err(WatchError::CursorVersion {
                found,
                supported: 1
            })
        );
    }
}

#[test]
fn test_a_cursor_is_the_projects_it_was_taken_from() {
    let cursor = Cursor::parse(&encode("jolly-mute-magma", &Positions::new())).unwrap();
    assert_eq!(cursor.check_project("jolly-mute-magma"), Ok(()));
    assert_eq!(
        cursor.check_project("swift-amber-falcon"),
        Err(WatchError::CursorForeign {
            cursor_project: "jolly-mute-magma".to_string(),
            project: "swift-amber-falcon".to_string(),
        })
    );
}

#[test]
fn test_members_a_later_build_adds_are_ignored() {
    let extra = json!({"v": 1, "project": "p", "streams": {}, "later": true});
    assert!(parsed(&extra).is_ok());
}
