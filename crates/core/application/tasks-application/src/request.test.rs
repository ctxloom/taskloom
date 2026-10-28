//! The derivation of a request's command ids.

use super::{request_command_id, REQUEST_NAMESPACE};

fn bytes(id: &annals::next::v1::Uuid) -> [u8; 16] {
    <[u8; 16]>::try_from(&id.value[..]).expect("16 bytes")
}

#[test]
fn test_a_command_id_is_uuidv5_of_the_operation_and_the_request_id() {
    // Python: uuid.uuid5(NS, "set_status\0retry-me-1"), NS the namespace.
    assert_eq!(
        bytes(&request_command_id("set_status", "retry-me-1")),
        [158, 247, 218, 106, 69, 87, 88, 70, 182, 101, 75, 10, 97, 205, 0, 37]
    );
    assert_eq!(
        bytes(&request_command_id("add", "retry-me-1")),
        [160, 148, 135, 70, 174, 75, 91, 6, 183, 41, 54, 110, 119, 118, 154, 203]
    );
}

#[test]
fn test_one_request_is_one_command_id_and_another_operation_another() {
    let id = |op: &str, key: &str| bytes(&request_command_id(op, key));
    assert_eq!(id("tag", "r-1"), id("tag", "r-1"), "a retry sends the same");
    assert_ne!(id("tag", "r-1"), id("edit", "r-1"));
    assert_ne!(id("tag", "r-1"), id("tag", "r-2"));
    let v5 = id("tag", "r-1");
    assert_eq!(v5[6] >> 4, 5, "version 5");
    assert_eq!(v5[8] >> 6, 0b10, "the RFC 9562 variant");
}

#[test]
fn test_the_namespace_is_the_url_named_one() {
    // uuid.uuid5(uuid.NAMESPACE_URL, "https://github.com/ctxloom/taskloom/request-id")
    assert_eq!(REQUEST_NAMESPACE[6] >> 4, 5);
}
