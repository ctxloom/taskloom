//! The task's and the schema's commands and events as payloads: packed as `Any` by their wire
//! types' names, and read back.

use angzarr_embedded::next::markup::DomainMessage;
use angzarr_embedded::next::{decode_as, pack, type_name, Any, Payload, PayloadError, WireError};
use prost::{Message, Name};

use crate::{
    AddTask, Declare, EditText, RemoveTask, Retag, Retagged, SchemaCommand, SchemaEvent,
    SchemaRetagged, SetStatus, StatusChanged, TaskAdded, TaskCommand, TaskEvent, TaskRemoved,
    TextEdited,
};

/// `value` as its wire type, packed.
fn packed<D: DomainMessage>(value: &D) -> Any
where
    D::Wire: Message + Name,
{
    pack(&value.to_wire())
}

/// `any` as the domain message `D`, if it is `D`'s wire type: `None` if it
/// is another type.
fn read<D>(any: &Any) -> Option<Result<D, PayloadError>>
where
    D: DomainMessage + TryFrom<<D as DomainMessage>::Wire, Error = WireError>,
    D::Wire: Message + Name + Default,
{
    (type_name(&any.type_url) == D::Wire::full_name()).then(|| decode_as::<D::Wire, D>(any))
}

impl Payload for TaskCommand {
    fn to_any(&self) -> Any {
        match self {
            TaskCommand::AddTask(value) => packed(value),
            TaskCommand::SetStatus(value) => packed(value),
            TaskCommand::EditText(value) => packed(value),
            TaskCommand::Retag(value) => packed(value),
            TaskCommand::RemoveTask(value) => packed(value),
        }
    }

    fn from_any(any: &Any) -> Result<Self, PayloadError> {
        None.or_else(|| read::<AddTask>(any).map(|r| r.map(TaskCommand::AddTask)))
            .or_else(|| read::<SetStatus>(any).map(|r| r.map(TaskCommand::SetStatus)))
            .or_else(|| read::<EditText>(any).map(|r| r.map(TaskCommand::EditText)))
            .or_else(|| read::<Retag>(any).map(|r| r.map(TaskCommand::Retag)))
            .or_else(|| read::<RemoveTask>(any).map(|r| r.map(TaskCommand::RemoveTask)))
            .unwrap_or_else(|| {
                Err(PayloadError::UnknownType {
                    type_url: any.type_url.clone(),
                })
            })
    }

    fn type_urls() -> Vec<String> {
        vec![
            tasks_proto::v1::AddTask::type_url(),
            tasks_proto::v1::SetStatus::type_url(),
            tasks_proto::v1::EditText::type_url(),
            tasks_proto::v1::Retag::type_url(),
            tasks_proto::v1::RemoveTask::type_url(),
        ]
    }
}

impl Payload for TaskEvent {
    fn to_any(&self) -> Any {
        match self {
            TaskEvent::TaskAdded(value) => packed(value),
            TaskEvent::StatusChanged(value) => packed(value),
            TaskEvent::TextEdited(value) => packed(value),
            TaskEvent::Retagged(value) => packed(value),
            TaskEvent::TaskRemoved(value) => packed(value),
        }
    }

    fn from_any(any: &Any) -> Result<Self, PayloadError> {
        None.or_else(|| read::<TaskAdded>(any).map(|r| r.map(TaskEvent::TaskAdded)))
            .or_else(|| read::<StatusChanged>(any).map(|r| r.map(TaskEvent::StatusChanged)))
            .or_else(|| read::<TextEdited>(any).map(|r| r.map(TaskEvent::TextEdited)))
            .or_else(|| read::<Retagged>(any).map(|r| r.map(TaskEvent::Retagged)))
            .or_else(|| read::<TaskRemoved>(any).map(|r| r.map(TaskEvent::TaskRemoved)))
            .unwrap_or_else(|| {
                Err(PayloadError::UnknownType {
                    type_url: any.type_url.clone(),
                })
            })
    }

    fn type_urls() -> Vec<String> {
        vec![
            tasks_proto::v1::TaskAdded::type_url(),
            tasks_proto::v1::StatusChanged::type_url(),
            tasks_proto::v1::TextEdited::type_url(),
            tasks_proto::v1::Retagged::type_url(),
            tasks_proto::v1::TaskRemoved::type_url(),
        ]
    }
}

impl Payload for SchemaCommand {
    fn to_any(&self) -> Any {
        match self {
            SchemaCommand::Declare(value) => packed(value),
        }
    }

    fn from_any(any: &Any) -> Result<Self, PayloadError> {
        read::<Declare>(any)
            .map(|r| r.map(SchemaCommand::Declare))
            .unwrap_or_else(|| {
                Err(PayloadError::UnknownType {
                    type_url: any.type_url.clone(),
                })
            })
    }

    fn type_urls() -> Vec<String> {
        vec![tasks_proto::v1::Declare::type_url()]
    }
}

impl Payload for SchemaEvent {
    fn to_any(&self) -> Any {
        match self {
            SchemaEvent::SchemaRetagged(value) => packed(value),
        }
    }

    fn from_any(any: &Any) -> Result<Self, PayloadError> {
        read::<SchemaRetagged>(any)
            .map(|r| r.map(SchemaEvent::SchemaRetagged))
            .unwrap_or_else(|| {
                Err(PayloadError::UnknownType {
                    type_url: any.type_url.clone(),
                })
            })
    }

    fn type_urls() -> Vec<String> {
        vec![tasks_proto::v1::SchemaRetagged::type_url()]
    }
}
