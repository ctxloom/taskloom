//! The registry file: its schema read and written, strictly: a key this
//! version does not know is refused, and so is a version it does not read
//! (architecture-fixes B4; the owner's rulings of 2026-10-31: one reader,
//! angzarr-yaml, for every Rust config, strict for all files with more than
//! one writer).

use std::path::{Path, PathBuf};

crate::english_for!(&[crate::EN_US.1, file_lock::EN_US.1, angzarr_yaml::EN_US.1]);

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

/// A registry as Go's yaml.v3 writes it (its sequence items not indented),
/// one entry seen and one not.
const GO_WRITTEN: &str = "\
schema_version: 1
projects:
- project_id: jolly-mute-magma
  path: /tmp/ctxloom-smoke
  created_at: 2026-06-10T02:35:20.837009562Z
  last_seen_at: 2026-06-10T02:35:20.837009562Z
- project_id: high-tight-gulf
  path: /work/main
  created_at: 2026-06-10T02:42:33.650686047Z
";

/// What `load` of `text` is refused with, as displayed: the reader's code
/// and args.
fn refused(text: &str) -> String {
    let (fs, dir) = at();
    let file = dir.join("index.yaml");
    put(&fs, &file, text);
    let error = load(&fs, &file).unwrap_err();
    let shown = error.to_string();
    assert!(
        shown.starts_with(&format!(
            "angzarr-yaml-load-failed-at kind=project-registry source={}",
            angzarr_diagnostic::quote(&file.display().to_string())
        )),
        "{shown}"
    );
    assert!(error.english().contains("index.yaml"), "{error}");
    shown
}

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

// Strict for all (the owner's ruling, 2026-10-31): a key this version does
// not know, in an entry or at the root, is refused with its key path, not
// kept; Go's and Rust's writers change the schema in lockstep.
#[test]
fn test_a_key_this_version_does_not_know_is_refused() {
    let entry = GO_WRITTEN.replace("  path: /work/main\n", "  path: /work/main\n  note: kept\n");
    assert!(
        refused(&entry)
            .contains("problem=angzarr-yaml-decode-unknown-key problem.key_path=projects[1].note"),
        "{entry}"
    );
    let root = format!("{GO_WRITTEN}later: field\n");
    assert!(
        refused(&root).contains("problem=angzarr-yaml-decode-unknown-key problem.key_path=later"),
        "{root}"
    );
}

// The schema version is checked on read, as Go's schemaver checks it (the
// kind at 1, with no older version): a newer registry is refused, and so
// is one with no stamp or one that is not a number.
#[test]
fn test_a_version_this_registry_does_not_read_is_refused() {
    let newer = GO_WRITTEN.replace("schema_version: 1", "schema_version: 2");
    assert!(
        refused(&newer).contains("problem=angzarr-yaml-too-new problem.found=2 problem.current=1")
    );
    let unstamped = GO_WRITTEN.replace("schema_version: 1\n", "");
    assert!(refused(&unstamped).contains("problem=angzarr-yaml-too-old problem.found=0"));
    let word = GO_WRITTEN.replace("schema_version: 1", "schema_version: one");
    assert!(refused(&word).contains("problem=angzarr-yaml-version-not-an-integer"));
}

#[test]
fn test_saving_keeps_what_was_read() {
    let (fs, dir) = at();
    let file = dir.join("index.yaml");
    put(&fs, &file, GO_WRITTEN);
    let mut index = load(&fs, &file).unwrap();
    index.register("calm-otter-fox", "/new");
    save(&fs, &file, &index).unwrap();
    let again = load(&fs, &file).unwrap();
    assert_eq!(again, index);
    assert_eq!(again.projects.len(), 3);
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
    assert!(
        refused("schema_version: 1\nprojects: [unterminated").contains("problem=angzarr-yaml-"),
        "the reader's parse failure"
    );
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
    // RFC 3339 scalar; quoted, it is a string and the decode fails. The
    // sequence's items are indented, as the reader's writer lays them out;
    // yaml.v3 reads either.
    let (fs, dir) = at();
    let file = dir.join("index.yaml");
    let mut index = Index::default();
    index.projects.push(super::Entry {
        project_id: "jolly-mute-magma".to_string(),
        path: "/work/tree".to_string(),
        created_at: "2026-06-10T02:35:20.837009562Z".to_string(),
        last_seen_at: "2026-06-10T02:35:20.837009562Z".to_string(),
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
            cause: angzarr_diagnostic::Report::new("file-lock-not-regular")
                .arg("path", lock_path.display().to_string()),
        })
    );
}
