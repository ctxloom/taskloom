//! The registry file: its schema read and written, unknown fields kept.

use std::path::{Path, PathBuf};

crate::english_for!(&[crate::EN_US.1, file_lock::EN_US.1]);

use file_system::{Fs, Open, Perms, Rooted};

use os::Mode;

use super::{
    from_lock, load, lock_within, path, save, write_atomically, Index, LOCK_WAIT, SCHEMA_VERSION,
};
use crate::RegistryError;

/// Whether the registry lock at `lock_file` comes free within the wait a
/// change allows, as it must once its guard is dropped. Not at once: flock
/// is per open file description, and a child another thread here forks
/// (git, for the worktree tests) shares every descriptor until it execs.
fn released(fs: &dyn Fs, lock_file: &Path) -> bool {
    file_lock::lock_within(fs, lock_file, LOCK_WAIT).is_ok()
}

/// The lock, waited for as a change waits.
pub(crate) fn lock(fs: &dyn Fs, path: &Path) -> Result<super::Locked, RegistryError> {
    lock_within(fs, path, LOCK_WAIT)
}

/// A filesystem of the test's own, and a directory on it.
fn at() -> (Rooted, PathBuf) {
    let fs = Rooted::temp().unwrap();
    let dir = os::fixture("/d");
    fs.create_dir_all(&dir, Mode::DIR).unwrap();
    (fs, dir)
}

/// An empty filesystem: nothing on it resolves.
fn nowhere() -> Rooted {
    Rooted::temp().unwrap()
}

fn put(fs: &dyn Fs, path: &Path, text: &str) {
    fs.write_atomic(path, text.as_bytes(), Perms::Exact(Mode::FILE))
        .unwrap();
}

const GO_WRITTEN: &str = "\
schema_version: 1
projects:
  - project_id: jolly-mute-magma
    path: /tmp/ctxloom-smoke
    created_at: 2026-06-10T02:35:20.837009562Z
    last_seen_at: 2026-06-10T02:35:20.837009562Z
    note: kept
  - project_id: high-tight-gulf
    path: /work/main
    created_at: 2026-06-10T02:42:33.650686047Z
later: field
";

#[test]
fn test_the_registry_is_under_the_homes_ctxloom_projects() {
    assert_eq!(
        path("/home/u".as_ref()),
        std::path::Path::new("/home/u/.ctxloom/projects/index.yaml")
    );
}

#[test]
fn test_a_registry_go_wrote_is_read_with_each_entry() {
    let (fs, dir) = at();
    let file = dir.join("index.yaml");
    put(&fs, &file, GO_WRITTEN);
    let index = load(&fs, &file).unwrap();
    assert_eq!(index.schema_version, 1);
    assert_eq!(
        index.id_for(&fs, Path::new("/work/main")),
        Some("high-tight-gulf")
    );
    assert_eq!(
        index.id_for(&fs, Path::new("/tmp/ctxloom-smoke")),
        Some("jolly-mute-magma")
    );
    assert_eq!(index.id_for(&fs, Path::new("/elsewhere")), None);
    assert!(index.holds("high-tight-gulf"));
    assert!(!index.holds("other"));
    assert_eq!(index.projects[1].last_seen_at, "", "absent: empty");
}

#[test]
fn test_saving_keeps_what_was_read_and_what_this_version_does_not_know() {
    let (fs, dir) = at();
    let file = dir.join("index.yaml");
    put(&fs, &file, GO_WRITTEN);
    let mut index = load(&fs, &file).unwrap();
    index.register("calm-otter-fox", "/new");
    save(&fs, &file, &index).unwrap();
    let again = load(&fs, &file).unwrap();
    assert_eq!(again.projects[..2], index.projects[..2]);
    assert_eq!(again.projects[0].other["note"], "kept");
    assert_eq!(again.other["later"], "field");
    let text = fs.read_to_string(&file).unwrap();
    assert!(
        !text.contains("last_seen_at: ''"),
        "an absent time stays absent: {text}"
    );
}

#[test]
fn test_a_registered_entry_carries_its_id_path_and_when() {
    let mut index = Index::default();
    index.register("calm-otter-fox", "/new");
    let entry = &index.projects[0];
    assert_eq!(entry.project_id, "calm-otter-fox");
    assert_eq!(entry.path, "/new");
    assert_eq!(entry.created_at, entry.last_seen_at);
    let at = time::OffsetDateTime::parse(
        &entry.created_at,
        &time::format_description::well_known::Rfc3339,
    )
    .expect("RFC 3339");
    assert!((time::OffsetDateTime::now_utc() - at).whole_seconds().abs() < 60);
}

#[test]
fn test_a_new_registry_is_written_at_this_schema_version() {
    let (fs, dir) = at();
    let file = dir.join("deeper").join("index.yaml");
    save(&fs, &file, &Index::default()).unwrap();
    assert_eq!(load(&fs, &file).unwrap().schema_version, SCHEMA_VERSION);
}

#[test]
fn test_a_missing_or_blank_registry_is_empty() {
    let (fs, dir) = at();
    let file = dir.join("index.yaml");
    assert_eq!(load(&fs, &file).unwrap(), Index::default());
    put(&fs, &file, "\n  \n");
    assert_eq!(load(&fs, &file).unwrap(), Index::default());
}

#[test]
fn test_a_registry_that_is_not_yaml_is_refused() {
    let (fs, dir) = at();
    let file = dir.join("index.yaml");
    put(&fs, &file, "projects: [unterminated");
    assert!(matches!(
        load(&fs, &file),
        Err(RegistryError::Unreadable { .. })
    ));
}

#[test]
fn test_a_registry_that_cannot_be_read_fails() {
    let (fs, dir) = at();
    // A directory where the file should be.
    assert!(matches!(load(&fs, &dir), Err(RegistryError::Io { .. })));
}

#[test]
fn test_the_lock_is_the_file_go_locks_and_excludes_another_holder() {
    use fs4::FileExt;

    let (fs, dir) = at();
    let file = dir.join("sub").join("index.yaml");
    let held = lock(&fs, &file).unwrap();
    let lock_file = dir.join("sub").join("index.yaml.lock");
    let other = fs
        .open(&lock_file, Open::Read)
        .expect("the lock file exists");
    assert!(
        FileExt::try_lock(&other).is_err(),
        "held exclusively while the guard lives"
    );
    drop(held);
    drop(other);
    assert!(released(&fs, &lock_file), "released on drop");
}

#[test]
fn test_an_entry_matches_its_path_cleaned() {
    let mut index = Index::default();
    index.register("high-tight-gulf", "/work/./main/");
    assert_eq!(
        index.id_for(&nowhere(), Path::new("/work/main")),
        Some("high-tight-gulf")
    );
}

#[test]
fn test_the_text_written_is_what_gos_yaml_v3_reads() {
    // yaml.v3 decodes `created_at` into a time.Time from an unquoted
    // RFC 3339 scalar; quoted, it is a string and the decode fails.
    let (fs, dir) = at();
    let file = dir.join("index.yaml");
    let mut index = Index::default();
    index.projects.push(super::Entry {
        project_id: "jolly-mute-magma".to_string(),
        path: "/work/tree".to_string(),
        created_at: "2026-06-10T02:35:20.837009562Z".to_string(),
        last_seen_at: "2026-06-10T02:35:20.837009562Z".to_string(),
        other: Default::default(),
    });
    save(&fs, &file, &index).unwrap();
    assert_eq!(
        fs.read_to_string(&file).unwrap(),
        "\
schema_version: 1
projects:
- project_id: jolly-mute-magma
  path: /work/tree
  created_at: 2026-06-10T02:35:20.837009562Z
  last_seen_at: 2026-06-10T02:35:20.837009562Z
"
    );
}

#[test]
fn test_nothing_is_written_or_locked_at_a_relative_path() {
    // A path relative to the working directory would land wherever the
    // process runs: in a crate's source, under its tests.
    let fs = nowhere();
    let relative = Path::new("relative-registry-dir/index.yaml");
    let refused = |result: Result<(), RegistryError>| {
        assert!(
            matches!(&result, Err(RegistryError::RelativePath { path }) if path == "relative-registry-dir/index.yaml"),
            "{result:?}"
        );
    };
    refused(write_atomically(&fs, relative, b"x"));
    refused(save(&fs, relative, &Index::default()));
    refused(lock(&fs, relative).map(drop));
    assert!(!fs.exists(Path::new("relative-registry-dir")).unwrap());
}

// ------------------------------------------------------------ the lock

#[test]
fn test_a_lock_held_past_the_wait_fails_saying_who_to_look_for() {
    let (fs, dir) = at();
    let file = dir.join("index.yaml");
    let held = lock(&fs, &file).unwrap();
    let refused = lock_within(&fs, &file, std::time::Duration::from_millis(30));
    let lock_path = dir.join("index.yaml.lock").display().to_string();
    assert!(matches!(
        refused.as_ref().map(drop),
        Err(RegistryError::LockTimeout { path, waited_ms: 30 }) if *path == lock_path
    ));
    drop(held);
    lock(&fs, &file).expect("free once released");
}

#[test]
fn test_a_lock_failure_keeps_the_registrys_own_codes() {
    use file_lock::LockError;
    let at = Path::new("/h/.ctxloom/projects/index.yaml.lock");
    let lock_path = || at.display().to_string();
    assert_eq!(
        from_lock(LockError::Replaced {
            path: lock_path(),
            attempts: 16
        }),
        RegistryError::LockReplaced {
            path: lock_path(),
            attempts: 16
        }
    );
    assert_eq!(
        from_lock(LockError::Io {
            path: "/h/.ctxloom/projects".to_string(),
            detail: "denied".to_string()
        }),
        RegistryError::Io {
            path: "/h/.ctxloom/projects".to_string(),
            detail: "denied".to_string()
        }
    );
}

#[test]
fn test_a_lock_file_that_is_not_a_file_is_the_locks_failure_naming_it() {
    let (fs, dir) = at();
    let lock_path = dir.join("index.yaml.lock");
    fs.create_dir_all(&lock_path, Mode::DIR).unwrap();
    let refused = lock(&fs, &dir.join("index.yaml")).map(drop);
    assert_eq!(
        refused,
        Err(RegistryError::LockFailed {
            cause: angzarr_embedded_diagnostic::Report::new("file-lock-not-regular")
                .arg("path", lock_path.display().to_string()),
        })
    );
}
