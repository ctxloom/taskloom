//! What `taskloom manage` says: the recorded run's words
//! (`agent-configs/fixtures/go-manage/<case>/*.out`, which
//! `scripts/go-manage-fixtures.sh` recorded on unix), with the
//! departures the human reviewed and approved applied on the hosts they
//! apply to.
//!
//! Each departure is an exact `(old, new)` pair on the recording's bytes,
//! as the MCP text's are (`tasks-mcp/fixtures/reviewed.rs`). Each `old`
//! must occur exactly once: if Go rewords the sentence, the pair no longer
//! matches and the parity test fails until the pair is reviewed again.

/// Departures in an install's words (`install.out`) on Windows (the
/// human's ruling of 2026-10-28, "Windows (c): $PATH vs %PATH%"): the
/// unresolvable-command warning names the search path `%PATH%`, as
/// Windows refers to a variable, where the recording says unix's `$PATH`.
/// (Go's own `exec.LookPath` says `%PATH%` on Windows too; the recording
/// is unix's.) An uninstall says no such warning, so has none.
pub const INSTALL_ON_WINDOWS: &[(&str, &str)] = &[(
    "executable file not found in $PATH",
    "executable file not found in %PATH%",
)];

/// The recorded words `go`, of the recording's `file`, as a host of
/// `flavor` says them: the file's Windows departures applied on Windows,
/// the recording unchanged on unix.
pub fn said(go: &str, file: &str, flavor: os::Flavor) -> String {
    match (flavor, file) {
        (os::Flavor::Windows, "install.out") => applied(go, INSTALL_ON_WINDOWS),
        _ => go.to_string(),
    }
}

/// `go` with each pair applied, each `old` found exactly once.
pub fn applied(go: &str, pairs: &[(&str, &str)]) -> String {
    let mut text = go.to_string();
    for (old, new) in pairs {
        let found = text.matches(old).count();
        assert!(
            found == 1,
            "a reviewed change's old text occurs {found} times, not once, in Go's text: \
             Go has changed it, so review the change again: {old:?}"
        );
        text = text.replacen(old, new, 1);
    }
    text
}
