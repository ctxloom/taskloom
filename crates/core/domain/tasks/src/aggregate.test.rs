//! A task on angzarr-embedded, driven through its kernel exactly as the
//! binary drives it: `Kernel::handle` over angzarr-ports' in-memory
//! store, which checks, identifies, deduplicates against the store, hands
//! the command to the marked-up [`TaskAggregate`] through angzarr-router,
//! and commits. The task's rules, and the runtime's own: the instant is the context's `recorded_at`, and a
//! refusal is business-fatal, a key and its args, never English.
//!
//! Not ported from `lib.test.rs`: the snapshot tests and the four "before
//! L" tests, which fabricated an earlier history through a snapshot (they
//! and the rules for that history are gone): the aggregate takes no
//! snapshot, and a store starts fresh on angzarr-embedded (the owner's
//! ruling D2), so no stream holds a history today's rules could not make. The pure tests (`check`, `belongs`, a tag's target,
//! the catalog) stay where they are.

use std::collections::BTreeMap;
use std::future::Future;

use angzarr_embedded::next::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::next::angzarr_router::error::RouterReason;
use angzarr_embedded::next::v1::{command_page, Command, CommandBook, CommandPage, Cover, Uuid};
use angzarr_embedded::next::{
    ids, pack, Any, Domain, EventStore, Failure, Handled, Kernel, KernelError, MemoryDeadLetters,
    OnRetryable, Payload, Recovery, Registry, Rejected, Sampled, Stream, StreamRef, Timestamp,
};
use angzarr_embedded_diagnostic::{Arg, Diagnostic};

use crate::{
    check, AddTask, EditText, RemovalReason, RemoveTask, Retag, Retagged, SetStatus, StatusChanged,
    Tag, TaskAdded, TaskAggregate, TaskCommand, TaskEvent, TaskRejection, TaskRemoved, TaskState,
    TextEdited, DEFERRED, TASK_DOMAIN, TO_DO,
};

const HARP: &str = "swift-amber";

/// What a diagnostic says: its key and its args, each as text, by name
/// (decision 14.37: a key and a map, never English). Args by name, as the
/// router carries them.
type Said = (&'static str, BTreeMap<&'static str, String>);

fn said(diagnostic: &dyn Diagnostic) -> Said {
    let args = diagnostic
        .args()
        .into_iter()
        .map(|(name, arg)| {
            let text = match arg {
                Arg::Int(n) => n.to_string(),
                Arg::Uint(n) => n.to_string(),
                Arg::Str(s) => s,
                Arg::Nested(cause) => cause.code().to_string(),
            };
            (name, text)
        })
        .collect();
    (diagnostic.code(), args)
}

/// `rejection` as a command's refusal: what [`Task::run`] answers.
fn refused(rejection: TaskRejection) -> Result<Vec<TaskEvent>, Said> {
    Err(said(&rejection))
}

/// Poll `future` once: the in-memory store never waits.
fn ready<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let waker = std::task::Waker::noop();
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(waker))
    {
        std::task::Poll::Ready(out) => out,
        std::task::Poll::Pending => panic!("the in-memory store never waits"),
    }
}

/// One project's store holding one task's stream, and the clock its
/// commands are sampled at.
struct Task {
    store: MemoryStore,
    registry: Registry,
    seconds: i64,
}

impl Task {
    fn new() -> Task {
        let mut registry = Registry::new();
        registry.aggregate::<TaskAggregate>(TaskAggregate::default);
        Task {
            store: MemoryStore::new(),
            registry,
            seconds: 100,
        }
    }

    /// A task added with `text` at second 101.
    fn added(text: &str) -> Task {
        let mut task = Task::new();
        task.run(add(text)).unwrap();
        task
    }

    /// Run `command` one second after the last: the events it committed,
    /// or its refusal.
    fn run(&mut self, command: impl Into<TaskCommand>) -> Result<Vec<TaskEvent>, Said> {
        match self.try_run(book(command.into().to_any())) {
            Ok(Handled::Applied(recorded)) => Ok(events(recorded.books())),
            Ok(Handled::Duplicate(_)) => panic!("a fresh command id is never a duplicate"),
            Ok(Handled::Parked) => panic!("only a fact is parked, and these are commands"),
            Err(failure) => match *failure {
                Failure::Rejected(rejected) => Err(said(&rejected)),
                other => panic!("not a rejection: {other:?}"),
            },
        }
    }

    /// Handle `book` through the kernel, one second after the last command.
    fn try_run(&mut self, book: CommandBook) -> Result<Handled, Box<Failure<Rejected>>> {
        self.seconds += 1;
        self.try_run_at(book, at(self.seconds))
    }

    /// Handle `book` through the kernel, sampled at `now`. A retryable
    /// failure is not retried: these tests wait on nothing.
    fn try_run_at(
        &mut self,
        book: CommandBook,
        now: Timestamp,
    ) -> Result<Handled, Box<Failure<Rejected>>> {
        let sampled = Sampled {
            now,
            command_id: Uuid::from_bytes([self.seconds as u8; 16]),
        };
        let dead_letters = MemoryDeadLetters::new();
        ready(
            Kernel::new(&self.registry, &dead_letters)
                .on_retryable(OnRetryable {
                    max_retries: 0,
                    ..OnRetryable::default()
                })
                .handle::<TaskAggregate, _, _>(&self.store, book, &sampled, &mut ()),
        )
        .map_err(Box::new)
    }

    /// `command`, refused: the refusal as the caller receives it. Nothing
    /// was recorded for it.
    fn refusal(&mut self, command: impl Into<TaskCommand>) -> Rejected {
        let before = self.pages().len();
        let refusal = match self.try_run(book(command.into().to_any())) {
            Err(failure) => match *failure {
                Failure::Rejected(rejected) => rejected,
                other => panic!("refused, not {other:?}"),
            },
            Ok(_) => panic!("refused, not applied"),
        };
        assert_eq!(self.pages().len(), before, "a refusal records nothing");
        refusal
    }

    /// The task's stored pages, oldest first.
    fn pages(&self) -> Vec<angzarr_embedded::next::v1::EventPage> {
        let cover = cover();
        let stream = StreamRef::try_from(&cover).unwrap();
        ready(self.store.read(stream, 0, None)).unwrap().pages
    }

    /// The task, replayed from the store as the next process would.
    fn state(&self) -> TaskState {
        Stream::<TaskAggregate>::replay(cover(), &self.pages(), &self.registry)
            .unwrap()
            .state()
            .state()
            .clone()
    }
}

/// The task events `books` hold, in order.
fn events(books: &[angzarr_embedded::next::v1::EventBook]) -> Vec<TaskEvent> {
    books
        .iter()
        .flat_map(|book| &book.pages)
        .map(|page| TaskEvent::from_any(page.payload.as_ref().unwrap()).unwrap())
        .collect()
}

/// The task's stream root, derived from its harp id.
fn root() -> Uuid {
    ids::compute_root(TASK_DOMAIN, HARP).unwrap()
}

fn cover() -> Cover {
    Cover {
        domain: TASK_DOMAIN.to_string(),
        root: Some(root()),
        edition: None,
    }
}

fn at(seconds: i64) -> Timestamp {
    Timestamp { seconds, nanos: 0 }
}

/// A client's command to the task's stream, with no ids of its own.
fn book(payload: Any) -> CommandBook {
    CommandBook {
        cover: Some(cover()),
        page: Some(CommandPage {
            body: Some(command_page::Body::Command(Command {
                payload: Some(payload),
            })),
            ..CommandPage::default()
        }),
        handling: None,
    }
}

/// `book`, carrying the client's own command id: what a retry resends.
fn with_id(mut book: CommandBook, id: u8) -> CommandBook {
    if let Some(page) = book.page.as_mut() {
        page.command_id = Some(Uuid::from_bytes([id; 16]));
    }
    book
}

fn add(text: &str) -> AddTask {
    AddTask {
        harp_id: HARP.to_string(),
        text: text.to_string(),
        status: None,
        trigger: None,
        tags: Vec::new(),
        origin_session: Some("calm-otter".to_string()),
        defers: DEFERRED.to_string(),
        initial: TO_DO.to_string(),
    }
}

/// A status change: each of `status` and `trigger` left out when "".
fn status(status: &str, trigger: &str) -> SetStatus {
    let given = |s: &str| (!s.is_empty()).then(|| s.to_string());
    SetStatus {
        status: given(status),
        trigger: given(trigger),
        defers: DEFERRED.to_string(),
        initial: TO_DO.to_string(),
    }
}

/// A status change giving exactly `status` and `trigger`, "" included.
fn set(status: Option<&str>, trigger: Option<&str>) -> SetStatus {
    SetStatus {
        status: status.map(str::to_string),
        trigger: trigger.map(str::to_string),
        defers: DEFERRED.to_string(),
        initial: TO_DO.to_string(),
    }
}

fn retag(add: &[&str], remove: &[&str]) -> Retag {
    Retag {
        add: tags(add),
        remove: tags(remove),
    }
}

/// Each of `spelled`, parsed as tagma parses a tag.
fn tags(spelled: &[&str]) -> Vec<Tag> {
    spelled
        .iter()
        .map(|s| tagma_core::Tag::parse(s).expect("a valid tag").into())
        .collect()
}

/// The single `StatusChanged` among `events`.
fn changed(events: &[TaskEvent]) -> &StatusChanged {
    match events {
        [TaskEvent::StatusChanged(changed)] => changed,
        other => panic!("one StatusChanged, not {other:?}"),
    }
}

// ------------------------------------------------- the new runtime's rules

// The instant is the context's `recorded_at`, the one the application
// sampled and the kernel stamped: the handler reads no clock. An instant no
// clock would give (2023, to the nanosecond) is recorded exactly.
#[test]
fn an_add_on_a_fresh_stream_records_task_added_at_the_sampled_instant() {
    let sampled = Timestamp {
        seconds: 1_700_000_000,
        nanos: 123_456_789,
    };
    let mut task = Task::new();
    let handled = task
        .try_run_at(
            book(
                TaskCommand::from(AddTask {
                    trigger: Some("after the release".to_string()),
                    ..add("Write the tour")
                })
                .to_any(),
            ),
            sampled,
        )
        .unwrap_or_else(|failure| panic!("applied, not {failure:?}"));
    let Handled::Applied(recorded) = handled else {
        panic!("a fresh command is applied");
    };
    let events = events(recorded.books());
    let [TaskEvent::TaskAdded(added)] = events.as_slice() else {
        panic!("one TaskAdded, not {events:?}");
    };
    assert_eq!(added.at, sampled, "the event's instant is the sampled one");
    assert_eq!(
        added.deferred_since,
        Some(sampled),
        "added deferred at that instant"
    );
    let pages = task.pages();
    assert_eq!(
        pages[0].meta.as_ref().and_then(|meta| meta.recorded_at),
        Some(sampled),
        "recorded at it"
    );
    let state = task.state();
    assert_eq!((state.created_at, state.status_set_at), (sampled, sampled));
}

// A stream with history holds a task (or held one): its harp id is taken.
// Refused as a business "no", fatal (never retried, its sender's to
// answer by minting another harp), keyed with its harp id and never
// English; message-only, with no typed object; nothing recorded.
#[test]
fn an_add_on_a_stream_with_history_is_refused_task_exists() {
    let mut task = Task::added("First");
    let refusal = task.refusal(add("Second"));
    assert_eq!(
        said(&refusal),
        said(&TaskRejection::TaskExists {
            harp_id: HARP.to_string()
        })
    );
    assert_eq!(refusal.code(), "tasks-task-exists");
    assert_eq!(
        refusal.to_string(),
        "tasks-task-exists harp_id=swift-amber",
        "its key and args, never English"
    );
    assert_eq!(refusal.refusal_object(), None, "message-only");
    assert_eq!(
        Failure::Rejected(refusal).recovery(),
        Recovery::Business,
        "business-fatal"
    );
    assert_eq!(task.state().text, "First", "the task held is untouched");
}

// Deferred needs a trigger, given or held: by a status change, and by an
// add. Business-fatal, keyed with no args; nothing recorded.
#[test]
fn a_deferral_without_a_trigger_is_refused() {
    let mut task = Task::added("Revisit");
    let refusal = task.refusal(status(DEFERRED, ""));
    assert_eq!(said(&refusal), said(&TaskRejection::TriggerRequired));
    assert_eq!(refusal.to_string(), "tasks-trigger-required");
    assert_eq!(task.state().status, TO_DO, "not deferred");

    let refusal = Task::new().refusal(AddTask {
        status: Some(DEFERRED.to_string()),
        trigger: None,
        ..add("Revisit")
    });
    assert_eq!(said(&refusal), said(&TaskRejection::TriggerRequired));
}

// A trigger defers the task, so it cannot come with a status other than
// the deferring one: by a status change, and by an add. Keyed with the
// status given and the deferring status; nothing recorded.
#[test]
fn a_trigger_on_a_non_deferred_status_is_refused() {
    let expected = said(&TaskRejection::TriggerWithStatus {
        status: "In Progress".to_string(),
        defers: DEFERRED.to_string(),
    });
    let mut task = Task::added("Revisit");
    let refusal = task.refusal(set(Some("In Progress"), Some("after v2")));
    assert_eq!(said(&refusal), expected);
    assert_eq!(task.state().trigger, "", "no trigger taken");

    let refusal = Task::new().refusal(AddTask {
        status: Some("In Progress".to_string()),
        trigger: Some("after v2".to_string()),
        ..add("Revisit")
    });
    assert_eq!(said(&refusal), expected);
}

#[test]
fn the_aggregate_is_the_tasks_domain() {
    assert_eq!(<TaskAggregate as Domain>::NAME, TASK_DOMAIN);
}

// ------------------------------------------------------------ usage

#[test]
fn test_add_records_the_task_with_its_creation_time() {
    let mut task = Task::new();
    let events = task.run(add("Write the tour")).unwrap();
    assert_eq!(
        events,
        vec![TaskEvent::TaskAdded(TaskAdded {
            harp_id: HARP.to_string(),
            text: "Write the tour".to_string(),
            status: TO_DO.to_string(),
            trigger: String::new(),
            tags: Vec::new(),
            origin_session: "calm-otter".to_string(),
            at: at(101),
            deferred_since: None,
            prior_status: String::new(),
        })]
    );
    let state = task.state();
    assert_eq!(state.harp_id, HARP);
    assert_eq!(state.text, "Write the tour");
    assert_eq!(state.status, TO_DO);
    assert_eq!(state.origin_session, "calm-otter");
    assert_eq!(state.created_at, at(101));
    assert_eq!(state.status_set_at, at(101));
    assert_eq!(state.deferred_since, None, "never deferred");
    assert!(!state.removed);
}

#[test]
fn test_add_trims_text_and_normalizes_tags() {
    let mut task = Task::new();
    task.run(AddTask {
        tags: tags(&["urgent", "release", "urgent", "\"urgent\""]),
        ..add("  Ship it \n")
    })
    .unwrap();
    assert_eq!(task.state().text, "Ship it");
    assert_eq!(task.state().tags, tags(&["release", "urgent"]));
}

// Sorted by the spelling, so a namespace or a value sorts by its
// characters, not as a component.
#[test]
fn test_tags_are_sorted_by_their_canonical_spelling() {
    let mut task = Task::new();
    task.run(AddTask {
        tags: tags(&["b", "a=\"x y\"", "a:z", "a"]),
        ..add("Ship it")
    })
    .unwrap();
    let spelled: Vec<String> = task.state().tags.iter().map(Tag::to_string).collect();
    assert_eq!(spelled, ["a", "a:z", "a=\"x y\"", "b"]);
}

// A tag and the same tag with a value, or in a namespace, are different
// tags; removing one leaves the others.
#[test]
fn test_a_retag_removes_exactly_the_tag_it_names() {
    let mut task = Task::new();
    task.run(AddTask {
        tags: tags(&["level", "level=1", "triage:level=1"]),
        ..add("Ship it")
    })
    .unwrap();
    task.run(retag(&[], &["level=1"])).unwrap();
    assert_eq!(task.state().tags, tags(&["level", "triage:level=1"]));
}

#[test]
fn test_add_deferred_with_a_trigger_records_when_it_was_deferred() {
    let mut task = Task::new();
    task.run(AddTask {
        status: Some(DEFERRED.to_string()),
        trigger: Some(" after the release ".to_string()),
        ..add("Revisit")
    })
    .unwrap();
    assert_eq!(task.state().status, DEFERRED);
    assert_eq!(task.state().trigger, "after the release");
    assert_eq!(task.state().deferred_since, Some(at(101)));
}

#[test]
fn test_set_status_moves_the_task_and_stamps_when() {
    let mut task = Task::added("Write the tour");
    let events = task.run(status("In Progress", "")).unwrap();
    assert_eq!(
        events,
        vec![TaskEvent::StatusChanged(StatusChanged {
            status: "In Progress".to_string(),
            trigger: String::new(),
            at: at(102),
            deferred_since: None,
            prior_status: String::new(),
        })]
    );
    assert_eq!(task.state().status, "In Progress");
    assert_eq!(task.state().status_set_at, at(102));
    assert_eq!(task.state().created_at, at(101));
}

#[test]
fn test_deferring_a_deferred_task_by_name_keeps_the_trigger_it_holds() {
    let mut task = Task::added("Revisit");
    task.run(status(DEFERRED, "after the release")).unwrap();
    let events = task.run(status(DEFERRED, "")).unwrap();
    assert_eq!(
        events,
        vec![TaskEvent::StatusChanged(StatusChanged {
            status: DEFERRED.to_string(),
            trigger: "after the release".to_string(),
            at: at(103),
            deferred_since: Some(at(103)),
            prior_status: TO_DO.to_string(),
        })],
        "the event carries the trigger held, unchanged or not, and the status to return to"
    );
    assert_eq!(task.state().trigger, "after the release");
    assert_eq!(task.state().deferred_since, Some(at(103)));
}

// A task holds a trigger exactly while it is Deferred, so deferring it
// again needs a trigger again.
#[test]
fn test_deferring_again_after_leaving_deferred_needs_a_trigger_again() {
    let mut task = Task::added("Revisit");
    task.run(status(DEFERRED, "after the release")).unwrap();
    task.run(status(TO_DO, "")).unwrap();
    assert_eq!(
        task.run(status(DEFERRED, "")),
        refused(TaskRejection::TriggerRequired)
    );
}

#[test]
fn test_leaving_deferred_clears_the_trigger_and_keeps_when_it_was_deferred() {
    let mut task = Task::added("Revisit");
    task.run(status(DEFERRED, "after the release")).unwrap();
    let events = task.run(status(TO_DO, "")).unwrap();
    assert_eq!(
        events,
        vec![TaskEvent::StatusChanged(StatusChanged {
            status: TO_DO.to_string(),
            trigger: String::new(),
            at: at(103),
            deferred_since: Some(at(102)),
            prior_status: String::new(),
        })]
    );
    assert_eq!(task.state().trigger, "");
    assert_eq!(task.state().deferred_since, Some(at(102)));
}

#[test]
fn test_a_new_trigger_replaces_the_one_held() {
    let mut task = Task::added("Revisit");
    task.run(status(DEFERRED, "after the release")).unwrap();
    task.run(status(DEFERRED, " when CI is green ")).unwrap();
    assert_eq!(task.state().trigger, "when CI is green");
}

#[test]
fn test_edit_text_replaces_the_text_whole() {
    let mut task = Task::added("Write the tour");
    let events = task
        .run(EditText {
            text: " Write the full tour ".to_string(),
        })
        .unwrap();
    assert_eq!(
        events,
        vec![TaskEvent::TextEdited(TextEdited {
            text: "Write the full tour".to_string(),
        })]
    );
    assert_eq!(task.state().text, "Write the full tour");
}

#[test]
fn test_retag_records_the_tags_the_task_holds_after_it() {
    let mut task = Task::new();
    task.run(AddTask {
        tags: tags(&["later", "release"]),
        ..add("Ship it")
    })
    .unwrap();
    let events = task
        .run(retag(&["urgent", "release"], &["later", "absent"]))
        .unwrap();
    assert_eq!(
        events,
        vec![TaskEvent::Retagged(Retagged {
            tags: tags(&["release", "urgent"]),
        })]
    );
    assert_eq!(task.state().tags, tags(&["release", "urgent"]));
}

#[test]
fn test_retag_with_only_tags_to_add_records_them() {
    let mut task = Task::added("Ship it");
    assert_eq!(
        task.run(retag(&["urgent"], &[])).unwrap(),
        vec![TaskEvent::Retagged(Retagged {
            tags: tags(&["urgent"]),
        })]
    );
}

#[test]
fn test_retag_with_only_tags_to_remove_records_them() {
    let mut task = Task::new();
    task.run(AddTask {
        tags: tags(&["later"]),
        ..add("Ship it")
    })
    .unwrap();
    assert_eq!(
        task.run(retag(&[], &["later"])).unwrap(),
        vec![TaskEvent::Retagged(Retagged { tags: Vec::new() })]
    );
    assert_eq!(task.state().tags, Vec::<Tag>::new());
}

#[test]
fn test_retag_naming_a_tag_in_both_lists_ends_without_it() {
    let mut task = Task::added("Ship it");
    task.run(retag(&["urgent"], &["urgent"])).unwrap();
    assert_eq!(task.state().tags, Vec::<Tag>::new());
}

#[test]
fn test_retag_that_changes_nothing_records_nothing() {
    let mut task = Task::new();
    task.run(AddTask {
        tags: tags(&["urgent"]),
        ..add("Ship it")
    })
    .unwrap();
    assert_eq!(
        task.run(retag(&["urgent"], &["absent"])).unwrap(),
        Vec::new()
    );
}

#[test]
fn test_remove_marks_the_task_removed_and_keeps_its_harp_id() {
    let mut task = Task::added("Ship it");
    let events = task
        .run(RemoveTask {
            reason: RemovalReason::Pruned,
        })
        .unwrap();
    assert_eq!(
        events,
        vec![TaskEvent::TaskRemoved(TaskRemoved {
            reason: RemovalReason::Pruned,
        })]
    );
    assert!(task.state().removed);
    assert_eq!(task.state().harp_id, HARP);
}

// ------------------------------------------------------------ refusals

#[test]
fn test_add_to_a_stream_holding_a_task_is_refused() {
    let mut task = Task::added("First");
    assert_eq!(
        task.run(add("Second")),
        refused(TaskRejection::TaskExists {
            harp_id: HARP.to_string()
        })
    );
}

#[test]
fn test_add_to_a_removed_tasks_stream_is_refused() {
    let mut task = Task::added("First");
    task.run(RemoveTask {
        reason: RemovalReason::Removed,
    })
    .unwrap();
    assert_eq!(
        task.run(add("Second")),
        refused(TaskRejection::TaskExists {
            harp_id: HARP.to_string()
        })
    );
}

#[test]
fn test_add_without_a_harp_id_is_refused() {
    let mut task = Task::new();
    assert_eq!(
        task.run(AddTask {
            harp_id: " ".to_string(),
            ..add("Ship it")
        }),
        refused(TaskRejection::HarpIdRequired)
    );
}

#[test]
fn test_add_naming_another_harp_than_its_stream_is_refused() {
    // The stream's root is derived from the harp id; a task under another
    // harp would take this harp id and leave its own free.
    let mut task = Task::new();
    assert_eq!(
        task.run(AddTask {
            harp_id: "other-harp".to_string(),
            ..add("Ship it")
        }),
        refused(TaskRejection::NotItsStream {
            harp_id: "other-harp".to_string()
        })
    );
}

// The session an add came from is trimmed, as its text, status, trigger
// and harp id are.
#[test]
fn test_add_trims_its_origin_session() {
    let mut task = Task::new();
    let events = task
        .run(AddTask {
            origin_session: Some("  calm-otter \n".to_string()),
            ..add("Ship it")
        })
        .unwrap();
    let TaskEvent::TaskAdded(added) = &events[0] else {
        panic!("{events:?}")
    };
    assert_eq!(added.origin_session, "calm-otter");
    assert_eq!(task.state().origin_session, "calm-otter");
}

#[test]
fn test_add_without_text_is_refused() {
    assert_eq!(
        Task::new().run(add("  \n ")),
        refused(TaskRejection::TextRequired)
    );
}

#[test]
fn test_add_deferred_without_a_trigger_is_refused() {
    assert_eq!(
        Task::new().run(AddTask {
            status: Some(DEFERRED.to_string()),
            trigger: None,
            ..add("Revisit")
        }),
        refused(TaskRejection::TriggerRequired)
    );
}

#[test]
fn test_a_status_starting_with_the_class_sigil_is_refused() {
    let reserved = || {
        refused(TaskRejection::StatusReserved {
            status: "@open".to_string(),
        })
    };
    assert_eq!(
        Task::new().run(AddTask {
            status: Some("@open".to_string()),
            ..add("Ship it")
        }),
        reserved()
    );
    assert_eq!(Task::added("Ship it").run(status("@open", "")), reserved());
}

#[test]
fn test_set_status_without_a_status_is_refused() {
    assert_eq!(
        Task::added("Ship it").run(status("", "")),
        refused(TaskRejection::StatusRequired)
    );
}

#[test]
fn test_deferring_a_task_with_no_trigger_is_refused() {
    assert_eq!(
        Task::added("Ship it").run(status(DEFERRED, "")),
        refused(TaskRejection::TriggerRequired)
    );
}

#[test]
fn test_edit_text_to_nothing_is_refused() {
    assert_eq!(
        Task::added("Ship it").run(EditText {
            text: " ".to_string()
        }),
        refused(TaskRejection::TextRequired)
    );
}

#[test]
fn test_retag_without_a_tag_is_refused() {
    assert_eq!(
        Task::added("Ship it").run(retag(&[], &[])),
        refused(TaskRejection::TagRequired)
    );
}

// What the aggregate refuses of a command alone, check refuses too, before
// any stream is read; what needs the stream (a harp taken, a trigger held)
// it leaves to the aggregate.
#[test]
fn test_check_refuses_what_decide_refuses_of_a_command_alone() {
    let refused_alone: Vec<(TaskCommand, TaskRejection)> = vec![
        (add("  \n ").into(), TaskRejection::TextRequired),
        (
            AddTask {
                status: Some(DEFERRED.to_string()),
                trigger: None,
                ..add("Revisit")
            }
            .into(),
            TaskRejection::TriggerRequired,
        ),
        (
            AddTask {
                status: Some("@open".to_string()),
                ..add("Ship it")
            }
            .into(),
            TaskRejection::StatusReserved {
                status: "@open".to_string(),
            },
        ),
        (status("", "").into(), TaskRejection::StatusRequired),
        (
            status("@open", "").into(),
            TaskRejection::StatusReserved {
                status: "@open".to_string(),
            },
        ),
        (
            EditText {
                text: " ".to_string(),
            }
            .into(),
            TaskRejection::TextRequired,
        ),
        (retag(&[], &[]).into(), TaskRejection::TagRequired),
    ];
    for (command, rejection) in refused_alone {
        assert_eq!(check(&command), Err(rejection.clone()), "{command:?}");
        assert_eq!(
            Task::added("Ship it").run(command.clone()),
            refused(match command {
                TaskCommand::AddTask(_) => TaskRejection::TaskExists {
                    harp_id: HARP.to_string(),
                },
                _ => rejection.clone(),
            }),
            "the aggregate, on a task held"
        );
        if let TaskCommand::AddTask(_) = command {
            assert_eq!(
                Task::new().run(command),
                refused(rejection),
                "the aggregate, fresh"
            );
        }
    }
}

#[test]
fn test_every_command_to_a_missing_task_is_refused() {
    let commands: Vec<TaskCommand> = vec![
        status(TO_DO, "").into(),
        EditText {
            text: "x".to_string(),
        }
        .into(),
        retag(&["x"], &[]).into(),
        RemoveTask {
            reason: RemovalReason::Removed,
        }
        .into(),
    ];
    for command in commands {
        assert_eq!(Task::new().run(command), refused(TaskRejection::NoTask));
    }
}

#[test]
fn test_every_command_to_a_removed_task_is_refused() {
    let commands: Vec<TaskCommand> = vec![
        status(TO_DO, "").into(),
        EditText {
            text: "x".to_string(),
        }
        .into(),
        retag(&["x"], &[]).into(),
        RemoveTask {
            reason: RemovalReason::Removed,
        }
        .into(),
    ];
    for command in commands {
        let mut task = Task::added("Ship it");
        task.run(RemoveTask {
            reason: RemovalReason::Removed,
        })
        .unwrap();
        assert_eq!(
            task.run(command),
            refused(TaskRejection::Removed {
                harp_id: HARP.to_string()
            })
        );
    }
}

// ------------------------------------------------------------ the wire

// A payload that is no domain value is malformed input: a bug the router
// answers ANY_DECODE_FAILED, never a refusal (decision 14.37).
#[test]
fn test_a_removal_without_a_reason_is_undecodable_a_bug_not_a_refusal() {
    let mut task = Task::added("Ship it");
    let unspecified = pack(&tasks_proto::v1::RemoveTask { reason: 0 });
    let failure = match task.try_run(book(unspecified)) {
        Err(failure) => *failure,
        Ok(_) => panic!("an unspecified reason is no domain value: not applied"),
    };
    assert_eq!(failure.recovery(), Recovery::Bug);
    let Failure::Kernel(KernelError::Router(error)) = failure else {
        panic!("the router's: {failure:?}");
    };
    assert_eq!(error.code, RouterReason::AnyDecodeFailed.as_str_name());
    assert!(!task.state().removed);
}

#[test]
fn test_a_retried_command_is_answered_with_its_first_events() {
    let mut task = Task::new();
    let first = with_id(book(TaskCommand::from(add("Ship it")).to_any()), 7);
    let Ok(Handled::Applied(_)) = task.try_run(first.clone()) else {
        panic!("the first send applies");
    };
    let Ok(Handled::Duplicate(original)) = task.try_run(first) else {
        panic!("the retry is a duplicate");
    };
    assert_eq!(original.pages.len(), 1);
    assert_eq!(task.pages().len(), 1, "nothing was appended twice");
}

// ------------------------------------------- L: a trigger defers the task
//
// Fix queue L (passthrough-os-review.plan.md), with the human's rulings of
// 2026-10-27: setting a trigger MAKES the task Deferred, overriding its
// status, which is kept and restored when the trigger is cleared; leaving
// Deferred by a status clears the trigger, so a task holds one exactly
// while it is Deferred; a trigger with another status is refused. The
// wire's fields are optional, so a status change tells a trigger left out
// (keep it) from one given empty (clear it); a field given blank is
// refused.

#[test]
fn test_a_trigger_given_alone_defers_the_task_keeping_its_status_to_return_to() {
    let mut task = Task::added("Revisit");
    task.run(status("In Progress", "")).unwrap();
    let events = task.run(set(None, Some(" after v2 "))).unwrap();
    assert_eq!(
        events,
        vec![TaskEvent::StatusChanged(StatusChanged {
            status: DEFERRED.to_string(),
            trigger: "after v2".to_string(),
            at: at(103),
            deferred_since: Some(at(103)),
            prior_status: "In Progress".to_string(),
        })]
    );
    let state = task.state();
    assert_eq!(
        (state.status.as_str(), state.prior_status.as_str()),
        (DEFERRED, "In Progress")
    );
}

#[test]
fn test_clearing_the_trigger_returns_the_task_to_the_status_it_was_deferred_from() {
    let mut task = Task::added("Revisit");
    task.run(status("In Progress", "")).unwrap();
    task.run(set(None, Some("after v2"))).unwrap();
    let events = task.run(set(None, Some(""))).unwrap();
    assert_eq!(
        events,
        vec![TaskEvent::StatusChanged(StatusChanged {
            status: "In Progress".to_string(),
            trigger: String::new(),
            at: at(104),
            deferred_since: Some(at(103)),
            prior_status: String::new(),
        })],
        "the trigger and the status to return to are gone; when it was deferred is kept"
    );
    assert_eq!(task.state().status, "In Progress");
}

#[test]
fn test_deferring_by_name_keeps_the_status_to_return_to_too() {
    let mut task = Task::added("Revisit");
    task.run(status("In Progress", "")).unwrap();
    let events = task.run(status(DEFERRED, "after v2")).unwrap();
    assert_eq!(changed(&events).prior_status, "In Progress");
    task.run(set(None, Some(""))).unwrap();
    assert_eq!(task.state().status, "In Progress");
}

#[test]
fn test_a_trigger_given_with_another_status_is_refused() {
    let refusal = TaskRejection::TriggerWithStatus {
        status: "In Progress".to_string(),
        defers: DEFERRED.to_string(),
    };
    let commands: Vec<TaskCommand> = vec![
        set(Some("In Progress"), Some("after CI is green")).into(),
        set(Some(" In Progress "), Some("after CI is green")).into(),
        AddTask {
            status: Some("In Progress".to_string()),
            trigger: Some("after CI is green".to_string()),
            ..add("Revisit")
        }
        .into(),
    ];
    for command in commands {
        assert_eq!(check(&command), Err(refusal.clone()), "check {command:?}");
        let mut task = match command {
            TaskCommand::AddTask(_) => Task::new(),
            _ => Task::added("Revisit"),
        };
        assert_eq!(
            task.run(command.clone()),
            refused(refusal.clone()),
            "the aggregate {command:?}"
        );
    }
}

#[test]
fn test_a_trigger_given_with_the_deferring_status_defers_the_task() {
    let mut task = Task::added("Revisit");
    task.run(status("In Progress", "")).unwrap();
    let events = task
        .run(set(Some(DEFERRED), Some("after CI is green")))
        .unwrap();
    let changed = changed(&events);
    assert_eq!(
        (changed.status.as_str(), changed.trigger.as_str()),
        (DEFERRED, "after CI is green")
    );
    assert_eq!(changed.prior_status, "In Progress");
}

#[test]
fn test_a_new_trigger_on_a_deferred_task_keeps_the_status_to_return_to() {
    let mut task = Task::added("Revisit");
    task.run(status("In Progress", "")).unwrap();
    task.run(set(None, Some("after v2"))).unwrap();
    task.run(set(None, Some("after v3"))).unwrap();
    let state = task.state();
    assert_eq!(state.trigger, "after v3");
    assert_eq!(state.prior_status, "In Progress", "not Deferred");
}

#[test]
fn test_clearing_the_trigger_with_a_status_moves_the_task_to_that_status() {
    let mut task = Task::added("Revisit");
    task.run(set(None, Some("after v2"))).unwrap();
    let events = task.run(set(Some("Done"), Some(""))).unwrap();
    let changed = changed(&events);
    assert_eq!(
        (changed.status.as_str(), changed.trigger.as_str()),
        ("Done", "")
    );
    assert_eq!(changed.prior_status, "");
}

#[test]
fn test_leaving_deferred_by_status_drops_the_status_to_return_to() {
    let mut task = Task::added("Revisit");
    task.run(status("In Progress", "")).unwrap();
    task.run(set(None, Some("after v2"))).unwrap();
    let events = task.run(status("Done", "")).unwrap();
    let changed = changed(&events);
    assert_eq!(
        changed.trigger, "",
        "cleared: only a Deferred task holds one"
    );
    assert_eq!(changed.prior_status, "");
}

#[test]
fn test_clearing_a_trigger_the_task_does_not_hold_changes_nothing() {
    let mut task = Task::added("Revisit");
    assert_eq!(task.run(set(None, Some(""))), Ok(Vec::new()));
}

#[test]
fn test_deferring_while_clearing_the_trigger_is_refused() {
    let mut task = Task::added("Revisit");
    task.run(set(None, Some("after v2"))).unwrap();
    assert_eq!(
        task.run(set(Some(DEFERRED), Some(""))),
        refused(TaskRejection::TriggerRequired)
    );
}

#[test]
fn test_a_task_added_with_a_trigger_is_added_deferred() {
    let mut task = Task::new();
    let events = task
        .run(AddTask {
            trigger: Some("after v2".to_string()),
            ..add("Revisit")
        })
        .unwrap();
    let [TaskEvent::TaskAdded(added)] = events.as_slice() else {
        panic!("{events:?}")
    };
    assert_eq!(added.status, DEFERRED);
    assert_eq!(added.deferred_since, Some(at(101)));
    assert_eq!(added.prior_status, TO_DO, "the status it returns to");
}

#[test]
fn test_a_task_added_deferred_by_name_returns_to_to_do() {
    let mut task = Task::new();
    task.run(AddTask {
        status: Some(DEFERRED.to_string()),
        trigger: Some("after v2".to_string()),
        ..add("Revisit")
    })
    .unwrap();
    assert_eq!(task.state().prior_status, TO_DO);
}

#[test]
fn test_a_task_added_without_a_trigger_has_no_status_to_return_to() {
    let mut task = Task::added("Ship it");
    assert_eq!(task.state().prior_status, "");
    let events = task.run(status("Done", "")).unwrap();
    assert_eq!(changed(&events).prior_status, "");
}

#[test]
fn test_a_field_given_blank_is_refused_never_read_as_left_out() {
    let blank = |field: &str| TaskRejection::Blank {
        field: field.to_string(),
    };
    let commands: Vec<(TaskCommand, TaskRejection)> = vec![
        (
            AddTask {
                status: Some(" ".to_string()),
                ..add("Ship it")
            }
            .into(),
            blank("status"),
        ),
        (
            AddTask {
                trigger: Some("".to_string()),
                ..add("Ship it")
            }
            .into(),
            blank("trigger"),
        ),
        (
            AddTask {
                origin_session: Some(" \n".to_string()),
                ..add("Ship it")
            }
            .into(),
            blank("origin_session"),
        ),
        (set(Some(""), None).into(), blank("status")),
        (set(Some(" "), Some("after v2")).into(), blank("status")),
        // A trigger given blank but not empty is no clearing: refused.
        (set(None, Some("  ")).into(), blank("trigger")),
    ];
    for (command, rejection) in commands {
        assert_eq!(check(&command), Err(rejection.clone()), "check {command:?}");
        let mut task = match command {
            TaskCommand::AddTask(_) => Task::new(),
            _ => Task::added("Ship it"),
        };
        assert_eq!(
            task.run(command.clone()),
            refused(rejection),
            "the aggregate {command:?}"
        );
    }
}

#[test]
fn test_an_add_leaving_its_fields_out_is_to_do_with_no_trigger_or_session() {
    let mut task = Task::new();
    task.run(AddTask {
        origin_session: None,
        ..add("Ship it")
    })
    .unwrap();
    let state = task.state();
    assert_eq!(
        (
            state.status.as_str(),
            state.trigger.as_str(),
            state.origin_session.as_str()
        ),
        (TO_DO, "", "")
    );
}

#[test]
fn test_a_status_change_giving_neither_a_status_nor_a_trigger_is_refused() {
    assert_eq!(
        Task::added("Ship it").run(set(None, None)),
        refused(TaskRejection::StatusRequired)
    );
}

// ------------------------------------- the statuses the command carries
//
// The domain names no status: which one a trigger defers a task to, and
// which one a task starts in, are the command's, resolved by the
// application from the configured taxonomy.

/// An add under a taxonomy whose deferring status is Parked and whose
/// initial one is Backlog.
fn add_parked(text: &str) -> AddTask {
    AddTask {
        defers: "Parked".to_string(),
        initial: "Backlog".to_string(),
        ..add(text)
    }
}

/// A status change under the same taxonomy.
fn set_parked(status: Option<&str>, trigger: Option<&str>) -> SetStatus {
    SetStatus {
        defers: "Parked".to_string(),
        initial: "Backlog".to_string(),
        ..set(status, trigger)
    }
}

#[test]
fn test_an_add_takes_its_initial_and_deferring_statuses_from_the_command() {
    let mut task = Task::new();
    task.run(add_parked("Ship it")).unwrap();
    assert_eq!(task.state().status, "Backlog");
    let mut task = Task::new();
    let events = task
        .run(AddTask {
            trigger: Some("after v2".to_string()),
            ..add_parked("Revisit")
        })
        .unwrap();
    let [TaskEvent::TaskAdded(added)] = events.as_slice() else {
        panic!("{events:?}")
    };
    assert_eq!(
        (added.status.as_str(), added.prior_status.as_str()),
        ("Parked", "Backlog")
    );
    assert_eq!(added.deferred_since, Some(at(101)));
}

#[test]
fn test_a_status_change_defers_to_and_needs_a_trigger_for_the_commands_deferring_status() {
    let mut task = Task::new();
    task.run(add_parked("Revisit")).unwrap();
    assert_eq!(
        task.run(set_parked(Some("Parked"), None)),
        refused(TaskRejection::TriggerRequired)
    );
    let events = task.run(set_parked(Some(DEFERRED), None)).unwrap();
    assert_eq!(
        changed(&events).deferred_since,
        None,
        "Deferred is an ordinary status here"
    );
    let events = task.run(set_parked(None, Some("after v2"))).unwrap();
    let changed = changed(&events);
    assert_eq!(
        (changed.status.as_str(), changed.prior_status.as_str()),
        ("Parked", DEFERRED)
    );
    assert_eq!(changed.deferred_since, Some(at(104)));
    assert_eq!(
        task.run(set_parked(Some(DEFERRED), Some("after v3"))),
        refused(TaskRejection::TriggerWithStatus {
            status: DEFERRED.to_string(),
            defers: "Parked".to_string(),
        })
    );
}

#[test]
fn test_a_command_without_its_statuses_is_refused() {
    let unresolved = |field: &str| TaskRejection::Unresolved {
        field: field.to_string(),
    };
    let commands: Vec<(TaskCommand, TaskRejection)> = vec![
        (
            AddTask {
                defers: String::new(),
                ..add("Ship it")
            }
            .into(),
            unresolved("defers"),
        ),
        (
            AddTask {
                initial: " ".to_string(),
                ..add("Ship it")
            }
            .into(),
            unresolved("initial"),
        ),
        (
            SetStatus {
                defers: String::new(),
                ..status("Done", "")
            }
            .into(),
            unresolved("defers"),
        ),
        (
            SetStatus {
                initial: String::new(),
                ..status("Done", "")
            }
            .into(),
            unresolved("initial"),
        ),
    ];
    for (command, rejection) in commands {
        assert_eq!(check(&command), Err(rejection.clone()), "check {command:?}");
        let mut task = match command {
            TaskCommand::AddTask(_) => Task::new(),
            _ => Task::added("Ship it"),
        };
        assert_eq!(
            task.run(command.clone()),
            refused(rejection),
            "the aggregate {command:?}"
        );
    }
}
