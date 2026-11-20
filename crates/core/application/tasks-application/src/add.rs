//! Adding a task ([`Adding`]): the one write that mints, binds and
//! creates. It reads as validate, judge, bind, judge again, add:
//!
//! 1. what the add carries alone is held to the domain's rules
//!    ([`Admitted`]: no value given blank, its tags admitted, then
//!    `tasks::check`), before anything is bound or created for it;
//! 2. the store's rules as read now judge its tags, so an add they refuse
//!    binds no project and creates no store;
//! 3. the project is bound, its store opened and its schema stream seeded,
//!    and the rules judge the add again there: they are the store's now;
//! 4. it is added on the first harp no task holds: a request's own harps,
//!    in order, after looking for its own add on each of them (a retry is
//!    answered with the task it added); without a request id, harps
//!    minted fresh. A harp another task holds is passed, never merged into.
//!
//! It is answered with the task its commit left, never read after it.

use angzarr_embedded::v1::{CommandBook, Uuid};
use angzarr_embedded::{CommandBookExt, ReadStore, Registry, Store};
use kernel_runner::{Refusal, Runner};
use ports::{Clock, CommandIds, Configuration, Delay, Jitter, Names, ProjectIdentity, TaskStores};
use tasks::{AddTask, TaskAggregate, TaskCommand, TaskRejection};

use crate::request::given;
use crate::rules::{judged, StoreRules};
use crate::task::{caused, task_after, task_cover, task_now};
use crate::write::Writes;
use crate::{
    failed, refused, rejected, request_command_id, tags, view, AppError, Tag, Task, Written,
};

/// How many task ids an add mints before giving up, each refused because a
/// task already holds it.
pub(crate) const HARP_ATTEMPTS: u32 = 100;

/// How many positions of its key's order ([`Names::task_id_for`]) an add
/// under a request id probes, from position 0, before refusing
/// ([`AppError::AddRequestIdCollides`]): a position another request's task
/// holds is passed and a free one is added on, but only once every one of
/// the positions has been checked for this request's own add, which is
/// answered DUPLICATE wherever it is: a position passed may since be free.
///
/// Compatibility promise: the key's harps are fixed by the seed (FNV-1a 64
/// of the key, in harp-names), harp's seeded index (scheme v1), and this
/// probe order (positions 0 to `KEYED_PROBES - 1`, in turn). A retry finds
/// the task it added only if all three are what they were when it was
/// added; raising the bound is safe, lowering it or reordering is not.
pub const KEYED_PROBES: u32 = 8;

/// A task to add. A value given (`Some`) is never read as one left out:
/// given blank, it is refused ([`AppError::Blank`]).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct NewTask {
    /// An explicit project id, overriding resolution.
    pub project: Option<String>,
    pub text: String,
    /// `None` means To Do.
    pub status: Option<String>,
    /// Given, the task is added Deferred, returning to To Do when the
    /// trigger is cleared. Required when the status is Deferred; refused
    /// with any other.
    pub trigger: Option<String>,
    pub tags: Vec<String>,
    /// The session adding it, if one is.
    pub origin_session: Option<String>,
    /// The request's key, as [`crate::TagChange::request_id`]'s. An add's
    /// stream is its harp's and the kernel deduplicates within a stream, so
    /// the key pins the harp too ([`Names::task_id_for`]): the add looks
    /// for its own add on each of the key's [`KEYED_PROBES`] harps and
    /// answers with the task there if one holds it (DUPLICATE); else it is
    /// added on the first harp no task holds. Every one held by another
    /// request's task, it is refused ([`AppError::AddRequestIdCollides`]).
    pub request_id: Option<String>,
}

/// The add use case over the ports it needs: its project and the kernel
/// ([`Writes`]), and the harp mint ([`Names`]). Borrowed from the service
/// for one call.
pub(crate) struct Adding<'s, I, S, N, G, C, D> {
    pub writes: Writes<'s, I, S, G, C, D>,
    pub names: &'s mut N,
}

/// What an add carries, held to every rule it can be held to alone: its
/// fields, given or left out but never given blank; its tags admitted;
/// the command each attempt sends but for its harp, held to the domain's
/// rules (`tasks::check`).
struct Admitted {
    project: Option<String>,
    request_id: Option<String>,
    tags: Vec<Tag>,
    template: AddTask,
}

impl Admitted {
    fn of(task: NewTask) -> Result<Admitted, AppError> {
        let request_id = given(task.request_id, "request_id")?;
        let status = given(task.status, "status")?;
        let trigger = given(task.trigger, "trigger")?;
        let origin_session = given(task.origin_session, "origin_session")?;
        let tags = tags::admit_all(&task.tags).map_err(|r| refused(&r))?;
        let template = AddTask {
            harp_id: String::new(),
            text: task.text,
            status,
            trigger,
            tags: Vec::new(),
            origin_session,
            defers: view::DEFERS.to_string(),
            initial: view::INITIAL.to_string(),
        };
        tasks::check(&template.clone().into()).map_err(|r| rejected(&r))?;
        Ok(Admitted {
            project: task.project,
            request_id,
            tags,
            template,
        })
    }

    /// The tags `rules` leave the add holding (a scalar target's last
    /// value), judged: refused on a finding the add would introduce, else
    /// the findings it carries in, to warn of.
    fn judged(&self, rules: &StoreRules) -> Result<(Vec<Tag>, Vec<crate::RuleFinding>), AppError> {
        let (tags, _) = tags::collapse(rules, &[], self.tags.clone());
        let warnings = judged(rules.validate(&[], &tags, &[]))?;
        Ok((tags, warnings))
    }
}

impl<I, S, N, G, C, D> Adding<'_, I, S, N, G, C, D>
where
    I: ProjectIdentity,
    S: TaskStores,
    N: Names,
    G: Configuration,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    pub(crate) async fn add(&mut self, task: NewTask) -> Result<Written, AppError> {
        let admitted = Admitted::of(task)?;
        let pinned = admitted.project.as_deref();
        // The store's rules as read before anything is bound: a refused
        // add binds no project and creates no store.
        let before = self.writes.projects.rules_for(pinned).await?;
        admitted.judged(&before)?;
        let project = self
            .writes
            .projects
            .identity
            .for_write(pinned)
            .map_err(AppError::Identity)?;
        let store = self
            .writes
            .projects
            .stores
            .open(&project)
            .await
            .map_err(failed)?;
        // The store's rules now it is open and seeded: they judge the add.
        let rules = crate::schema::seed_in(&mut self.writes.runner, store).await?;
        let (tags, warnings) = admitted.judged(&rules)?;
        let template = AddTask {
            tags,
            ..admitted.template
        };
        let probes = match admitted.request_id {
            Some(key) => {
                let keyed = Keyed::of(key, self.names);
                // A keyed add's harps, each checked for this request's own
                // add before any is added on: a harp the add probed past may
                // since have been freed (its task removed and purged), and a
                // retry adding there would add the task again.
                let registry = self.writes.runner.registry;
                if let Some(task) = keyed.own_add(store, registry).await? {
                    return Ok(self.writes.written(project, task, &rules, warnings));
                }
                Some(keyed)
            }
            None => None,
        };
        let task = add_on_free(
            &mut self.writes.runner,
            self.names,
            store,
            &template,
            probes,
        )
        .await?;
        Ok(self.writes.written(project, task, &rules, warnings))
    }
}

/// A keyed add: its request's key, the harps the key's order names, and
/// the one command id every probe sends.
struct Keyed {
    request_id: String,
    harps: Vec<String>,
    command_id: Uuid,
}

impl Keyed {
    /// The add under the request id `key`: the first [`KEYED_PROBES`] harps
    /// of its order, under the command id it derives.
    fn of(key: String, names: &mut impl Names) -> Keyed {
        let harps = (0..KEYED_PROBES)
            .map(|position| names.task_id_for(&key, u64::from(position)))
            .collect();
        Keyed {
            command_id: request_command_id("add", &key),
            request_id: key,
            harps,
        }
    }

    /// The task on the first of its harps whose stream holds the add its
    /// command id sent (answered DUPLICATE wherever it landed); `None` when
    /// none does.
    async fn own_add<S: Store + ReadStore>(
        &self,
        store: &S,
        registry: &Registry,
    ) -> Result<Option<Task>, AppError> {
        for harp_id in &self.harps {
            if caused(store, harp_id, &self.command_id).await? {
                let task = task_now(store, registry, harp_id).await?;
                return task.map(Some).ok_or_else(|| AppError::Vanished {
                    harp_id: harp_id.clone(),
                });
            }
        }
        Ok(None)
    }
}

/// `template` added on the first harp no task holds: of a keyed add's
/// harps, in order, under its one command id (`keyed`, the key with its
/// harps and id); unkeyed, of up to [`HARP_ATTEMPTS`] harps `names` mints,
/// each under a fresh id. The task as the add left it, from its commit. A
/// harp another task holds is refused ([`TaskRejection::TaskExists`]; a
/// keyed add's own was looked for first, and would be answered
/// DUPLICATE) and passed, never merged into its stream.
async fn add_on_free<S, N, C, D>(
    runner: &mut Runner<'_, C, D>,
    names: &mut N,
    store: &S,
    template: &AddTask,
    keyed: Option<Keyed>,
) -> Result<Task, AppError>
where
    S: Store,
    N: Names,
    C: Clock + CommandIds,
    D: Delay + Jitter + Send,
{
    let attempts = match keyed {
        Some(_) => KEYED_PROBES,
        None => HARP_ATTEMPTS,
    };
    for position in 0..attempts {
        let (harp_id, command_id) = match &keyed {
            Some(keyed) => (
                keyed.harps[position as usize].clone(),
                keyed.command_id.clone(),
            ),
            None => (names.task_id(), runner.clock.command_id()),
        };
        let add: TaskCommand = AddTask {
            harp_id: harp_id.clone(),
            ..template.clone()
        }
        .into();
        let book = CommandBook::command_of(task_cover(&harp_id), &add);
        match runner
            .run::<TaskAggregate, TaskRejection, _>(store, book, &command_id)
            .await?
        {
            Ok(handled) => return task_after(store, runner.registry, &harp_id, &handled).await,
            Err(Refusal {
                rejection: Some(TaskRejection::TaskExists { .. }),
                ..
            }) => continue,
            Err(refusal) => return Err(refusal.into()),
        }
    }
    Err(match keyed {
        Some(keyed) => AppError::AddRequestIdCollides {
            request_id: keyed.request_id,
            probes: KEYED_PROBES,
        },
        None => AppError::HarpsExhausted {
            attempts: HARP_ATTEMPTS,
        },
    })
}
