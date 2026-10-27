//! Flat names, and records read for the one fact they are read for. The
//! names and records are recorded ones: written by taskloom's `manage
//! install` (fixtures/go-records, the home as `$HOME`).

use std::path::{Path, PathBuf};

use file_system::{Fs, Os, Perms, Rooted};

use os::Mode;

use super::added;

/// `path`'s flat name on the host.
fn flat_name(path: &str) -> String {
    super::flat_name(os::host().flavor(), path)
}

/// The recorded record `fixture`, read from this crate's fixtures.
fn fixture(fixture: &str) -> String {
    Os::new(os::host(), Path::new(env!("CARGO_MANIFEST_DIR")))
        .unwrap()
        .read_to_string(Path::new(&format!(
            "fixtures/go-records/{fixture}.hew-record.yaml"
        )))
        .unwrap()
}

/// A filesystem of the test's own, its record directory made, and the
/// configuration the records are of.
fn records() -> (Rooted, PathBuf, String) {
    let fs = Rooted::temp().unwrap();
    let dir = PathBuf::from("/home/u/.ctxloom/records/taskloom");
    fs.create_dir_all(&dir, Mode::DIR).unwrap();
    (fs, dir, "/home/u/.claude.json".to_string())
}

fn write(fs: &dyn Fs, path: &Path, text: &str) {
    fs.write_atomic(path, text.as_bytes(), Perms::Exact(Mode::FILE))
        .unwrap();
}

/// The record `fixture`, for the file at `target`, in `dir` under its flat
/// name.
fn lay(fs: &dyn Fs, dir: &Path, fixture_name: &str, target: &str, stamp: &str) {
    let text = fixture(fixture_name).replace("$HOME/.claude.json", target);
    let name = format!("{}__{stamp}.hew-record.yaml", flat_name(target));
    write(fs, &dir.join(name), &text);
}

#[test]
fn test_a_record_says_whether_go_added_the_servers_object_whole() {
    let (fs, dir, target) = records();
    assert!(!added(&fs, &dir, &target, "/mcpServers"), "no record");
    lay(
        &fs,
        &dir,
        "indented-no-servers",
        &target,
        "20261021T070716.759601638Z",
    );
    assert!(added(&fs, &dir, &target, "/mcpServers"));
    assert!(!added(&fs, &dir, &target, "/mcpServers/taskloom"));
    assert!(!added(&fs, &dir, "/elsewhere/.claude.json", "/mcpServers"));
}

#[test]
fn test_the_newest_record_is_the_one_read() {
    let (fs, dir, target) = records();
    lay(
        &fs,
        &dir,
        "indented-no-servers",
        &target,
        "20261021T070716.759601638Z",
    );
    // A later application, into servers that were there already.
    let text = fixture("indented-other-server")
        .replace("$HOME/.claude.json", &target)
        .replace("2026-10-21T07:07:16.774018621Z", "2026-10-22T00:00:00Z");
    let name = format!(
        "{}__20261022T000000.000000000Z.hew-record.yaml",
        flat_name(&target)
    );
    write(&fs, &dir.join(name), &text);
    assert!(!added(&fs, &dir, &target, "/mcpServers"));
    assert!(added(&fs, &dir, &target, "/mcpServers/taskloom"));
}

#[test]
fn test_a_record_that_cannot_be_read_says_nothing() {
    let (fs, dir, target) = records();
    let name = format!("{}__x.hew-record.yaml", flat_name(&target));
    write(&fs, &dir.join(name), "{ not: [yaml");
    assert!(!added(&fs, &dir, &target, "/mcpServers"));
    assert!(!added(&fs, &dir.join("absent"), &target, "/mcpServers"));
}

#[test]
fn test_a_flat_name_keeps_exactly_the_last_ninety_six_bytes() {
    let at = |n: usize| flat_name(&"a".repeat(n));
    assert_eq!(at(96).len(), 96 + 18);
    assert_eq!(at(97).len(), 96 + 18);
    assert!(at(96).starts_with(&"a".repeat(96)));
    let name = flat_name(&format!("b{}", "\u{e9}".repeat(48)));
    assert_eq!(
        &name[..name.len() - 18],
        "\u{e9}".repeat(48),
        "cut forward to a character"
    );
    assert!(flat_name(" ").starts_with(" __"), "a space is kept");
}

#[test]
fn test_only_a_record_of_this_file_and_this_kind_is_read() {
    let (fs, dir, target) = records();
    lay(
        &fs,
        &dir,
        "indented-no-servers",
        &target,
        "20261021T070716.759601638Z",
    );
    let named = dir.join(&fs.read_dir(&dir).unwrap()[0].name);
    let other = named.with_extension("yaml.bak");
    fs.rename(&named, &other).unwrap();
    assert!(
        !added(&fs, &dir, &target, "/mcpServers"),
        "not a record's suffix"
    );
}
