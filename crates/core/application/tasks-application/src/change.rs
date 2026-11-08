//! Changing a task that exists ([`Changing`]): its status (deferring it on
//! a trigger, and reviving it, are status changes: one `SetStatus`), its
//! text, or its tags. Each is held to the domain's rules by what it carries
//! alone, then sent to the task's stream in its project, found as a read
//! finds it (never minted or bound: a change to a task where there is none
//! is refused), its store's schema stream seeded first; and it is answered
//! with the task its commit left. A retag is the one shaped by the store's
//! rules: a scalar target's new value displaces the one the task holds,
//! and tagma judges the change against the task as the kernel loads it.

use angzarr_embedded::v1::Cover;
use ports::{Clock, CommandIds, Configuration, Delay, Jitter, ProjectIdentity, TaskStores};
use tasks::{EditText, Retag, SetStatus, TaskAggregate, TaskCommand, TaskRejection};

use crate::request::given;
use crate::rules::{judged, StoreRules};
use crate::task::{task_after, task_cover};
use crate::write::Writes;
use crate::{failed, refused, rejected, tags, view, AppError, Project, Written};

/// Tags to add to and remove from a task: the adds first, then the
/// removes, so a tag named in both ends up removed. Each is a tag in
/// tagma's grammar; one that does not parse refuses the change, as does an
/// added one the tag schema refuses. An added value of a scalar target
/// displaces the value the task holds.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TagChange {
    pub project: Option<String>,
    pub harp_id: String,
    pub add: Vec<String>,
    pub remove: Vec<String>,
    /// The request's key: the id the caller gave the request, and gives
    /// again when it retries it (no edge mints one). The command id derives
    /// from it ([`crate::request_command_id`]), so a retry is answered
    /// DUPLICATE and applies nothing, answering with the task as it stands.
    /// `None`, a fresh command id. Given blank, refused ([`AppError::Blank`]).
    pub request_id: Option<String>,
}

/// A task's status, its trigger, or both, changed. A trigger given defers
/// the task, keeping the status it held to return to; given with a
/// status, the status must be Deferred. `clear_trigger` clears the one
/// held, returning a Deferred task to that status (or to `status`, given).
/// A trigger left out (`None`) keeps the one held while the task stays
/// Deferred; moving to any other status clears it. A status or trigger
/// given blank is refused ([`AppError::Blank`]), and a trigger given with
/// `clear_trigger` ([`AppError::TriggerAndClear`]).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct StatusChange {
    pub project: Option<String>,
    pub harp_id: String,
    pub status: Option<String>,
    pub trigger: Option<String>,
    pub clear_trigger: bool,
    /// The request's key, as [`TagChange::request_id`]'s.
    pub request_id: Option<String>,
}

/// A task's text replaced whole; its status and trigger untouched.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TextEdit {
    pub project: Option<String>,
    pub harp_id: String,
    pub text: String,
    /// The request's key, as [`TagChange::request_id`]'s.
    pub request_id: Option<String>,
}

/// The changes to an existing task over the ports they need: its project
/// and the kernel ([`Writes`]). Borrowed from the service for one call.
pub(crate) struct Changing<'s, I, S, G, C, D> {
    pub writes: Writes<'s, I, S, G, C, D>,
}

impl<I, S, G, C, D> Changing<'_, I, S, G, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    G: Configuration,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    /// A status change; deferring a task (a trigger given) and reviving it
    /// (its trigger cleared) among them.
    pub(crate) async fn set_status(&mut self, change: StatusChange) -> Result<Written, AppError> {
        let request_id = given(change.request_id, "request_id")?;
        let status = given(change.status, "status")?;
        let trigger = given(change.trigger, "trigger")?;
        // SetStatus's trigger given empty is its clearing, which no edge
        // spells so: a trigger given blank is refused above.
        let trigger = match (trigger, change.clear_trigger) {
            (Some(_), true) => return Err(AppError::TriggerAndClear),
            (None, true) => Some(String::new()),
            (trigger, false) => trigger,
        };
        let set: TaskCommand = SetStatus {
            status,
            trigger,
            defers: view::DEFERS.to_string(),
            initial: view::INITIAL.to_string(),
        }
        .into();
        self.sent(
            change.project,
            &change.harp_id,
            set,
            "set_status",
            request_id,
        )
        .await
    }

    /// The text replaced whole.
    pub(crate) async fn edit(&mut self, edit: TextEdit) -> Result<Written, AppError> {
        let request_id = given(edit.request_id, "request_id")?;
        let command: TaskCommand = EditText { text: edit.text }.into();
        self.sent(edit.project, &edit.harp_id, command, "edit", request_id)
            .await
    }

    /// `command`, for `operation`, held to the domain's rules by what it
    /// carries alone, then sent to the task `harp_id` in the project
    /// `pinned` (or the work root) names, its store seeded first.
    async fn sent(
        &mut self,
        pinned: Option<String>,
        harp_id: &str,
        command: TaskCommand,
        operation: &str,
        request_id: Option<String>,
    ) -> Result<Written, AppError> {
        tasks::check(&command).map_err(|r| rejected(&r))?;
        let (project, rules) = self.found(pinned.as_deref()).await?;
        let command_id = self.writes.command_id(operation, request_id.as_deref());
        let task = self
            .writes
            .commit(&project, harp_id, command, command_id)
            .await?;
        Ok(self.writes.written(project, task, &rules, Vec::new()))
    }

    /// The project holding the task to change, and its store's rules once
    /// its schema stream is seeded; refused first where a configuration
    /// layer still sets a key that moved into the store.
    async fn found(&mut self, pinned: Option<&str>) -> Result<(Project, StoreRules), AppError> {
        self.writes.projects.refuse_moved()?;
        let project = self.writes.projects.existing(pinned).await?;
        let rules = self.writes.seeded(&project).await?;
        Ok((project, rules))
    }

    /// Tags added and removed, as the store's rules shape and judge them.
    pub(crate) async fn retag(&mut self, change: TagChange) -> Result<Written, AppError> {
        let request_id = given(change.request_id, "request_id")?;
        // Nothing to add or remove is refused before anything is read.
        let add = tags::parse_all(&change.add).map_err(|r| refused(&r))?;
        let remove = tags::parse_all(&change.remove).map_err(|r| refused(&r))?;
        let retag: TaskCommand = Retag {
            add: add.clone(),
            remove: remove.clone(),
        }
        .into();
        tasks::check(&retag).map_err(|r| rejected(&r))?;
        for tag in &add {
            tags::admit(tag).map_err(|r| refused(&r))?;
        }
        let (project, rules) = self.found(change.project.as_deref()).await?;
        // Judged against the task as the kernel loads it for the retag,
        // and sent at that head: a write landing between is stale, and the
        // kernel loads it again, and this collapses and judges again. A
        // scalar target's new value displaces the one the task holds,
        // resolved here, so decide never reads the schema.
        let shape = |task: &TaskAggregate| {
            let task = task.state();
            let (add, displaced) = tags::collapse(&rules, &task.tags, add.clone());
            let mut remove = remove.clone();
            remove.extend(displaced);
            match judged(rules.validate(&task.tags, &add, &remove)) {
                Ok(warnings) => (Some(TaskCommand::from(Retag { add, remove })), Ok(warnings)),
                Err(refused) => (None, Err(refused)),
            }
        };
        let command_id = self.writes.command_id("tag", request_id.as_deref());
        let store = self
            .writes
            .projects
            .stores
            .open(&project)
            .await
            .map_err(failed)?;
        let cover: Cover = task_cover(&change.harp_id);
        let (handled, verdict) = self
            .writes
            .runner
            .shaped::<TaskAggregate, TaskRejection, _, _, _>(store, &cover, &command_id, shape)
            .await??;
        let warnings = verdict?;
        let handled = handled.expect("a retag judged sound is sent");
        let task = task_after(
            store,
            self.writes.runner.registry,
            &change.harp_id,
            &handled,
        )
        .await?;
        Ok(self.writes.written(project, task, &rules, warnings))
    }
}
