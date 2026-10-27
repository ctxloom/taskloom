//! Linked worktrees made by git itself.

use std::path::{Path, PathBuf};

use file_system::{Fs, Os, Perms, Rooted};

use os::Mode;

use super::primary_checkout;
use crate::tests::git;

/// The real filesystem, where git makes its repositories.
fn real() -> Os {
    Os::new(os::host(), Path::new("/")).unwrap()
}

/// A filesystem of the test's own, and the directory to work in on it.
fn rooted() -> (Rooted, PathBuf) {
    (Rooted::temp().unwrap(), PathBuf::from("/d"))
}

fn put(fs: &dyn Fs, path: &Path, text: &str) {
    fs.create_dir_all(path.parent().unwrap(), Mode::DIR)
        .unwrap();
    fs.write_atomic(path, text.as_bytes(), Perms::Exact(Mode::FILE))
        .unwrap();
}

#[test]
fn test_a_linked_worktree_names_its_primary_checkout() {
    let repo = git::Repo::new();
    let linked = repo.worktree("linked");
    assert_eq!(
        primary_checkout(&real(), &linked).unwrap(),
        Some(repo.root())
    );
}

#[test]
fn test_a_worktree_inside_a_subdirectory_names_its_primary_checkout() {
    let repo = git::Repo::new();
    let elsewhere = git::temp();
    let linked = repo.worktree_at(&elsewhere.root().join("deep/linked"));
    assert_eq!(
        primary_checkout(&real(), &linked).unwrap(),
        Some(repo.root())
    );
}

#[test]
fn test_the_primary_checkout_has_none() {
    let repo = git::Repo::new();
    assert_eq!(primary_checkout(&real(), &repo.root()).unwrap(), None);
}

#[test]
fn test_a_bare_repositorys_worktree_has_none() {
    let repo = git::Repo::bare();
    let linked = repo.worktree("linked");
    assert_eq!(primary_checkout(&real(), &linked).unwrap(), None);
}

#[test]
fn test_a_git_file_with_no_commondir_has_none() {
    // A submodule's `.git` names a git directory under its superproject's
    // `.git/modules`, with no commondir.
    let (fs, root) = rooted();
    let modules = root.join("super/.git/modules/sub");
    fs.create_dir_all(&modules, Mode::DIR).unwrap();
    put(
        &fs,
        &root.join("sub/.git"),
        "gitdir: ../super/.git/modules/sub\n",
    );
    assert_eq!(primary_checkout(&fs, &root.join("sub")).unwrap(), None);
}

#[test]
fn test_a_git_file_that_is_not_a_gitdir_line_has_none() {
    let (fs, root) = rooted();
    put(
        &fs,
        &root.join("real/.git/worktrees/x/commondir"),
        "../..\n",
    );
    put(&fs, &root.join("x/.git"), "../real/.git/worktrees/x\n");
    assert_eq!(primary_checkout(&fs, &root.join("x")).unwrap(), None);
    put(
        &fs,
        &root.join("x/.git"),
        "gitdir: ../real/.git/worktrees/x\n",
    );
    assert_eq!(
        primary_checkout(&fs, &root.join("x")).unwrap(),
        Some(root.join("real")),
        "the same with its prefix"
    );
}

#[test]
fn test_a_worktree_whose_git_directory_is_gone_is_an_error() {
    let repo = git::Repo::new();
    let linked = repo.worktree("linked");
    real().remove_dir_all(&repo.root()).unwrap();
    let error = primary_checkout(&real(), &linked).unwrap_err();
    assert!(
        matches!(&error, crate::RegistryError::StaleWorktree { root, .. } if *root == linked.display().to_string()),
        "{error:?}"
    );
}

#[test]
fn test_an_unreadable_git_file_is_an_error() {
    let (fs, root) = rooted();
    put(&fs, &root.join(".git"), "gitdir: x\n");
    fs.set_permissions(&root.join(".git"), Mode::from_bits(0o000))
        .unwrap();
    let error = primary_checkout(&fs, &root);
    fs.set_permissions(&root.join(".git"), Mode::PRIVATE_FILE)
        .unwrap();
    assert!(
        matches!(error, Err(crate::RegistryError::Io { .. })),
        "{error:?}"
    );
}
