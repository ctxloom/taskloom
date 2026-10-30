//! The task's commands and events as angzarr-embedded's markup takes them:
//! each generated domain type is a [`DomainMessage`], travelling as its
//! prost message in tasks-proto.
//!
//! HAND-WRITTEN over the annals domain layer this crate generates, until
//! the workspace leaves annals (step 3 of
//! taskloom-on-angzarr-embedded.plan.md). angzarr-build's `markup_domain`
//! (angzarr-cli dbfb538) generates tasks.proto's markup layer, and it
//! compiles against the pinned angzarr-embedded, but not beside annals'
//! layer: the two are the same types under the same names, written to the
//! same file in `OUT_DIR`, and the rules in `lib.rs` are written over
//! annals'. Once annals' layer goes, the generated layer replaces both it
//! and this module.

use angzarr_embedded::next::markup::DomainMessage;
use angzarr_embedded::next::WireError;

use crate::{
    AddTask, EditText, RemoveTask, Retag, Retagged, SetStatus, StatusChanged, TaskAdded,
    TaskRemoved, TextEdited,
};

/// annals' wire error as angzarr-embedded's: the same two cases.
fn wire_error(error: annals::next::WireError) -> WireError {
    match error {
        annals::next::WireError::Missing { message, field } => {
            WireError::Missing { message, field }
        }
        annals::next::WireError::UnknownEnum {
            message,
            field,
            value,
        } => WireError::UnknownEnum {
            message,
            field,
            value,
        },
    }
}

macro_rules! domain_messages {
    ($($name:ident),* $(,)?) => {$(
        impl DomainMessage for $name {
            type Wire = tasks_proto::v1::$name;

            fn to_wire(&self) -> tasks_proto::v1::$name {
                self.clone().into()
            }

            fn from_wire(wire: tasks_proto::v1::$name) -> Result<Self, WireError> {
                $name::try_from(wire).map_err(wire_error)
            }
        }
    )*};
}

domain_messages!(
    AddTask,
    SetStatus,
    EditText,
    Retag,
    RemoveTask,
    TaskAdded,
    StatusChanged,
    TextEdited,
    Retagged,
    TaskRemoved,
);
