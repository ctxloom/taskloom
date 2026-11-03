//! The watch stream over a scripted sequence of snapshots and ticks.

use std::collections::VecDeque;

use angzarr_embedded::testing::block_on_ready as ready;
use angzarr_presentation::Format;
use plans_application::{FrontError, Meta, Plan, PlanError, Snapshot, Watched};
use serde_json::{json, Value};

use super::*;

/// Plans whose snapshots are scripted: each call takes the next, the last
/// for ever after.
struct Scripted {
    snapshots: VecDeque<Result<Snapshot, PlanError>>,
    taken: usize,
}

impl Scripted {
    fn of(snapshots: Vec<Snapshot>) -> Scripted {
        Scripted {
            snapshots: snapshots.into_iter().map(Ok).collect(),
            taken: 0,
        }
    }
}

macro_rules! unused {
    ($($name:ident($($arg:ty),*) -> $ret:ty;)*) => {
        $(async fn $name(&mut self, $(_: $arg),*) -> Result<$ret, PlanError> { unreachable!() })*
    };
}

impl Plans for Scripted {
    unused! {
        list(&plans_application::PlanQuery) -> Vec<Plan>;
        show(&str) -> plans_application::PlanText;
        add(plans_application::NewPlan) -> Plan;
        set(&str, plans_application::SetPlan) -> Plan;
        tag(&str, &str, bool) -> Plan;
        link(&str, plans_application::Link, bool) -> Plan;
        check() -> plans_application::CheckReport;
    }

    fn stamp(&mut self, _: &str, _: &str) -> Result<plans_application::Stamp, PlanError> {
        unreachable!()
    }

    fn snapshot(&mut self) -> Result<Snapshot, PlanError> {
        self.taken += 1;
        match self.snapshots.len() {
            0 => unreachable!("scripted"),
            1 => self.snapshots[0].clone(),
            _ => self.snapshots.pop_front().expect("one"),
        }
    }
}

/// Ticks `n` times, then never again; stop resolves when they are used up.
struct Ticking(usize);

impl Ticks for Ticking {
    async fn tick(&mut self) {
        match self.0 {
            0 => std::future::pending().await,
            _ => self.0 -= 1,
        }
    }
}

fn watched(status: &str, body: &str) -> Snapshot {
    let mut snapshot = Snapshot::new();
    snapshot.insert(
        "/tree/p.plan.md".to_string(),
        Watched {
            id: Some("quiet-amber-falcon".into()),
            meta: Meta {
                id: Some("quiet-amber-falcon".into()),
                status: Some(status.to_string()),
                ..Meta::default()
            },
            body: body.to_string(),
            invalid: None,
        },
    );
    snapshot
}

fn lines(script: Vec<Snapshot>, ticks: usize) -> (u8, Vec<Value>, String) {
    let settings = Settings {
        format: Format::Json,
        session: String::new(),
        prog: "taskloom",
    };
    let catalog = Catalog::en_us(&[crate::EN_US, plans_application::EN_US]).unwrap();
    let mut plans = Scripted::of(script);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut ticks = Ticking(ticks);
    let code = ready(super::run(
        &mut plans,
        &settings,
        &mut ticks,
        std::future::ready(()),
        &catalog,
        &mut out,
        &mut err,
    ));
    let lines = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (code, lines, String::from_utf8(err).unwrap())
}

#[test]
fn the_first_line_is_the_priming_one_and_a_stop_ends_with_exit_0() {
    // The stop is ready at once, so no tick is looked at.
    let (code, lines, err) = lines(vec![watched("draft", "# P\n")], 0);
    assert_eq!(code, 0, "{err}");
    assert_eq!(lines, [json!({"event": "changed", "kind": "plans"})]);
}

#[test]
fn a_priming_line_alone_when_the_snapshot_cannot_be_taken_is_the_error_and_exit_1() {
    let settings = Settings {
        format: Format::Text,
        session: String::new(),
        prog: "taskloom",
    };
    let catalog = Catalog::en_us(&[crate::EN_US, plans_application::EN_US]).unwrap();
    let mut plans = Scripted {
        snapshots: VecDeque::from([Err(PlanError::NoChange)]),
        taken: 0,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = ready(super::run(
        &mut plans,
        &settings,
        &mut Ticking(1),
        std::future::pending::<()>(),
        &catalog,
        &mut out,
        &mut err,
    ));
    assert_eq!(code, 1);
    assert!(
        out.is_empty(),
        "no priming line before the watch is established"
    );
    assert!(String::from_utf8(err)
        .unwrap()
        .starts_with("taskloom: cannot watch the plan files"),);
}

/// Run the watch with `ticks` ticks over `script`, stopping after the
/// last tick.
fn after_ticks(script: Vec<Snapshot>, ticks: usize) -> (u8, Vec<Value>) {
    struct Counted<'a> {
        left: &'a std::cell::Cell<usize>,
    }
    impl Ticks for Counted<'_> {
        async fn tick(&mut self) {
            self.left.set(self.left.get().saturating_sub(1));
        }
    }
    let left = std::cell::Cell::new(ticks);
    let settings = Settings {
        format: Format::Json,
        session: String::new(),
        prog: "taskloom",
    };
    let catalog = Catalog::en_us(&[crate::EN_US, plans_application::EN_US]).unwrap();
    let mut plans = Scripted::of(script);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let stop = std::future::poll_fn(|_| match left.get() {
        0 => std::task::Poll::Ready(()),
        _ => std::task::Poll::Pending,
    });
    let code = ready(super::run(
        &mut plans,
        &settings,
        &mut Counted { left: &left },
        stop,
        &catalog,
        &mut out,
        &mut err,
    ));
    let lines = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (code, lines)
}

#[test]
fn a_change_is_one_line_once_the_files_stand_still_for_a_tick() {
    let a = watched("draft", "# P\n");
    let b = watched("active", "# P\n");
    // Priming reads a; the first tick sees b (moving); the second sees b
    // again (still): the line.
    let (code, lines) = after_ticks(vec![a.clone(), b.clone(), b.clone()], 3);
    assert_eq!(code, 0);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(
        lines[1],
        json!({
            "event": "changed",
            "kind": "plans",
            "id": "quiet-amber-falcon",
            "path": "/tree/p.plan.md",
            "changes": {"status": {"from": "draft", "to": "active"}},
        })
    );
}

#[test]
fn files_still_moving_say_nothing_and_a_change_undone_before_it_settles_says_nothing() {
    let a = watched("draft", "# P\n");
    let b = watched("active", "# P\n");
    let c = watched("done", "# P\n");
    // a, b, c, a, a: always moving until it is back to a.
    let (_, lines) = after_ticks(vec![a.clone(), b, c, a.clone(), a], 5);
    assert_eq!(lines.len(), 1, "only the priming line: {lines:?}");
}

#[test]
fn created_and_deleted_files_and_a_body_edit_are_lines_of_their_own() {
    let a = watched("draft", "# P\n");
    let body = watched("draft", "# Other\n");
    let mut both = a.clone();
    both.insert("/tree/q.plan.md".to_string(), a["/tree/p.plan.md"].clone());
    let (_, lines) = after_ticks(
        vec![
            Snapshot::new(),
            both.clone(),
            both,
            body.clone(),
            body,
            Snapshot::new(),
            Snapshot::new(),
        ],
        7,
    );
    let events: Vec<(&str, &str)> = lines[1..]
        .iter()
        .map(|l| (l["event"].as_str().unwrap(), l["path"].as_str().unwrap()))
        .collect();
    assert_eq!(
        events,
        [
            ("created", "/tree/p.plan.md"),
            ("created", "/tree/q.plan.md"),
            ("changed", "/tree/p.plan.md"),
            ("deleted", "/tree/q.plan.md"),
            ("deleted", "/tree/p.plan.md"),
        ]
    );
    assert_eq!(lines[3]["changes"], json!({"body": true}));
    assert_eq!(lines[4]["changes"], json!({}));
    assert!(
        lines.iter().all(|l| !l.to_string().contains("Other")),
        "a line carried the body: {lines:?}"
    );
}

#[test]
fn a_file_whose_frontmatter_cannot_be_read_says_why() {
    let mut broken = watched("draft", "# P\n");
    broken.get_mut("/tree/p.plan.md").unwrap().invalid = Some(FrontError::Unclosed);
    let (_, lines) = after_ticks(vec![Snapshot::new(), broken.clone(), broken], 3);
    assert_eq!(lines[1]["event"], "created");
    assert!(
        lines[1]["invalid"].as_str().unwrap().contains("closing"),
        "{}",
        lines[1]
    );
}
