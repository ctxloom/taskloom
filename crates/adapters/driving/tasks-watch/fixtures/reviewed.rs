//! What `taskloom tasks watch --help` says at length: unix's words (Go's
//! `taskloom watch` help, with the store and closed-stdin changes Rust's
//! watch already carried, and, with consolidation slice D (2026-11-03),
//! the cursor, the heartbeat and the exit codes: those words are the
//! slice's, pending the owner's review), with the departures the human
//! reviewed and approved applied on the hosts they apply to.
//!
//! Each departure is an exact `(old, new)` pair on unix's words, as
//! registration-cli's are (`registration-cli/fixtures/reviewed.rs`). Each
//! `old` must occur exactly once: if the words are changed, the pair no
//! longer matches and the test fails until the pair is reviewed again.

/// Unix's words: what stops the watch is Go's (`Ctrl-C / SIGTERM`).
pub const ON_UNIX: &str =
    "Watch the project's task store and write one line per task event, resumable.

Long-lived: runs until interrupted (Ctrl-C / SIGTERM); a closed stdin does not
stop it. A priming line at once, then one line per event with its stream,
sequence and the task as it is then, each carrying a cursor, e.g.
{\"v\":1,\"event\":\"changed\",\"project\":\"swift-amber-falcon\",...,\"cursor\":\"eyJ2...\"}.
Pass a line's cursor as --since to resume after it: the lines you missed are
replayed, none twice. Cursors are per stream, so no order across tasks is
promised. A stream pruned since is reported once as expired. A heartbeat
line comes after each --heartbeat quiet seconds (default 15, 0 for none).
Exit 2 for a cursor that is malformed, of an unknown version or another
project's; 1 for a project with no store.";

/// Departures on Windows (the human's ruling of 2026-10-28): the help
/// names what stops the watch there, as the os crate's stop signal hears
/// it (Ctrl-C and Ctrl-Break, and the console's close, the user's logoff
/// and the system's shutdown), where unix's words say Ctrl-C and SIGTERM,
/// a signal Windows does not send.
pub const ON_WINDOWS: &[(&str, &str)] = &[(
    "Long-lived: runs until interrupted (Ctrl-C / SIGTERM); a closed stdin does not\n",
    "Long-lived: runs until interrupted (Ctrl-C or Ctrl-Break, or the console's\n\
     close, the user's logoff or the system's shutdown); a closed stdin does not\n",
)];

/// The long help as a host of `flavor` says it: Windows' departures
/// applied on Windows, unix's words unchanged on unix.
pub fn said(flavor: os::Flavor) -> String {
    match flavor {
        os::Flavor::Windows => applied(ON_UNIX, ON_WINDOWS),
        os::Flavor::Unix => ON_UNIX.to_string(),
    }
}

/// `words` with each pair applied, each `old` found exactly once.
pub fn applied(words: &str, pairs: &[(&str, &str)]) -> String {
    let mut text = words.to_string();
    for (old, new) in pairs {
        let found = text.matches(old).count();
        assert!(
            found == 1,
            "a reviewed change's old text occurs {found} times, not once, in unix's words: \
             they have changed, so review the change again: {old:?}"
        );
        text = text.replacen(old, new, 1);
    }
    text
}
