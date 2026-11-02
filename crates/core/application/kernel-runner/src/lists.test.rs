//! A project's list caught up from its store: each project's own, from
//! its streams' starts on its first read and only what was written since
//! on each later one, and a failure (the store's, or a stream's the list
//! fails) the read's. The list is a recorder of the books it is handed;
//! the streams are the task aggregate's, any marked-up domain's would do.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::testing::block_on_ready as ready;
use angzarr_embedded::v1::{CommandBook, Cover, EventBook, Projection, Uuid};
use angzarr_embedded::{
    CatchUp, CommandBookExt, CoverExt, Kernel, ProjectorHandler, Registry, Sampled,
};
use ports::Project;
use tasks::{AddTask, EditText, TaskAggregate, TaskCommand, TASK_DOMAIN};

use super::ProjectLists;

/// What a book handed to the list held: its stream's harp, its first
/// page's sequence and how many pages.
type Book = (String, u64, usize);

/// A list that records each book it is handed, failing the first
/// `failing` of them.
#[derive(Default)]
struct Recorder {
    books: Mutex<Vec<Book>>,
    failing: Mutex<u32>,
}

impl Recorder {
    /// The books handed, in the order they were.
    fn books(&self) -> Vec<Book> {
        self.books
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl ProjectorHandler for Recorder {
    fn name(&self) -> &str {
        "recorder"
    }

    async fn dispatch(&self, events: &EventBook) -> Result<Projection, CodedError> {
        {
            let mut failing = self.failing.lock().unwrap_or_else(PoisonError::into_inner);
            if *failing > 0 {
                *failing -= 1;
                return Err(CodedError::bug("recorder-failed", []));
            }
        }
        let harp = events
            .cover
            .as_ref()
            .and_then(|cover| cover.root.as_ref())
            .map(|root| String::from_utf8_lossy(&root.value).into_owned())
            .unwrap_or_default();
        let first = events.pages.first().map_or(0, |page| page.sequence);
        self.books
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((harp, first, events.pages.len()));
        Ok(Projection {
            cover: events.cover.clone(),
            projector: self.name().to_string(),
            next_sequence: events.next_sequence,
            projection: None,
        })
    }
}

/// A recorder's catch-up from nothing, over the task streams.
fn recording() -> CatchUp<Recorder> {
    CatchUp::new(&[TASK_DOMAIN], Recorder::default()).unwrap()
}

/// [`recording`], its list failing the first book it is handed.
fn failing_once() -> CatchUp<Recorder> {
    let recorder = Recorder {
        failing: Mutex::new(1),
        ..Recorder::default()
    };
    CatchUp::new(&[TASK_DOMAIN], recorder).unwrap()
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry
        .aggregate::<TaskAggregate>(TaskAggregate::default)
        .unwrap();
    registry
}

fn project(id: &str) -> Project {
    Project {
        id: id.to_string(),
        root: None,
        store: PathBuf::from(format!("/home/.ctxloom/projects/{id}/state.db")),
    }
}

/// Run `command` on the task `harp`'s stream in `store`, as `id`.
fn commit(store: &MemoryStore, harp: &str, command: TaskCommand, id: u8) {
    let sampled = Sampled {
        now: angzarr_embedded::Timestamp {
            seconds: 1_700_000_000 + i64::from(id),
            nanos: 0,
        },
        command_id: Uuid::from_bytes([id; 16]),
    };
    let book = CommandBook::command_of(Cover::natural(TASK_DOMAIN, harp), &command);
    ready(Kernel::new(&registry()).handle::<TaskAggregate, _>(store, book, &sampled))
        .unwrap_or_else(|failure| panic!("{failure:?}"));
}

fn add(store: &MemoryStore, harp: &str, id: u8) {
    let add = AddTask {
        harp_id: harp.to_string(),
        text: format!("task {harp}"),
        status: None,
        trigger: None,
        tags: Vec::new(),
        origin_session: None,
        defers: tasks::DEFERRED.to_string(),
        initial: tasks::TO_DO.to_string(),
    };
    commit(store, harp, add.into(), id);
}

fn edit(store: &MemoryStore, harp: &str, id: u8) {
    let edit = EditText {
        text: format!("task {harp}, edited"),
    };
    commit(store, harp, edit.into(), id);
}

fn sorted(mut books: Vec<Book>) -> Vec<Book> {
    books.sort();
    books
}

fn book(harp: &str, first: u64, pages: usize) -> Book {
    (harp.to_string(), first, pages)
}

#[test]
fn test_a_projects_first_read_folds_every_stream_and_a_later_one_what_was_written_since() {
    let (store, registry) = (MemoryStore::default(), registry());
    add(&store, "swift-amber", 1);
    add(&store, "brisk-otter", 2);
    let mut lists = ProjectLists::new(recording);
    let jolly = project("jolly-mute-magma");

    let list = ready(lists.caught_up(&jolly, &store, &registry)).unwrap();
    assert_eq!(
        sorted(list.books()),
        [book("brisk-otter", 0, 1), book("swift-amber", 0, 1)],
        "every stream from its start"
    );

    edit(&store, "swift-amber", 3);
    let list = ready(lists.caught_up(&jolly, &store, &registry)).unwrap();
    assert_eq!(
        list.books()[2..],
        [book("swift-amber", 1, 1)],
        "the same list, handed only what was written since"
    );
    let list = ready(lists.caught_up(&jolly, &store, &registry)).unwrap();
    assert_eq!(
        list.books().len(),
        3,
        "nothing written since: nothing handed"
    );
}

#[test]
fn test_each_project_has_a_list_of_its_own() {
    let (store, registry) = (MemoryStore::default(), registry());
    add(&store, "swift-amber", 1);
    let mut lists = ProjectLists::new(recording);
    ready(lists.caught_up(&project("jolly-mute-magma"), &store, &registry)).unwrap();

    let other = ready(lists.caught_up(&project("calm-wide-river"), &store, &registry)).unwrap();
    assert_eq!(
        other.books(),
        [book("swift-amber", 0, 1)],
        "another project's first read starts its own list"
    );
}

#[test]
fn test_a_stream_the_list_fails_fails_the_read_and_is_handed_again() {
    let (store, registry) = (MemoryStore::default(), registry());
    add(&store, "swift-amber", 1);
    let mut lists = ProjectLists::new(failing_once);
    let jolly = project("jolly-mute-magma");

    let error = match ready(lists.caught_up(&jolly, &store, &registry)) {
        Ok(_) => panic!("the list failed the stream"),
        Err(error) => error,
    };
    assert!(
        format!("{error:?}").contains("recorder-failed"),
        "the list's failure: {error:?}"
    );
    let list = ready(lists.caught_up(&jolly, &store, &registry)).unwrap();
    assert_eq!(
        list.books(),
        [book("swift-amber", 0, 1)],
        "the stream failed is handed again, from where it was"
    );
}

// A catch-up that cannot run at all (here a registry that knows no task
// domain, so no page of it can be upcast) fails the read: nothing handed.
#[test]
fn test_a_catch_up_that_cannot_run_fails_the_read() {
    let store = MemoryStore::default();
    add(&store, "swift-amber", 1);
    let mut lists = ProjectLists::new(recording);
    let jolly = project("jolly-mute-magma");

    let read = ready(lists.caught_up(&jolly, &store, &Registry::new()));
    assert!(read.is_err(), "the catch-up's failure is the read's");
    let list = ready(lists.caught_up(&jolly, &store, &registry())).unwrap();
    assert_eq!(
        list.books(),
        [book("swift-amber", 0, 1)],
        "nothing was handed before"
    );
}
