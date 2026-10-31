//! The plan's commands and events as the proto's messages: this crate's
//! types converted to and from the generated domain layer, and packed as
//! `Any` by their wire types' names.

use angzarr_embedded::next::markup::DomainMessage;
use angzarr_embedded::next::{decode_as, pack, type_name, Any, Payload, PayloadError, WireError};
use prost::{Message, Name};

use crate::generated as g;
use crate::{AddPlan, PlanCommand, PlanEvent, PlanStatus};

impl From<PlanStatus> for g::PlanStatus {
    fn from(status: PlanStatus) -> g::PlanStatus {
        match status {
            PlanStatus::Draft => g::PlanStatus::Draft,
            PlanStatus::Active => g::PlanStatus::Active,
            PlanStatus::Superseded => g::PlanStatus::Superseded,
            PlanStatus::Done => g::PlanStatus::Done,
            PlanStatus::Abandoned => g::PlanStatus::Abandoned,
        }
    }
}

impl From<g::PlanStatus> for PlanStatus {
    fn from(status: g::PlanStatus) -> PlanStatus {
        match status {
            g::PlanStatus::Draft => PlanStatus::Draft,
            g::PlanStatus::Active => PlanStatus::Active,
            g::PlanStatus::Superseded => PlanStatus::Superseded,
            g::PlanStatus::Done => PlanStatus::Done,
            g::PlanStatus::Abandoned => PlanStatus::Abandoned,
        }
    }
}

impl From<g::AddPlan> for AddPlan {
    fn from(add: g::AddPlan) -> AddPlan {
        AddPlan {
            harp_id: add.harp_id,
            title: add.title,
            status: add.status.into(),
            path: add.path,
            content_hash: add.content_hash,
            origin_session: add.origin_session,
        }
    }
}

impl From<AddPlan> for g::AddPlan {
    fn from(add: AddPlan) -> g::AddPlan {
        g::AddPlan {
            harp_id: add.harp_id,
            title: add.title,
            status: add.status.into(),
            path: add.path,
            content_hash: add.content_hash,
            origin_session: add.origin_session,
        }
    }
}

impl From<g::PlanAdded> for AddPlan {
    fn from(added: g::PlanAdded) -> AddPlan {
        AddPlan {
            harp_id: added.harp_id,
            title: added.title,
            status: added.status.into(),
            path: added.path,
            content_hash: added.content_hash,
            origin_session: added.origin_session,
        }
    }
}

impl From<AddPlan> for g::PlanAdded {
    fn from(add: AddPlan) -> g::PlanAdded {
        g::PlanAdded {
            harp_id: add.harp_id,
            title: add.title,
            status: add.status.into(),
            path: add.path,
            content_hash: add.content_hash,
            origin_session: add.origin_session,
        }
    }
}

/// `value` as its wire type, packed.
fn packed<D: DomainMessage>(value: D) -> Any
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

/// The command as the proto's message.
pub(crate) fn command_message(command: &PlanCommand) -> g::PlanCommand {
    match command.clone() {
        PlanCommand::Add(add) => g::PlanCommand::AddPlan(add.into()),
        PlanCommand::SetStatus(status) => g::PlanCommand::SetPlanStatus(g::SetPlanStatus {
            status: status.into(),
        }),
        PlanCommand::Govern(task) => g::PlanCommand::Govern(g::Govern { task }),
        PlanCommand::Ungovern(task) => g::PlanCommand::Ungovern(g::Ungovern { task }),
        PlanCommand::Supersede(plan) => g::PlanCommand::Supersede(g::Supersede { plan }),
        PlanCommand::Unsupersede(plan) => g::PlanCommand::Unsupersede(g::Unsupersede { plan }),
        PlanCommand::MarkSuperseded(by) => g::PlanCommand::MarkSuperseded(g::MarkSuperseded { by }),
        PlanCommand::Reinstate => g::PlanCommand::Reinstate(g::Reinstate {}),
        PlanCommand::Rehash(content_hash) => g::PlanCommand::Rehash(g::Rehash { content_hash }),
    }
}

/// The proto's command as this crate's.
pub(crate) fn command_of(command: g::PlanCommand) -> PlanCommand {
    match command {
        g::PlanCommand::AddPlan(add) => PlanCommand::Add(add.into()),
        g::PlanCommand::SetPlanStatus(set) => PlanCommand::SetStatus(set.status.into()),
        g::PlanCommand::Govern(g::Govern { task }) => PlanCommand::Govern(task),
        g::PlanCommand::Ungovern(g::Ungovern { task }) => PlanCommand::Ungovern(task),
        g::PlanCommand::Supersede(g::Supersede { plan }) => PlanCommand::Supersede(plan),
        g::PlanCommand::Unsupersede(g::Unsupersede { plan }) => PlanCommand::Unsupersede(plan),
        g::PlanCommand::MarkSuperseded(g::MarkSuperseded { by }) => PlanCommand::MarkSuperseded(by),
        g::PlanCommand::Reinstate(g::Reinstate {}) => PlanCommand::Reinstate,
        g::PlanCommand::Rehash(g::Rehash { content_hash }) => PlanCommand::Rehash(content_hash),
    }
}

/// The event as the proto's message.
pub(crate) fn event_message(event: &PlanEvent) -> g::PlanEvent {
    match event.clone() {
        PlanEvent::PlanAdded(add) => g::PlanEvent::PlanAdded(add.into()),
        PlanEvent::StatusChanged { from, to } => {
            g::PlanEvent::PlanStatusChanged(g::PlanStatusChanged {
                from_status: from.into(),
                to_status: to.into(),
            })
        }
        PlanEvent::Governed(task) => g::PlanEvent::Governed(g::Governed { task }),
        PlanEvent::Ungoverned(task) => g::PlanEvent::Ungoverned(g::Ungoverned { task }),
        PlanEvent::Superseded(plan) => g::PlanEvent::Superseded(g::Superseded { plan }),
        PlanEvent::Unsuperseded(plan) => g::PlanEvent::Unsuperseded(g::Unsuperseded { plan }),
        PlanEvent::MarkedSuperseded { by, from } => {
            g::PlanEvent::MarkedSuperseded(g::MarkedSuperseded {
                by,
                from_status: from.into(),
            })
        }
        PlanEvent::Reinstated => g::PlanEvent::Reinstated(g::Reinstated {}),
        PlanEvent::Rehashed(content_hash) => g::PlanEvent::Rehashed(g::Rehashed { content_hash }),
    }
}

/// The proto's event as this crate's.
pub(crate) fn event_of(event: g::PlanEvent) -> PlanEvent {
    match event {
        g::PlanEvent::PlanAdded(added) => PlanEvent::PlanAdded(added.into()),
        g::PlanEvent::PlanStatusChanged(changed) => PlanEvent::StatusChanged {
            from: changed.from_status.into(),
            to: changed.to_status.into(),
        },
        g::PlanEvent::Governed(g::Governed { task }) => PlanEvent::Governed(task),
        g::PlanEvent::Ungoverned(g::Ungoverned { task }) => PlanEvent::Ungoverned(task),
        g::PlanEvent::Superseded(g::Superseded { plan }) => PlanEvent::Superseded(plan),
        g::PlanEvent::Unsuperseded(g::Unsuperseded { plan }) => PlanEvent::Unsuperseded(plan),
        g::PlanEvent::MarkedSuperseded(marked) => PlanEvent::MarkedSuperseded {
            by: marked.by,
            from: marked.from_status.into(),
        },
        g::PlanEvent::Reinstated(g::Reinstated {}) => PlanEvent::Reinstated,
        g::PlanEvent::Rehashed(g::Rehashed { content_hash }) => PlanEvent::Rehashed(content_hash),
    }
}

impl Payload for PlanCommand {
    fn to_any(&self) -> Any {
        match command_message(self) {
            g::PlanCommand::AddPlan(value) => packed(value),
            g::PlanCommand::SetPlanStatus(value) => packed(value),
            g::PlanCommand::Govern(value) => packed(value),
            g::PlanCommand::Ungovern(value) => packed(value),
            g::PlanCommand::Supersede(value) => packed(value),
            g::PlanCommand::Unsupersede(value) => packed(value),
            g::PlanCommand::MarkSuperseded(value) => packed(value),
            g::PlanCommand::Reinstate(value) => packed(value),
            g::PlanCommand::Rehash(value) => packed(value),
        }
    }

    fn from_any(any: &Any) -> Result<Self, PayloadError> {
        let command = None
            .or_else(|| read::<g::AddPlan>(any).map(|r| r.map(g::PlanCommand::AddPlan)))
            .or_else(|| read::<g::SetPlanStatus>(any).map(|r| r.map(g::PlanCommand::SetPlanStatus)))
            .or_else(|| read::<g::Govern>(any).map(|r| r.map(g::PlanCommand::Govern)))
            .or_else(|| read::<g::Ungovern>(any).map(|r| r.map(g::PlanCommand::Ungovern)))
            .or_else(|| read::<g::Supersede>(any).map(|r| r.map(g::PlanCommand::Supersede)))
            .or_else(|| read::<g::Unsupersede>(any).map(|r| r.map(g::PlanCommand::Unsupersede)))
            .or_else(|| {
                read::<g::MarkSuperseded>(any).map(|r| r.map(g::PlanCommand::MarkSuperseded))
            })
            .or_else(|| read::<g::Reinstate>(any).map(|r| r.map(g::PlanCommand::Reinstate)))
            .or_else(|| read::<g::Rehash>(any).map(|r| r.map(g::PlanCommand::Rehash)))
            .unwrap_or_else(|| {
                Err(PayloadError::UnknownType {
                    type_url: any.type_url.clone(),
                })
            })?;
        Ok(command_of(command))
    }

    fn type_urls() -> Vec<String> {
        use plans_proto::v1;
        vec![
            v1::AddPlan::type_url(),
            v1::SetPlanStatus::type_url(),
            v1::Govern::type_url(),
            v1::Ungovern::type_url(),
            v1::Supersede::type_url(),
            v1::Unsupersede::type_url(),
            v1::MarkSuperseded::type_url(),
            v1::Reinstate::type_url(),
            v1::Rehash::type_url(),
        ]
    }
}

impl Payload for PlanEvent {
    fn to_any(&self) -> Any {
        match event_message(self) {
            g::PlanEvent::PlanAdded(value) => packed(value),
            g::PlanEvent::PlanStatusChanged(value) => packed(value),
            g::PlanEvent::Governed(value) => packed(value),
            g::PlanEvent::Ungoverned(value) => packed(value),
            g::PlanEvent::Superseded(value) => packed(value),
            g::PlanEvent::Unsuperseded(value) => packed(value),
            g::PlanEvent::MarkedSuperseded(value) => packed(value),
            g::PlanEvent::Reinstated(value) => packed(value),
            g::PlanEvent::Rehashed(value) => packed(value),
        }
    }

    fn from_any(any: &Any) -> Result<Self, PayloadError> {
        let event = None
            .or_else(|| read::<g::PlanAdded>(any).map(|r| r.map(g::PlanEvent::PlanAdded)))
            .or_else(|| {
                read::<g::PlanStatusChanged>(any).map(|r| r.map(g::PlanEvent::PlanStatusChanged))
            })
            .or_else(|| read::<g::Governed>(any).map(|r| r.map(g::PlanEvent::Governed)))
            .or_else(|| read::<g::Ungoverned>(any).map(|r| r.map(g::PlanEvent::Ungoverned)))
            .or_else(|| read::<g::Superseded>(any).map(|r| r.map(g::PlanEvent::Superseded)))
            .or_else(|| read::<g::Unsuperseded>(any).map(|r| r.map(g::PlanEvent::Unsuperseded)))
            .or_else(|| {
                read::<g::MarkedSuperseded>(any).map(|r| r.map(g::PlanEvent::MarkedSuperseded))
            })
            .or_else(|| read::<g::Reinstated>(any).map(|r| r.map(g::PlanEvent::Reinstated)))
            .or_else(|| read::<g::Rehashed>(any).map(|r| r.map(g::PlanEvent::Rehashed)))
            .unwrap_or_else(|| {
                Err(PayloadError::UnknownType {
                    type_url: any.type_url.clone(),
                })
            })?;
        Ok(event_of(event))
    }

    fn type_urls() -> Vec<String> {
        use plans_proto::v1;
        vec![
            v1::PlanAdded::type_url(),
            v1::PlanStatusChanged::type_url(),
            v1::Governed::type_url(),
            v1::Ungoverned::type_url(),
            v1::Superseded::type_url(),
            v1::Unsuperseded::type_url(),
            v1::MarkedSuperseded::type_url(),
            v1::Reinstated::type_url(),
            v1::Rehashed::type_url(),
        ]
    }
}
