//! Canonical paths.

use std::path::{Path, PathBuf};

use file_system::{Fs, Rooted};

use os::Mode;

use os::Flavor;

use super::{canonical, clean, displayed, same};

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
    let real = os::fixture("/d/real");
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

#[test]
fn test_a_path_is_displayed_without_a_windows_verbatim_prefix() {
    let verbatim = PathBuf::from(r"\\?\C:\Users\u");
    assert_eq!(
        displayed(Flavor::Windows, verbatim.clone()),
        Path::new(r"C:\Users\u")
    );
    assert_eq!(displayed(Flavor::Unix, verbatim.clone()), verbatim);
    let odd = PathBuf::from(os::not_unicode("/a"));
    assert_eq!(
        displayed(Flavor::Windows, odd.clone()),
        odd,
        "not UTF-8: as it is"
    );
}

#[test]
fn test_paths_are_the_same_as_the_flavor_compares_them() {
    let (a, b) = (Path::new(r"C:\Users\U\"), Path::new("c:/users/u"));
    assert!(same(Flavor::Windows, a, b));
    assert!(!same(Flavor::Unix, a, b));
    assert!(same(Flavor::Unix, Path::new("/a/b"), Path::new("/a/b")));
    assert!(!same(Flavor::Unix, Path::new("/a/B"), Path::new("/a/b")));
    let odd = PathBuf::from(os::not_unicode("/a"));
    assert!(same(Flavor::Windows, &odd, &odd.clone()));
    assert!(!same(Flavor::Windows, &odd, Path::new("/a")));
}
