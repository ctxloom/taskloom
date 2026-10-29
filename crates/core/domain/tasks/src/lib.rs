//! The tasks domain. One task is one aggregate: its stream's root is derived
//! from its harp id, so a harp id is unique because a stream is, and a
//! removed task's stream keeps its harp reserved; a prune removes a task
//! and then purges its stream, which frees the harp.
//!
//! The domain types, their wire conversions, the [`TaskCommand`] and
//! [`TaskEvent`] payloads and [`TaskState`]'s annals impl are generated from
//! `proto/ctxloom/tasks/v1/tasks.proto`; this crate holds the rules.

mod generated {
    include!(concat!(env!("OUT_DIR"), "/ctxloom.tasks.v1.domain.rs"));
}
mod aggregate;
mod markup;
mod rejection;

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fmt;

use annals::next::v1::Uuid;
use annals::next::{ids, Domain, IngestCtx, Tx};

pub use aggregate::TaskAggregate;
pub use generated::*;
pub use rejection::{diagnostic_samples, TaskRejection, EN_US};

/// The default taxonomy's initial status: the one a task gets when none is
/// given. Decide never reads it: a command carries its own (`initial`).
pub const TO_DO: &str = "To Do";
/// The default taxonomy's deferring status, whose tasks wait on a revive
/// trigger. Decide never reads it: a command carries its own (`defers`).
pub const DEFERRED: &str = "Deferred";
/// The prefix reserved for status classes (`@open`); no status starts with it.
pub const STATUS_CLASS_SIGIL: char = '@';

/// The tasks aggregate.
pub struct Tasks;

impl Domain for Tasks {
    type State = TaskState;
    type Command = TaskCommand;
    type Fact = Infallible;
    type Event = TaskEvent;
    type Rejection = TaskRejection;
    type Rules = ();

    const NAME: &'static str = TASK_DOMAIN;

    fn decide(command: &TaskCommand, tx: &mut Tx<'_, Self>) -> Result<(), TaskRejection> {
        match command {
            TaskCommand::AddTask(add) => decide_add(add, tx),
            TaskCommand::SetStatus(set) => {
                let held = live(tx.state())?;
                let Some(after) = moved(held, &changed(set)?)? else {
                    return Ok(());
                };
                let at = tx.recorded_at();
                let deferred_since = match after.deferred {
                    true => Some(at),
                    false => held.deferred_since,
                };
                tx.emit(
                    StatusChanged {
                        status: after.status,
                        trigger: after.trigger,
                        at,
                        deferred_since,
                        prior_status: after.prior_status,
                    }
                    .into(),
                );
                Ok(())
            }
            TaskCommand::EditText(edit) => {
                live(tx.state())?;
                let text = required_text(&edit.text)?;
                tx.emit(TextEdited { text }.into());
                Ok(())
            }
            TaskCommand::Retag(retag) => {
                let held = normalized(&live(tx.state())?.tags);
                retagging(retag)?;
                let mut after = held.clone();
                after.extend(normalized(&retag.add));
                for spelled in normalized(&retag.remove).keys() {
                    after.remove(spelled);
                }
                if after != held {
                    let tags = after.into_values().collect();
                    tx.emit(Retagged { tags }.into());
                }
                Ok(())
            }
            TaskCommand::RemoveTask(remove) => {
                live(tx.state())?;
                tx.emit(
                    TaskRemoved {
                        reason: remove.reason,
                    }
                    .into(),
                );
                Ok(())
            }
        }
    }

    // Equivalent: no fact exists (Infallible), so ingest is never called.
    #[cfg_attr(test, mutants::skip)]
    fn ingest(_: &TaskState, fact: &Infallible, _: &IngestCtx<'_, Self>) -> Vec<TaskEvent> {
        match *fact {}
    }

    fn apply(state: &mut TaskState, event: &TaskEvent, (): &()) {
        match event {
            TaskEvent::TaskAdded(added) => {
                state.harp_id = added.harp_id.clone();
                state.text = added.text.clone();
                state.status = added.status.clone();
                state.trigger = added.trigger.clone();
                state.tags = added.tags.clone();
                state.origin_session = added.origin_session.clone();
                state.created_at = added.at;
                state.status_set_at = added.at;
                state.deferred_since = added.deferred_since;
                state.prior_status = added.prior_status.clone();
            }
            TaskEvent::StatusChanged(changed) => {
                state.status = changed.status.clone();
                state.trigger = changed.trigger.clone();
                state.status_set_at = changed.at;
                state.deferred_since = changed.deferred_since;
                state.prior_status = changed.prior_status.clone();
            }
            TaskEvent::TextEdited(edited) => state.text = edited.text.clone(),
            TaskEvent::Retagged(retagged) => state.tags = retagged.tags.clone(),
            TaskEvent::TaskRemoved(_) => state.removed = true,
        }
    }
}

fn decide_add(add: &AddTask, tx: &mut Tx<'_, Tasks>) -> Result<(), TaskRejection> {
    let held = tx.state();
    if !held.harp_id.is_empty() {
        return Err(TaskRejection::TaskExists {
            harp_id: held.harp_id.clone(),
        });
    }
    let harp_id = add.harp_id.trim();
    if harp_id.is_empty() {
        return Err(TaskRejection::HarpIdRequired);
    }
    belongs(TASK_DOMAIN, harp_id, tx.stream().root.as_ref())?;
    let Added {
        text,
        status,
        trigger,
        origin_session,
        prior_status,
        deferred,
    } = added(add)?;
    let at = tx.recorded_at();
    let deferred_since = deferred.then_some(at);
    tx.emit(
        TaskAdded {
            harp_id: harp_id.to_string(),
            text,
            status,
            trigger,
            tags: normalized(&add.tags).into_values().collect(),
            origin_session,
            at,
            deferred_since,
            prior_status,
        }
        .into(),
    );
    Ok(())
}

/// Whether the task `harp_id` of `domain` belongs in the stream rooted at
/// `root`: the stream is addressed by the root its harp id derives, so the
/// harp id the event records must be that one, or the task takes
/// another's. A root that cannot be derived is refused for that, with its
/// cause.
fn belongs(domain: &str, harp_id: &str, root: Option<&Uuid>) -> Result<(), TaskRejection> {
    let derived = ids::compute_root(domain, harp_id).map_err(|e| TaskRejection::NoRoot {
        harp_id: harp_id.to_string(),
        cause: e.to_string(),
    })?;
    if root != Some(&derived) {
        return Err(TaskRejection::NotItsStream {
            harp_id: harp_id.to_string(),
        });
    }
    Ok(())
}

/// Whether `command` keeps the rules decide holds it to by what it carries
/// alone, whatever its stream holds: an add's text, status, trigger, origin
/// session and statuses (not its harp id, which is its stream's address),
/// a status change's status, trigger and statuses, an edit's text, and a
/// retag's having a tag. Decide holds every command to these same rules,
/// through the same functions; a caller runs them first so that a command
/// they refuse is refused before anything is bound, created or written for
/// it.
pub fn check(command: &TaskCommand) -> Result<(), TaskRejection> {
    match command {
        TaskCommand::AddTask(add) => added(add).map(drop),
        TaskCommand::SetStatus(set) => changed(set).map(drop),
        TaskCommand::EditText(edit) => required_text(&edit.text).map(drop),
        TaskCommand::Retag(retag) => retagging(retag),
        TaskCommand::RemoveTask(_) => Ok(()),
    }
}

/// What an add records of its own fields.
struct Added {
    text: String,
    status: String,
    trigger: String,
    origin_session: String,
    prior_status: String,
    /// Whether it is added deferred.
    deferred: bool,
}

/// An add's text trimmed; its status (`initial` when it gives none, and
/// `defers` when it gives only a trigger); its trigger and origin session
/// trimmed; and, added `defers`, `initial` to return to. Refused without
/// text, with a field given blank, without its statuses, with a reserved
/// status, `defers` with no trigger, or a trigger with another status.
fn added(add: &AddTask) -> Result<Added, TaskRejection> {
    let text = required_text(&add.text)?;
    let statuses = statuses(&add.defers, &add.initial)?;
    let status = valid_status(add.status.as_deref())?;
    let trigger = present(add.trigger.as_deref(), "trigger")?;
    let origin_session = present(add.origin_session.as_deref(), "origin_session")?;
    let status = match (status, &trigger) {
        (None, Some(_)) => statuses.defers.to_string(),
        (None, None) => statuses.initial.to_string(),
        (Some(status), _) => status,
    };
    let deferred = status == statuses.defers;
    if trigger.is_some() && !deferred {
        return Err(statuses.with_status(status));
    }
    if deferred && trigger.is_none() {
        return Err(TaskRejection::TriggerRequired);
    }
    Ok(Added {
        text,
        status,
        trigger: trigger.unwrap_or_default(),
        origin_session: origin_session.unwrap_or_default(),
        prior_status: match deferred {
            true => statuses.initial.to_string(),
            false => String::new(),
        },
        deferred,
    })
}

/// The statuses a command carries, which rule what it does: `defers`, the
/// status a trigger defers a task to, the one that needs a trigger; and
/// `initial`, a task's status when none is given.
struct Statuses<'a> {
    defers: &'a str,
    initial: &'a str,
}

impl Statuses<'_> {
    /// The refusal of a trigger given with `status`, which is not `defers`.
    fn with_status(&self, status: String) -> TaskRejection {
        TaskRejection::TriggerWithStatus {
            status,
            defers: self.defers.to_string(),
        }
    }
}

/// A command's `defers` and `initial`, trimmed: refused when either is
/// empty, since decide names no status to fall back on.
fn statuses<'a>(defers: &'a str, initial: &'a str) -> Result<Statuses<'a>, TaskRejection> {
    let resolved = |status: &'a str, field: &str| match status.trim() {
        "" => Err(TaskRejection::Unresolved {
            field: field.to_string(),
        }),
        status => Ok(status),
    };
    Ok(Statuses {
        defers: resolved(defers, "defers")?,
        initial: resolved(initial, "initial")?,
    })
}

/// What a status change asks for of the trigger.
enum Trigger {
    /// Left out: the one held is kept while the task stays deferred.
    Kept,
    /// Given empty: the one held is cleared.
    Cleared,
    /// Given: the task is deferred on it.
    Set(String),
}

/// What a status change asks for: its status (if it gives one), its
/// trigger, and its statuses.
struct Change<'a> {
    status: Option<String>,
    trigger: Trigger,
    statuses: Statuses<'a>,
}

/// What `set` asks for, checked by what it carries alone. Refused with a
/// field given blank, without its statuses, giving neither a status nor a
/// trigger, with a reserved status, a trigger with a status other than
/// `defers`, or a clearing with `defers`.
fn changed(set: &SetStatus) -> Result<Change<'_>, TaskRejection> {
    let statuses = statuses(&set.defers, &set.initial)?;
    let status = valid_status(set.status.as_deref())?;
    let trigger = match set.trigger.as_deref() {
        None => Trigger::Kept,
        Some("") => Trigger::Cleared,
        Some(given) => Trigger::Set(present(Some(given), "trigger")?.unwrap_or_default()),
    };
    match (&status, &trigger) {
        (None, Trigger::Kept) => return Err(TaskRejection::StatusRequired),
        (Some(status), Trigger::Set(_)) if status != statuses.defers => {
            return Err(statuses.with_status(status.clone()))
        }
        (Some(status), Trigger::Cleared) if status == statuses.defers => {
            return Err(TaskRejection::TriggerRequired)
        }
        _ => {}
    }
    Ok(Change {
        status,
        trigger,
        statuses,
    })
}

/// What a status change leaves the task holding.
struct Moved {
    status: String,
    trigger: String,
    prior_status: String,
    /// Whether it defers the task, now.
    deferred: bool,
}

/// What `change` makes of the task `held`: `None` when it changes nothing
/// (a trigger cleared that the task does not hold). Refused when it defers
/// the task with no trigger, given or held.
fn moved(held: &TaskState, change: &Change<'_>) -> Result<Option<Moved>, TaskRejection> {
    let defers = change.statuses.defers;
    let was_deferred = held.status == defers;
    // The status a task deferred now returns to: the one it was deferred
    // from, kept while it stays deferred.
    let to_return_to = || match was_deferred {
        true => held.prior_status.clone(),
        false => held.status.clone(),
    };
    let deferring = |trigger: String| Moved {
        status: defers.to_string(),
        trigger,
        prior_status: to_return_to(),
        deferred: true,
    };
    let after = match (&change.status, &change.trigger) {
        (_, Trigger::Set(trigger)) => deferring(trigger.clone()),
        (None, Trigger::Cleared) if held.trigger.is_empty() => return Ok(None),
        (status, Trigger::Cleared) => Moved {
            status: match status {
                Some(status) => status.clone(),
                None if was_deferred && !held.prior_status.is_empty() => held.prior_status.clone(),
                None if was_deferred => change.statuses.initial.to_string(),
                None => held.status.clone(),
            },
            trigger: String::new(),
            prior_status: String::new(),
            deferred: false,
        },
        (Some(status), Trigger::Kept) if status == defers => match held.trigger.as_str() {
            "" => return Err(TaskRejection::TriggerRequired),
            trigger => deferring(trigger.to_string()),
        },
        (Some(status), Trigger::Kept) => Moved {
            status: status.clone(),
            trigger: String::new(),
            prior_status: String::new(),
            deferred: false,
        },
        (None, Trigger::Kept) => unreachable!("a change giving nothing is refused (changed)"),
    };
    Ok(Some(after))
}

/// `value` trimmed, `None` when it is left out, refused when it is given
/// blank: a value given is never read as one left out. `field` names it.
fn present(value: Option<&str>, field: &str) -> Result<Option<String>, TaskRejection> {
    match value.map(str::trim) {
        None => Ok(None),
        Some("") => Err(TaskRejection::Blank {
            field: field.to_string(),
        }),
        Some(trimmed) => Ok(Some(trimmed.to_string())),
    }
}

/// Refused when a retag adds and removes nothing.
fn retagging(retag: &Retag) -> Result<(), TaskRejection> {
    match retag.add.is_empty() && retag.remove.is_empty() {
        true => Err(TaskRejection::TagRequired),
        false => Ok(()),
    }
}

/// The task, if the stream holds one that is not removed.
fn live(state: &TaskState) -> Result<&TaskState, TaskRejection> {
    if state.harp_id.is_empty() {
        return Err(TaskRejection::NoTask);
    }
    if state.removed {
        return Err(TaskRejection::Removed {
            harp_id: state.harp_id.clone(),
        });
    }
    Ok(state)
}

fn required_text(text: &str) -> Result<String, TaskRejection> {
    match text.trim() {
        "" => Err(TaskRejection::TextRequired),
        trimmed => Ok(trimmed.to_string()),
    }
}

/// The trimmed status, `None` when it is left out, refused when it is given
/// blank or claims the status-class sigil.
fn valid_status(status: Option<&str>) -> Result<Option<String>, TaskRejection> {
    match present(status, "status")? {
        Some(s) if s.starts_with(STATUS_CLASS_SIGIL) => {
            Err(TaskRejection::StatusReserved { status: s })
        }
        status => Ok(status),
    }
}

/// Tags deduplicated, by their canonical spelling (a tag's spelling is
/// unique to it), in the order of that spelling.
fn normalized(tags: &[Tag]) -> BTreeMap<String, Tag> {
    tags.iter().map(|t| (t.to_string(), t.clone())).collect()
}

/// A tag is tagma's: the generated domain type carries the same three
/// components as [`tagma_core::Tag`], and converts to it and back whole.
impl From<tagma_core::Tag> for Tag {
    fn from(tag: tagma_core::Tag) -> Self {
        Tag {
            namespace: tag.namespace,
            key: tag.key,
            value: tag.value,
        }
    }
}

impl From<Tag> for tagma_core::Tag {
    fn from(tag: Tag) -> Self {
        tagma_core::Tag {
            namespace: tag.namespace,
            key: tag.key,
            value: tag.value,
        }
    }
}

impl Tag {
    /// `tag`'s components as a [`tagma_core::Tag`].
    pub fn to_core(&self) -> tagma_core::Tag {
        self.clone().into()
    }

    /// The `namespace:key` (or bare `key`) the tag's facts are declared
    /// against, and a formula's placeholder names.
    pub fn target(&self) -> String {
        match &self.namespace {
            Some(namespace) => format!("{namespace}:{}", self.key),
            None => self.key.clone(),
        }
    }
}

/// The canonical spelling (tagma's `Display`): each component bare where it
/// can be, quoted where it must.
impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_core().fmt(f)
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
