//! Canonical paths.

use std::path::{Path, PathBuf};

use file_system::{Fs, Mode, Rooted};

use super::{canonical, clean};

#[test]
fn test_a_clean_is_gos_filepath_clean() {
    for (path, cleaned) in [
        ("/a/b/c", "/a/b/c"),
        ("/a/b/c/", "/a/b/c"),
        ("/a//b/./c", "/a/b/c"),
        ("/a/b/../c", "/a/c"),
        ("/a/b/../../..", "/"),
        ("/..", "/"),
        ("a/./b/", "a/b"),
        ("a/../..", ".."),
        ("../a/..", ".."),
        ("../../a", "../../a"),
        ("./a", "a"),
        (".", "."),
        ("", "."),
    ] {
        assert_eq!(clean(Path::new(path)), Path::new(cleaned), "{path:?}");
    }
}

#[test]
fn test_a_canonical_path_resolves_symlinks() {
    let fs = Rooted::temp().unwrap();
    let real = PathBuf::from("/d/real");
    fs.create_dir_all(&real, Mode::DIR).unwrap();
    let link = real.with_file_name("link");
    fs.symlink_dir(&real, &link).unwrap();
    assert_eq!(canonical(&fs, &link.join("")), real);
    assert_eq!(canonical(&fs, &link.join("../link/.")), real);
}

#[test]
fn test_a_path_that_does_not_resolve_is_cleaned() {
    assert_eq!(
        canonical(&Rooted::temp().unwrap(), Path::new("/no/such/../place/")),
        PathBuf::from("/no/place")
    );
}
