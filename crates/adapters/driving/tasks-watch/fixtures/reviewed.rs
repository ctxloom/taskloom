//! What `taskloom watch --help` says at length: unix's words (Go's
//! `taskloom watch` help, with the store and closed-stdin changes Rust's
//! watch already carried), with the departures the human reviewed and
//! approved applied on the hosts they apply to.
//!
//! Each departure is an exact `(old, new)` pair on unix's words, as
//! registration-cli's are (`registration-cli/fixtures/reviewed.rs`). Each
//! `old` must occur exactly once: if the words are changed, the pair no
//! longer matches and the test fails until the pair is reviewed again.

/// Unix's words: what stops the watch is Go's (`Ctrl-C / SIGTERM`).
pub const ON_UNIX: &str =
    "Watch the project's task store and emit a JSONL event whenever it changes.

Long-lived: runs until interrupted (Ctrl-C / SIGTERM); a closed stdin does not
stop it. One line at once, then one per burst of changes, e.g.
{\"event\":\"changed\",\"kind\":\"tasks\",\"project\":\"swift-amber-falcon\"}. A GUI
subscribes to this and re-queries the task list on each event, so it stays live
without polling or watching the store files itself (the watch plumbing lives in
the backend, not the client).";

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
