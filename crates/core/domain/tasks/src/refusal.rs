//! A rejection as a typed refusal, and back.
//!
//! The rules (`lib.rs`, `schema.rs`) refuse with a [`TaskRejection`] or a
//! [`SchemaRejection`]: a stable code, args and English, which the edge
//! prints. On the wire a command refuses with a typed refusal MESSAGE of
//! the protos (`proto/ctxloom/tasks/v1`), one per rejection, listed in the
//! `refuses` of the commands that raise it. The `refuse!` macro turns the one
//! into the other for a handler, and
//! `TryFrom<&TypedRefusal>` turns it back for the application, keyed by the
//! refusal's TYPE and by nothing else: the business status a refusal travels
//! under is its message's `(refusal_status)` option, read here from the
//! generated constant, and never decides what a person is told.

use std::fmt;

use angzarr_embedded::markup::TypedRefusal;

use crate::schema::SchemaRejection;
use crate::{
    Blank, HarpIdRequired, NoTask, Removed, SchemaNotConfig, SchemaTagRequired, StatusRequired,
    StatusReserved, TagRequired, TaskExists, TaskRejection, TextRequired, TriggerRequired,
    TriggerWithStatus, Unresolved,
};

/// What a command handler fails with for `rejection`, a rule's refusal:
/// the typed refusal message of the variant, one of the command's
/// `refuses(..)` choice `$choice` (its `From` each listed message), under
/// the status the message declares. A rejection the command lists no
/// message for is a bug of the handler, not a refusal.
macro_rules! refuse {
    ($rejection:ident, $value:expr, $choice:ty, [$($variant:ident $({$($field:ident),*})? => $message:ident),* $(,)?]) => {
        match $value {
            $(
                $rejection::$variant $({$($field),*})? => ::angzarr_embedded::markup::Failed::Rejected {
                    rejection: <$choice>::from($crate::$message { $($($field),*)? }),
                    status: <$crate::$message as $crate::Refusal>::REFUSAL_STATUS,
                },
            )*
            #[allow(unreachable_patterns)]
            other => ::angzarr_embedded::markup::Failed::bug(&other),
        }
    };
}

pub(crate) use refuse;

/// A typed refusal that is none of this domain's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NotOurs;

impl fmt::Display for NotOurs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a refusal of the tasks domain")
    }
}

impl std::error::Error for NotOurs {}

/// The rejection a task command's typed refusal is, by its type.
impl TryFrom<&TypedRefusal> for TaskRejection {
    type Error = NotOurs;

    fn try_from(refusal: &TypedRefusal) -> Result<Self, NotOurs> {
        if refusal.object::<HarpIdRequired>().is_some() {
            return Ok(TaskRejection::HarpIdRequired);
        }
        if let Some(TaskExists { harp_id }) = refusal.object() {
            return Ok(TaskRejection::TaskExists { harp_id });
        }
        if refusal.object::<NoTask>().is_some() {
            return Ok(TaskRejection::NoTask);
        }
        if let Some(Removed { harp_id }) = refusal.object() {
            return Ok(TaskRejection::Removed { harp_id });
        }
        if refusal.object::<TextRequired>().is_some() {
            return Ok(TaskRejection::TextRequired);
        }
        if refusal.object::<StatusRequired>().is_some() {
            return Ok(TaskRejection::StatusRequired);
        }
        if let Some(StatusReserved { status }) = refusal.object() {
            return Ok(TaskRejection::StatusReserved { status });
        }
        if refusal.object::<TriggerRequired>().is_some() {
            return Ok(TaskRejection::TriggerRequired);
        }
        if refusal.object::<TagRequired>().is_some() {
            return Ok(TaskRejection::TagRequired);
        }
        if let Some(Blank { field }) = refusal.object() {
            return Ok(TaskRejection::Blank { field });
        }
        if let Some(TriggerWithStatus { status, defers }) = refusal.object() {
            return Ok(TaskRejection::TriggerWithStatus { status, defers });
        }
        if let Some(Unresolved { field }) = refusal.object() {
            return Ok(TaskRejection::Unresolved { field });
        }
        Err(NotOurs)
    }
}

/// The rejection a declare's typed refusal is, by its type.
impl TryFrom<&TypedRefusal> for SchemaRejection {
    type Error = NotOurs;

    fn try_from(refusal: &TypedRefusal) -> Result<Self, NotOurs> {
        if refusal.object::<SchemaTagRequired>().is_some() {
            return Ok(SchemaRejection::TagRequired);
        }
        if let Some(SchemaNotConfig { tag }) = refusal.object() {
            return Ok(SchemaRejection::NotConfig { tag });
        }
        Err(NotOurs)
    }
}

#[cfg(test)]
#[path = "refusal.test.rs"]
mod tests;
