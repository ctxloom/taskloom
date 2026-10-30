//! Acceptance: the companion contract, as ctxloom reads it from the real
//! binary. `version --format json` is fed to a port of the parsing ctxloom
//! does at boot, and `loadout --format yaml` is the reference document with
//! the reviewed changes applied.

use std::process::Command;

use serde::Deserialize;

use super::{binary, disk, Isolated, Ran, Temp};

/// What ctxloom's boot probe and its lock reaper decode from `<bin>
/// version --format json`. A field left out decodes as zero and one it
/// does not know is ignored.
#[derive(Deserialize, Default, Debug)]
#[serde(default)]
struct Info {
    name: String,
    version: String,
    lock_protocol: i64,
}

/// The protocol a running binary must report, at least, before ctxloom
/// removes a lock file it may take.
const GO_LOCK_PROTOCOL: i64 = 2;

/// ctxloom's parse of the version output: the whole output decoded, and a
/// missing version an error, never an empty version.
fn parse(raw: &[u8]) -> Result<Info, String> {
    let info: Info = serde_json::from_slice(raw)
        .map_err(|e| format!("parse version --format json output: {e}"))?;
    if info.version.is_empty() {
        return Err("version --format json output has no version field".to_string());
    }
    Ok(info)
}

/// ctxloom's lock reaper's verdict on a running binary: none when it
/// reports at least the protocol required, else why it predates the
/// re-check.
fn predates_the_recheck(info: &Info) -> Option<String> {
    match info.lock_protocol {
        0 => Some("reports no lock_protocol, so it predates the re-check".to_string()),
        n if n < GO_LOCK_PROTOCOL => Some(format!(
            "reports lock_protocol {n}, below {GO_LOCK_PROTOCOL}"
        )),
        _ => None,
    }
}

/// ctxloom's probe: `<bin> version --format json`, its stdout, failing on
/// a non-zero exit. Run with nothing but a temporary HOME and PATH.
fn probe(home: &std::path::Path) -> Ran {
    let output = Command::new(binary())
        .args(["version", "--format", "json"])
        .current_dir(home)
        .isolated(home)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()
        .expect("taskloom runs");
    Ran::of(output)
}

#[test]
fn test_the_version_probe_parses_as_go_ctxloom_parses_it() {
    let home = Temp::new();
    let ran = probe(home.path());
    assert!(ran.ok, "the probe failed: {}", ran.stderr);
    let info = parse(ran.stdout.as_bytes()).unwrap_or_else(|e| panic!("{e}: {:?}", ran.stdout));
    assert_eq!(info.name, "taskloom");
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(predates_the_recheck(&info), None, "{info:?}");
    assert_eq!(info.lock_protocol, GO_LOCK_PROTOCOL);
    let written = disk::read_dir(home.path()).unwrap();
    assert_eq!(written.len(), 0, "the probe writes nothing: {written:?}");
}

#[test]
fn test_the_ported_parse_refuses_what_go_refuses() {
    assert!(parse(b"not json").is_err());
    assert!(parse(br#"{"name":"taskloom"}"#).is_err());
    let old = parse(br#"{"name":"taskloom","version":"v1","extra":1}"#).unwrap();
    assert!(predates_the_recheck(&old).is_some());
    let older = parse(br#"{"name":"taskloom","version":"v1","lock_protocol":1}"#).unwrap();
    assert!(predates_the_recheck(&older).is_some());
}

/// The VS Code extension's parse of `<bin> --version` (its
/// `parseVersion`): the first line, trimmed; the word after `version`, else
/// its last word; empty when the line is.
fn extension_parse(stdout: &str) -> String {
    let line = stdout.split('\n').next().unwrap_or_default().trim();
    let words: Vec<&str> = line.split_whitespace().collect();
    match words.iter().position(|w| w.eq_ignore_ascii_case("version")) {
        Some(i) if i + 1 < words.len() => words[i + 1].to_string(),
        _ => words.last().copied().unwrap_or_default().to_string(),
    }
}

#[test]
fn test_the_version_flag_prints_gos_line_the_extension_parses() {
    for flag in ["--version", "-v"] {
        let home = Temp::new();
        let output = Command::new(binary())
            .arg(flag)
            .current_dir(home.path())
            .isolated(home.path())
            .output()
            .expect("taskloom runs");
        let ran = Ran::of(output);
        assert!(ran.ok, "{flag} failed: {}", ran.stderr);
        assert_eq!(
            ran.stdout,
            format!("taskloom version {}\n", env!("CARGO_PKG_VERSION")),
            "{flag}"
        );
        assert_eq!(extension_parse(&ran.stdout), env!("CARGO_PKG_VERSION"));
        assert_eq!(ran.stderr, "", "{flag}");
        let written = disk::read_dir(home.path()).unwrap();
        assert_eq!(written.len(), 0, "{flag} writes nothing: {written:?}");
    }
}

#[test]
fn test_the_ported_extension_parse_reads_what_the_extension_reads() {
    assert_eq!(extension_parse("taskloom version v0.7.0-x\n"), "v0.7.0-x");
    assert_eq!(extension_parse("taskloom 1.2.3\nmore"), "1.2.3");
    assert_eq!(extension_parse("  \n"), "");
}

/// The reference loadout document, as `scripts/go-companion-fixtures.sh`
/// copied it from its source, with the reviewed factual changes applied.
#[path = "../../../../adapters/driving/tasks-cli/fixtures/reviewed.rs"]
mod reviewed;

#[test]
fn test_the_loadout_probe_prints_gos_document_reviewed() {
    let home = Temp::new();
    let output = Command::new(binary())
        .args(["loadout", "--format", "yaml"])
        .current_dir(home.path())
        .isolated(home.path())
        .output()
        .expect("taskloom runs");
    let ran = Ran::of(output);
    assert!(ran.ok, "{}", ran.stderr);
    assert!(
        ran.stdout == reviewed::loadout(),
        "the loadout differs from Go's, reviewed"
    );
}

// Nothing served to a model carries a placeholder the human has yet to fill.
#[test]
#[ignore = "the loadout's install placeholder (run.installation) awaits the human's install text; fill it, then un-ignore this"]
fn test_no_served_text_carries_a_placeholder() {
    assert!(
        !tasks_cli::LOADOUT.contains("\u{ab}HUMAN:"),
        "a placeholder is served in the loadout"
    );
}
