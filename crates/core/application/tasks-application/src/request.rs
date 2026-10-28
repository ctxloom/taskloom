//! A request's key: the id the caller gives a request, and gives again
//! when it retries it, from which the command the request sends takes its
//! command id. No edge mints one: the key is the caller's to keep and
//! resend. A retried request sends the same command id, and annals answers
//! a command it has already applied DUPLICATE: the retry applies nothing,
//! and answers with the task as it stands.
//!
//! PROPOSAL (red): the derivation is pinned by tests; its body is not
//! written yet (the uuid crate's `v5` feature, approved 2026-10-27).

use annals::next::v1::Uuid;

/// The namespace of request-derived command ids: UUIDv5 of the URL
/// `https://github.com/ctxloom/taskloom/request-id` in RFC 9562's URL
/// namespace, 6e69619b-b8b3-5db1-abb7-68cbf8734a6b.
pub const REQUEST_NAMESPACE: [u8; 16] = [
    110, 105, 97, 155, 184, 179, 93, 177, 171, 183, 104, 203, 248, 115, 74, 107,
];

/// The command id the request `request_id` sends for `operation` (`add`,
/// `set_status`, `tag`, `edit`): UUIDv5 (RFC 9562) in
/// [`REQUEST_NAMESPACE`] of `<operation>\0<request_id>`, so one key reused
/// for another operation is another command.
pub fn request_command_id(operation: &str, request_id: &str) -> Uuid {
    let _ = (operation, request_id);
    todo!(
        "PROPOSAL (idempotency, red): UUIDv5 of `<operation>\\0<request_id>` in REQUEST_NAMESPACE"
    )
}

#[cfg(test)]
#[path = "request.test.rs"]
mod tests;
