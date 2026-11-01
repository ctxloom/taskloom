//! The plan's commands and events as the proto's messages: this crate's
//! types converted to and from the generated domain layer's
//! (`crate::messages`), whose enums are the payloads a book carries and a
//! read model decodes.

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

impl From<&PlanCommand> for g::PlanCommand {
    fn from(command: &PlanCommand) -> g::PlanCommand {
        command_message(command)
    }
}

impl From<g::PlanCommand> for PlanCommand {
    fn from(command: g::PlanCommand) -> PlanCommand {
        command_of(command)
    }
}

impl From<&PlanEvent> for g::PlanEvent {
    fn from(event: &PlanEvent) -> g::PlanEvent {
        event_message(event)
    }
}

impl From<g::PlanEvent> for PlanEvent {
    fn from(event: g::PlanEvent) -> PlanEvent {
        event_of(event)
    }
}
