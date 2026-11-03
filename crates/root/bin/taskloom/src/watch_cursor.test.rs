//! Acceptance: `taskloom tasks watch` with a resume cursor (consolidation
//! slice D), the real binary in a temporary home. Red-first: these pin the
//! owner's rulings in `cli-consolidation.plan.md` ("Machine and VS Code
//! interop", the `watch` bullet), which REVERSE D7 ("nothing outside the
//! process reads a cursor"):
//!
//! - `--since <cursor>` resumes. The cursor is PER STREAM: an opaque string
//!   encoding a map of stream (domain, edition, root) to next sequence.
//! - Each event line carries its (stream, sequence), the changed task's new
//!   state and a version field. There is a heartbeat.
//! - An unknown project fails with exit 1 (it ran against state: not found);
//!   today it emits the priming line and runs on.
//! - There is NO global order across streams (embedded dropped the global
//!   position on purpose): lines are ordered per stream only, so no test
//!   here asserts the order of lines of two streams. A cursor on a line
//!   covers every line before it on the wire.
//! - `--format json` is the machine format and JSONL; text mode is still
//!   one line per event.
//!
//! Exit codes: 2 = invalid on its own content (checked before any state is
//! read); 1 = ran against state and the answer was negative; 0 = success.
//!
//! The cursor's ENCODING is a PROPOSAL (pending owner review): base64url of
//! JSON `{"v":1,"streams":{"<domain>/<root>[@edition]":<next_seq>}}`. These
//! tests treat a cursor as an opaque string and pass it back verbatim. The
//! single exception is `test_a_cursor_of_an_unknown_version_is_exit_2`,
//! which must forge a version it cannot get from the binary: it decodes the
//! cursor as base64url JSON, sets `v`, and encodes it again, so it is the
//! one test to change if the owner chooses another encoding.
//!
//! LINE FIELD NAMES are a PROPOSAL (pending owner review): `v` (the version
//! of the line and cursor format, 1), `event` (`changed`, `heartbeat`,
//! `expired`), `project`, `stream`, `sequence`, `harp_id`, `task` (the new
//! state: `text`, `status`, `tags`) and `cursor` (the cursor once this line
//! is processed). What is NOT pinned: the shape of `stream` (only that it is
//! present and compares equal for one stream and unequal for two), the name
//! of `stream`. Orchestrator decisions, pending owner review, are marked at
//! their tests: the `primed` line, the project in the cursor, tasks streams
//! only, `--heartbeat 0`, heartbeats in text mode, the re-minted stream.
//!
//! Every test waits on lines (each wait bounded) or on a bounded process,
//! never on a sleep, so a watch that runs on instead of failing, or never
//! answers, fails the test rather than hanging it.

use std::collections::BTreeSet;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::watch::Watching;
use super::{binary, one_document, Env, Isolated};

const SESSION: &str = "watch-cursor-harp";

/// How long a command that must end on its own may take.
const ENDS_WITHIN: Duration = Duration::from_secs(20);

// ------------------------------------------------------------ helpers

/// A project whose store exists and admits every tag, with one task in it
/// that no test watches (so the store has a stream before any watch).
fn world() -> Env {
    let env = Env::new(SESSION);
    env.permit_every_tag();
    env.seed("Seed");
    env
}

fn ok(env: &Env, args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.extend(["--format", "json"]);
    let ran = env.run(&args);
    assert!(ran.ok, "{args:?}: {}{}", ran.stdout, ran.stderr);
    ran.json()
}

fn add(env: &Env, text: &str) -> String {
    ok(env, &["tasks", "add", text])["harp_id"]
        .as_str()
        .expect("a harp id")
        .to_string()
}

fn set_text(env: &Env, harp: &str, text: &str) {
    ok(env, &["tasks", "set", harp, "--text", text]);
}

fn project_id(env: &Env) -> String {
    ok(env, &["manage", "project", "show"])["project_id"]
        .as_str()
        .expect("the project")
        .to_string()
}

/// A watch of `env`'s project, JSON lines.
fn watch(env: &Env, args: &[&str]) -> Watching {
    let mut all = args.to_vec();
    all.extend(["--format", "json"]);
    Watching::start(env, &all)
}

/// The watch's priming line: it comes first, whatever the event is named.
/// Checks what the owner's rulings give it: the version, the project and a
/// cursor (the store's position now, or the resumed one).
fn primed(w: &Watching, project: &str) -> Value {
    let line = one_document(&w.line());
    assert_eq!(line["v"], 1, "a priming line carries the version: {line}");
    // Orchestrator decision, pending owner review: the priming line is
    // `{"v":1,"event":"primed","kind":"tasks","project":...,"cursor":...}`.
    assert_eq!(line["event"], "primed", "{line}");
    assert_eq!(line["kind"], "tasks", "{line}");
    assert_eq!(line["project"], project, "{line}");
    assert!(!cursor_of(&line).is_empty(), "a priming cursor: {line}");
    line
}

fn cursor_of(line: &Value) -> String {
    line["cursor"]
        .as_str()
        .unwrap_or_else(|| panic!("a `cursor` string: {line}"))
        .to_string()
}

/// The next line that is not a heartbeat.
fn next(w: &Watching) -> Value {
    loop {
        let line = one_document(&w.line());
        if line["event"] != "heartbeat" {
            return line;
        }
    }
}

/// The next `n` lines that are not heartbeats.
fn take(w: &Watching, n: usize) -> Vec<Value> {
    (0..n).map(|_| next(w)).collect()
}

/// A line's stream, as the binary spells it (opaque here).
fn stream(line: &Value) -> Value {
    let stream = &line["stream"];
    assert!(!stream.is_null(), "a line names its stream: {line}");
    stream.clone()
}

fn sequence(line: &Value) -> u64 {
    line["sequence"]
        .as_u64()
        .unwrap_or_else(|| panic!("a `sequence` number: {line}"))
}

/// The (stream, sequence) of each line: what a delivery is, order aside.
fn positions(lines: &[Value]) -> BTreeSet<(String, u64)> {
    lines
        .iter()
        .map(|l| (stream(l).to_string(), sequence(l)))
        .collect()
}

/// The sequences of `lines` on one stream, in arrival order.
fn sequences_on(lines: &[Value], on: &Value) -> Vec<u64> {
    lines
        .iter()
        .filter(|l| &l["stream"] == on)
        .map(sequence)
        .collect()
}

/// A line must be a change of this task.
fn assert_change(line: &Value, harp: &str, text: &str, status: &str, tags: Value) {
    assert_eq!(line["v"], 1, "{line}");
    assert_eq!(line["event"], "changed", "{line}");
    assert_eq!(line["harp_id"], harp, "{line}");
    assert_eq!(line["task"]["text"], text, "{line}");
    assert_eq!(line["task"]["status"], status, "{line}");
    assert_eq!(line["task"]["tags"], tags, "{line}");
}

/// What a finished (or killed) bounded process left.
struct Ended {
    /// `None`: it was still running when the time was up, and was killed.
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// `taskloom args` in `env`'s project, which must end by itself within
/// [`ENDS_WITHIN`]: a command that fails fast does, and one that wrongly
/// runs on is killed and reported as having no code.
fn ends(env: &Env, args: &[&str]) -> Ended {
    let mut child = Command::new(binary())
        .args(args)
        .isolated(env.home.path())
        .working_in(env.project.path())
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("taskloom starts");
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out.read_to_string(&mut text);
        text
    });
    let err = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = err.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + ENDS_WITHIN;
    let code = loop {
        if let Some(status) = child.try_wait().expect("status") {
            break Some(status.code().unwrap_or(-1));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    Ended {
        code,
        stdout: out.join().unwrap(),
        stderr: err.join().unwrap(),
    }
}

/// `tasks watch args` which must end with `code` and write nothing to
/// stdout (no priming line, no event).
fn refused(env: &Env, args: &[&str], code: i32) -> Ended {
    let mut all = vec!["tasks", "watch"];
    all.extend(args);
    all.extend(["--format", "json"]);
    let ended = ends(env, &all);
    assert_eq!(
        ended.code,
        Some(code),
        "{all:?} (None: it ran on): {}{}",
        ended.stdout,
        ended.stderr
    );
    assert_eq!(ended.stdout, "", "{all:?} wrote before refusing");
    assert_ne!(ended.stderr, "", "{all:?} said why on stderr");
    ended
}

/// A cursor taken from a watch's priming line, for tests that need one.
fn some_cursor(env: &Env) -> String {
    let project = project_id(env);
    let w = watch(env, &[]);
    let cursor = cursor_of(&primed(&w, &project));
    drop(w);
    cursor
}

const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn b64url_decode(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let (mut bits, mut have) = (0u32, 0u32);
    for c in text.trim_end_matches('=').bytes() {
        let v = URL_SAFE
            .iter()
            .position(|&b| b == c)
            .unwrap_or_else(|| panic!("{text:?} is not base64url")) as u32;
        bits = (bits << 6) | v;
        have += 6;
        if have >= 8 {
            have -= 8;
            bytes.push((bits >> have) as u8);
            bits &= (1 << have) - 1;
        }
    }
    bytes
}

fn b64url_encode(bytes: &[u8]) -> String {
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            text.push(URL_SAFE[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    text
}

// ------------------------------------------------------------ 1. unknown project

#[test]
fn test_an_unknown_project_exits_1_with_no_priming_line() {
    let env = world();
    // Today: the priming line, then it runs on. Ran against state, not
    // found: exit 1, and stdout stays empty.
    refused(&env, &["--project", "no-such-project"], 1);
}

#[test]
fn test_an_unknown_project_is_exit_1_with_a_cursor_too() {
    let env = world();
    let cursor = some_cursor(&env);
    refused(
        &env,
        &["--project", "no-such-project", "--since", &cursor],
        1,
    );
}

#[test]
fn test_no_project_here_is_exit_1_not_a_priming_line() {
    // A work tree with no project yet: the same negative answer, and
    // nothing created. (The message itself is pinned in watch.test.rs.)
    let env = Env::new(SESSION);
    refused(&env, &[], 1);
}

// ------------------------------------------------------------ 2. the priming line

#[test]
fn test_the_priming_line_is_first_and_history_is_not_replayed() {
    let env = world();
    add(&env, "Before the watch");
    let project = project_id(&env);

    let w = watch(&env, &[]);
    // First, before any change, one JSON document with the version, the
    // project and the store's position now.
    primed(&w, &project);

    // A watch without --since starts at the head: the task added before it
    // is not delivered, the one added after is.
    let after = add(&env, "After the watch");
    let line = next(&w);
    assert_change(&line, &after, "After the watch", "To Do", json!([]));
    assert_eq!(sequence(&line), 0, "its stream's first event");
}

// ------------------------------------------------------------ 3. a line per event

#[test]
fn test_each_change_is_a_line_with_the_harp_the_new_state_the_stream_and_the_sequence() {
    let env = world();
    let project = project_id(&env);
    let w = watch(&env, &[]);
    primed(&w, &project);

    let harp = add(&env, "Alpha");
    let added = next(&w);
    assert_change(&added, &harp, "Alpha", "To Do", json!([]));
    assert_eq!(added["project"], project, "{added}");
    assert_eq!(sequence(&added), 0, "{added}");
    cursor_of(&added);

    set_text(&env, &harp, "Beta");
    let edited = next(&w);
    assert_change(&edited, &harp, "Beta", "To Do", json!([]));

    ok(&env, &["tasks", "set", &harp, "--status", "In Progress"]);
    let moved = next(&w);
    assert_change(&moved, &harp, "Beta", "In Progress", json!([]));

    ok(&env, &["tasks", "tag", "add", &harp, "area:cli"]);
    let tagged = next(&w);
    assert_change(&tagged, &harp, "Beta", "In Progress", json!(["area:cli"]));

    // One stream (the task's), numbered 0, 1, 2, 3 with no gap.
    let lines = [added, edited, moved, tagged];
    assert!(
        lines.iter().all(|l| stream(l) == stream(&lines[0])),
        "one task, one stream: {lines:?}"
    );
    assert_eq!(
        sequences_on(&lines, &stream(&lines[0])),
        [0, 1, 2, 3],
        "{lines:?}"
    );
}

#[test]
fn test_two_tasks_are_two_streams_each_counted_from_zero() {
    let env = world();
    let project = project_id(&env);
    let w = watch(&env, &[]);
    primed(&w, &project);

    let a = add(&env, "A");
    let b = add(&env, "B");
    set_text(&env, &a, "A2");
    // Three lines; their order ACROSS the two streams is not promised, so
    // only each stream's own order is held.
    let lines = take(&w, 3);
    let of = |harp: &str| -> Vec<Value> {
        lines
            .iter()
            .filter(|l| l["harp_id"] == harp)
            .cloned()
            .collect()
    };
    let (of_a, of_b) = (of(&a), of(&b));
    assert_eq!(of_a.len(), 2, "{lines:?}");
    assert_eq!(of_b.len(), 1, "{lines:?}");
    assert_ne!(stream(&of_a[0]), stream(&of_b[0]), "{lines:?}");
    assert_eq!(sequences_on(&lines, &stream(&of_a[0])), [0, 1]);
    assert_eq!(sequences_on(&lines, &stream(&of_b[0])), [0]);
}

#[test]
fn test_a_cursor_is_an_opaque_string_on_every_change_line() {
    let env = world();
    let project = project_id(&env);
    let w = watch(&env, &[]);
    primed(&w, &project);
    add(&env, "One");
    add(&env, "Two");
    let lines = take(&w, 2);
    for line in &lines {
        assert!(!cursor_of(line).is_empty(), "{line}");
    }
    assert_ne!(
        cursor_of(&lines[0]),
        cursor_of(&lines[1]),
        "a cursor moves with the lines it covers"
    );
}

// ------------------------------------------------------------ 4. resume

#[test]
fn test_a_cursor_from_the_priming_line_resumes_at_the_head_it_named() {
    let env = world();
    let project = project_id(&env);
    let first = watch(&env, &[]);
    let head = cursor_of(&primed(&first, &project));
    let harp = add(&env, "While nothing watches");
    let seen = next(&first);
    drop(first);

    // The same cursor, replayed: the event after it, once, with the same
    // position as the first watch gave it.
    let again = watch(&env, &["--since", &head]);
    primed(&again, &project);
    let replayed = next(&again);
    assert_change(
        &replayed,
        &harp,
        "While nothing watches",
        "To Do",
        json!([]),
    );
    assert_eq!(
        (stream(&replayed), sequence(&replayed)),
        (stream(&seen), sequence(&seen))
    );

    // And nothing else came: the next line is the next change.
    set_text(&env, &harp, "Next");
    let following = next(&again);
    assert_change(&following, &harp, "Next", "To Do", json!([]));
    assert_eq!(
        sequence(&following),
        1,
        "no repeat of 0, no gap: {following}"
    );
}

#[test]
fn test_resume_delivers_exactly_the_events_after_a_line_across_streams() {
    let env = world();
    let project = project_id(&env);

    // First run: three events on two streams.
    let first = watch(&env, &[]);
    primed(&first, &project);
    let b = add(&env, "B");
    let c = add(&env, "C");
    set_text(&env, &b, "B2");
    let lines = take(&first, 3);
    // A cursor covers every line before it on the wire.
    let after_all = cursor_of(&lines[2]);
    let after_first = cursor_of(&lines[0]);
    drop(first);

    // While no watch runs: more changes.
    set_text(&env, &c, "C2");

    // From the last line's cursor: only the change made while down.
    let second = watch(&env, &["--since", &after_all]);
    primed(&second, &project);
    let only = next(&second);
    assert_change(&only, &c, "C2", "To Do", json!([]));
    assert_eq!(sequence(&only), 1, "{only}");
    // A sentinel on the stream of the line just read: it is the next line,
    // so nothing was repeated in between.
    set_text(&env, &b, "B3");
    let sentinel = next(&second);
    assert_change(&sentinel, &b, "B3", "To Do", json!([]));
    assert_eq!(sequence(&sentinel), 2, "{sentinel}");
    drop(second);

    // From the first line's cursor: the two events after it in the first
    // run, the one made while down and the sentinel; none of the first.
    let third = watch(&env, &["--since", &after_first]);
    primed(&third, &project);
    let replayed = take(&third, 3);
    let mut expected = positions(&lines[1..]);
    expected.insert((stream(&only).to_string(), 1));
    expected.insert((stream(&sentinel).to_string(), 2));
    // Four events were after the first line: lines 2 and 3, `only`, and
    // the sentinel; the third read is therefore not yet the end.
    let mut replayed = replayed;
    replayed.push(next(&third));
    assert_eq!(positions(&replayed), expected, "{replayed:?}");
    assert_eq!(
        replayed.len(),
        4,
        "each exactly once, never the first line's own event"
    );
    for line in &replayed {
        let on = stream(line);
        let seq = sequences_on(&replayed, &on);
        assert!(
            seq.windows(2).all(|w| w[1] == w[0] + 1),
            "a stream's events arrive in order, with no gap: {seq:?}"
        );
    }
    // Quiet after that: a sentinel is the next line.
    set_text(&env, &c, "C3");
    let last = next(&third);
    assert_change(&last, &c, "C3", "To Do", json!([]));
    assert_eq!(sequence(&last), 2, "{last}");
}

// ------------------------------------------------------------ 5. invalid and pruned cursors

#[test]
fn test_a_malformed_cursor_is_exit_2() {
    let env = world();
    let real = some_cursor(&env);
    let cut = real[..real.len() / 2].to_string();
    for bad in [
        "",
        "not a cursor",
        "%%%%",
        "42",
        "123e4567-e89b-12d3-a456-426614174000",
        "a.b.c",
        cut.as_str(),
    ] {
        // Invalid on its own content: refused before any state is read,
        // so exit 2 and no priming line.
        let ended = refused(&env, &["--since", bad], 2);
        let _ = ended;
    }
}

#[test]
fn test_a_malformed_cursor_is_exit_2_even_where_there_is_no_project() {
    // Invalid on its own content beats "no project": 2, not 1.
    let env = Env::new(SESSION);
    refused(&env, &["--since", "not a cursor"], 2);
    // And with an unknown project too.
    let env = world();
    refused(
        &env,
        &["--project", "no-such-project", "--since", "not a cursor"],
        2,
    );
}

#[test]
fn test_a_cursor_of_an_unknown_version_is_exit_2() {
    // PROPOSAL (pending owner review): the encoding assumed here is base64url
    // of a JSON object with a `v` member; see the file header.
    let env = world();
    let real = some_cursor(&env);
    let mut forged: Value =
        serde_json::from_slice(&b64url_decode(&real)).expect("the proposed encoding: JSON");
    assert_eq!(forged["v"], 1, "the cursor carries its version: {forged}");
    forged["v"] = json!(999);
    let forged = b64url_encode(forged.to_string().as_bytes());
    refused(&env, &["--since", &forged], 2);
}

#[test]
fn test_a_cursor_whose_stream_was_pruned_is_reported_once_as_expired() {
    // PROPOSAL (pending owner review): a pruned stream is dropped from the
    // cursor and reported ONCE with an `expired` line naming the stream; the
    // watch goes on. (Alternatives, unpinned: an error exit, or silence.)
    let env = world();
    let project = project_id(&env);
    let doomed = add(&env, "Doomed");

    let first = watch(&env, &[]);
    primed(&first, &project);
    set_text(&env, &doomed, "Doomed 2");
    let seen = next(&first);
    let before_prune = cursor_of(&seen);
    drop(first);

    ok(&env, &["tasks", "set", &doomed, "--status", "Done"]);
    let pruned = ok(&env, &["tasks", "prune", "--older-than", "1ns", "--yes"]);
    assert_eq!(pruned["pruned"][0]["harp_id"], doomed.as_str(), "{pruned}");

    let second = watch(&env, &["--since", &before_prune]);
    primed(&second, &project);
    let expired = next(&second);
    assert_eq!(expired["v"], 1, "{expired}");
    assert_eq!(expired["event"], "expired", "{expired}");
    assert_eq!(stream(&expired), stream(&seen), "{expired}");

    // It goes on: a new task is the next line, with no second `expired`.
    let fresh = add(&env, "Fresh");
    let line = next(&second);
    assert_change(&line, &fresh, "Fresh", "To Do", json!([]));
    assert_eq!(sequence(&line), 0, "{line}");
    let dropped = cursor_of(&line);
    drop(second);

    // The cursor after that no longer holds the pruned stream: reported
    // once, not on every resume.
    let third = watch(&env, &["--since", &dropped]);
    primed(&third, &project);
    let other = add(&env, "Other");
    let line = next(&third);
    assert_change(&line, &other, "Other", "To Do", json!([]));
}

#[test]
fn test_plans_watch_takes_no_cursor() {
    // The cursor is for the task store's streams; `plans watch` watches
    // files. Not an option it has: a usage error.
    let env = world();
    let ended = ends(
        &env,
        &["plans", "watch", "--since", "x", "--format", "json"],
    );
    assert_eq!(ended.code, Some(2), "{}{}", ended.stdout, ended.stderr);
}

// ------------------------------------------------------------ 6. a new stream starts at 0

#[test]
fn test_a_stream_new_since_the_cursor_is_delivered_from_sequence_0() {
    let env = world();
    let project = project_id(&env);
    let first = watch(&env, &[]);
    let head = cursor_of(&primed(&first, &project));
    drop(first);

    // The task did not exist when the cursor was taken: the cursor has no
    // entry for its stream, and it is read from the start, not skipped and
    // not read from the head.
    let harp = add(&env, "Born later");
    set_text(&env, &harp, "Born later, edited");
    ok(&env, &["tasks", "tag", "add", &harp, "area:cli"]);

    let second = watch(&env, &["--since", &head]);
    primed(&second, &project);
    let lines = take(&second, 3);
    assert_eq!(sequences_on(&lines, &stream(&lines[0])), [0, 1, 2]);
    assert_change(&lines[0], &harp, "Born later", "To Do", json!([]));
    assert_change(
        &lines[2],
        &harp,
        "Born later, edited",
        "To Do",
        json!(["area:cli"]),
    );
}

// ------------------------------------------------------------ 7. heartbeat

#[test]
fn test_a_quiet_store_gets_heartbeat_lines_at_the_interval() {
    // PROPOSAL (pending owner review): `--heartbeat <seconds>`, default 15;
    // the default is not waited for here, a short interval is. A heartbeat
    // is a line of its own kind, versioned, carrying the cursor.
    let env = world();
    let project = project_id(&env);
    let w = watch(&env, &["--heartbeat", "1"]);
    let head = cursor_of(&primed(&w, &project));
    for _ in 0..2 {
        let beat = one_document(&w.line());
        assert_eq!(beat["v"], 1, "{beat}");
        assert_eq!(beat["event"], "heartbeat", "{beat}");
        // Nothing changed, so the position is the one already given.
        assert_eq!(cursor_of(&beat), head, "{beat}");
    }
    // And a change still arrives among them.
    let harp = add(&env, "Between beats");
    let line = next(&w);
    assert_change(&line, &harp, "Between beats", "To Do", json!([]));
}

#[test]
fn test_a_heartbeat_that_is_not_a_number_is_exit_2() {
    let env = world();
    refused(&env, &["--heartbeat", "soon"], 2);
}

#[test]
fn test_the_help_names_the_cursor_and_the_heartbeat() {
    // `watch` is documented in help (ruling 6): its line format is a
    // supported contract.
    let env = world();
    let ran = env.run(&["tasks", "watch", "--help"]);
    assert!(ran.ok, "{}", ran.stderr);
    assert!(ran.stdout.contains("--since"), "{}", ran.stdout);
    assert!(ran.stdout.contains("--heartbeat"), "{}", ran.stdout);
}

// ------------------------------------------------------------ 8. stopping

#[test]
fn test_sigterm_ends_a_resumed_watch_with_exit_0_and_nothing_written_after() {
    let env = world();
    let project = project_id(&env);
    let first = watch(&env, &[]);
    let head = cursor_of(&primed(&first, &project));
    add(&env, "Replayed on resume");
    next(&first);
    drop(first);

    // Resumed, with a heartbeat armed: stopping does not wait for the next
    // beat, and no half line or late beat follows the stop (`stop` fails if
    // anything is written after the lines already read).
    let w = watch(&env, &["--since", &head, "--heartbeat", "1"]);
    primed(&w, &project);
    next(&w);
    if let Some(status) = w.stop("TERM") {
        assert!(status.success(), "SIGTERM is a clean end: {status}");
    }
}

#[test]
fn test_sigint_ends_a_watch_with_exit_0_between_heartbeats() {
    let env = world();
    let project = project_id(&env);
    let w = watch(&env, &["--heartbeat", "1"]);
    primed(&w, &project);
    assert_eq!(one_document(&w.line())["event"], "heartbeat");
    if let Some(status) = w.stop("INT") {
        assert!(status.success(), "SIGINT is a clean end: {status}");
    }
}

// ------------------------------------------------------------ 9. versioned

#[test]
fn test_every_kind_of_line_carries_the_version() {
    let env = world();
    let project = project_id(&env);
    let doomed = add(&env, "Doomed");
    let first = watch(&env, &[]);
    primed(&first, &project);
    set_text(&env, &doomed, "Doomed 2");
    let cursor = cursor_of(&next(&first));
    drop(first);
    ok(&env, &["tasks", "set", &doomed, "--status", "Done"]);
    ok(&env, &["tasks", "prune", "--older-than", "1ns", "--yes"]);

    // Priming, expired, changed and heartbeat: all `v: 1`.
    let w = watch(&env, &["--since", &cursor, "--heartbeat", "1"]);
    let mut kinds = BTreeSet::new();
    let mut lines = vec![one_document(&w.line())];
    let harp = add(&env, "Versioned");
    while !lines
        .iter()
        .any(|l| l["event"] == "changed" && l["harp_id"] == harp.as_str())
        || !lines.iter().any(|l| l["event"] == "heartbeat")
    {
        lines.push(one_document(&w.line()));
    }
    for line in &lines {
        assert_eq!(line["v"], 1, "{line}");
        kinds.insert(line["event"].as_str().unwrap_or("").to_string());
    }
    assert!(kinds.contains("expired"), "{kinds:?}");
    assert!(kinds.contains("changed"), "{kinds:?}");
    assert!(kinds.contains("heartbeat"), "{kinds:?}");
}

// ------------------------------------------------------------ the machine and text formats

#[test]
fn test_text_mode_is_still_one_line_per_event() {
    let env = world();
    let w = Watching::start(&env, &["--format", "text"]);
    // The priming line, whatever it says.
    w.line();
    let a = add(&env, "Text A");
    let line = w.line();
    assert!(line.contains(&a), "a line names the task: {line:?}");
    let b = add(&env, "Text B");
    let line = w.line();
    assert!(
        line.contains(&b) && !line.contains(&a),
        "the second event is its own line, the first not repeated: {line:?}"
    );
}

#[test]
fn test_format_json_is_jsonl_one_document_a_line() {
    let env = world();
    let project = project_id(&env);
    let w = watch(&env, &["--heartbeat", "1"]);
    primed(&w, &project);
    add(&env, "Lines");
    // `w.line()` is one line; each must be exactly one JSON document.
    for _ in 0..3 {
        let line = w.line();
        assert!(
            !line.trim().is_empty() && one_document(&line).is_object(),
            "{line:?}"
        );
    }
}

// ------------------------------------------------ orchestrator decisions

/// Decode a cursor under the PROPOSAL encoding (see the file header).
fn decoded(cursor: &str) -> Value {
    serde_json::from_slice(&b64url_decode(cursor)).expect("the proposed encoding: JSON")
}

fn encoded(cursor: &Value) -> String {
    b64url_encode(cursor.to_string().as_bytes())
}

/// Whether `n` appears in `text` as a whole number.
fn has_number(text: &str, n: u64) -> bool {
    text.split(|c: char| !c.is_ascii_digit())
        .any(|t| t == n.to_string())
}

#[test]
fn test_resume_primes_with_the_requested_cursor_not_the_head() {
    // Orchestrator decision, pending owner review: saving the head from the
    // priming line would skip the replay.
    let env = world();
    let project = project_id(&env);
    let first = watch(&env, &[]);
    let head = cursor_of(&primed(&first, &project));
    add(&env, "Missed");
    next(&first);
    drop(first);

    let second = watch(&env, &["--since", &head]);
    let priming = primed(&second, &project);
    assert_eq!(cursor_of(&priming), head, "{priming}");
    next(&second);
}

#[test]
fn test_a_cursor_carries_the_project_and_a_foreign_projects_is_exit_2() {
    // Orchestrator decision, pending owner review. The `project` member
    // assumes the PROPOSAL encoding; the refusal does not.
    let env = world();
    let project = project_id(&env);
    let cursor = some_cursor(&env);
    assert_eq!(decoded(&cursor)["project"], project.as_str());

    let other = world();
    assert_ne!(project_id(&other), project);
    // Invalid on its own content: refused before any stream is read.
    refused(&other, &["--since", &cursor], 2);
}

#[test]
fn test_only_the_tasks_domain_appears_in_the_cursor_and_as_lines() {
    // Orchestrator decision, pending owner review: the schema stream (a
    // declare writes to it) and other domains are not watched.
    let env = world();
    let project = project_id(&env);
    let w = watch(&env, &[]);
    let head = cursor_of(&primed(&w, &project));
    ok(
        &env,
        &[
            "manage",
            "schema",
            "declare",
            "--add",
            r#"tagma.permit:"x:*""#,
            "--yes",
        ],
    );
    let harp = add(&env, "After the declare");
    let line = next(&w);
    assert_change(&line, &harp, "After the declare", "To Do", json!([]));
    assert_eq!(sequence(&line), 0, "no line for the schema stream: {line}");
    let prefix = format!("{}/", tasks::TASK_DOMAIN);
    for cursor in [head, cursor_of(&line)] {
        let streams = decoded(&cursor)["streams"].clone();
        let keys: Vec<&String> = streams.as_object().expect("streams").keys().collect();
        assert!(!keys.is_empty());
        assert!(keys.iter().all(|k| k.starts_with(&prefix)), "{keys:?}");
    }
}

#[test]
fn test_heartbeat_0_disables_heartbeats() {
    // Orchestrator decision, pending owner review. Bounded wait: with a
    // 1 s interval a beat would come in this time.
    let env = world();
    let project = project_id(&env);
    let mut w = watch(&env, &["--heartbeat", "0"]);
    primed(&w, &project);
    assert_eq!(w.try_line(Duration::from_secs(4)), None, "no beat");
    assert!(w.alive(), "0 is not an error");
    let harp = add(&env, "Still delivered");
    assert_change(&next(&w), &harp, "Still delivered", "To Do", json!([]));
}

#[test]
fn test_heartbeats_appear_in_text_mode_as_one_line() {
    // Orchestrator decision, pending owner review. The word is a guess at
    // the catalog text: it must say `heartbeat`.
    let env = world();
    let w = Watching::start(&env, &["--format", "text", "--heartbeat", "1"]);
    w.line();
    for _ in 0..2 {
        let beat = w.line();
        assert!(beat.to_lowercase().contains("heartbeat"), "{beat:?}");
    }
}

#[test]
fn test_a_stream_below_the_cursor_is_expired_and_the_new_one_starts_at_0() {
    // Orchestrator decision, pending owner review: a stream whose head is
    // below the cursor's next sequence was pruned (and maybe re-minted).
    // Black box cannot re-mint a harp, so the cursor is forged ahead of an
    // existing stream (PROPOSAL encoding).
    let env = world();
    let project = project_id(&env);
    let w = watch(&env, &[]);
    let seeded = decoded(&cursor_of(&primed(&w, &project)))["streams"].clone();
    let harp = add(&env, "Re-minted");
    let seen = next(&w);
    let mut cursor = decoded(&cursor_of(&seen));
    let streams = cursor["streams"].as_object_mut().expect("streams");
    assert_eq!(
        streams.len(),
        2,
        "the seed's stream and this one: {streams:?}"
    );
    // Only this task's stream is forged ahead (setup fixed after review,
    // 2026-11-03: the loop forged every stream at 1, and the seed's stream
    // is at 1 too, so a second `expired` line could come before the pair
    // the assertions read, in an order across streams that is not
    // promised). This task's is the one the priming cursor did not hold.
    for (key, next_sequence) in streams.iter_mut() {
        if seeded.get(key).is_none() {
            *next_sequence = json!(99);
        }
    }
    drop(w);

    let second = watch(&env, &["--since", &encoded(&cursor)]);
    primed(&second, &project);
    let lines = take(&second, 2);
    let expired = lines
        .iter()
        .find(|l| l["event"] == "expired")
        .expect("expired");
    assert_eq!(stream(expired), stream(&seen));
    let again = lines
        .iter()
        .find(|l| l["event"] == "changed")
        .expect("changed");
    assert_change(again, &harp, "Re-minted", "To Do", json!([]));
    assert_eq!(sequence(again), 0, "the new stream from the start");
}

#[test]
fn test_an_unknown_version_names_the_supported_one() {
    // Orchestrator decision, pending owner review.
    let env = world();
    let mut forged = decoded(&some_cursor(&env));
    forged["v"] = json!(999);
    let ended = refused(&env, &["--since", &encoded(&forged)], 2);
    assert!(has_number(&ended.stderr, 999), "{}", ended.stderr);
    assert!(has_number(&ended.stderr, 1), "{}", ended.stderr);
}

#[test]
fn test_text_mode_keeps_one_line_per_event_for_one_task() {
    // Orchestrator decision, pending owner review: two events on one
    // stream are two lines, each naming the task.
    let env = world();
    let w = Watching::start(&env, &["--format", "text"]);
    w.line();
    let harp = add(&env, "Text");
    set_text(&env, &harp, "Text 2");
    for _ in 0..2 {
        let line = w.line();
        assert!(line.contains(&harp), "{line:?}");
    }
    assert_eq!(w.try_line(Duration::from_secs(2)), None, "no extra line");
}
