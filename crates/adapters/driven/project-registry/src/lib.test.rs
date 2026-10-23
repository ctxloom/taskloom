//! Resolution over a filesystem of the test's own (`Rooted::temp`): a home
//! and a work tree; and, where git makes the repositories, over the real
//! one, under a temporary directory.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use annals_diagnostic::Diagnostic;
use file_system::{Fs, Mode, Os, Perms, Rooted};
use ports::{Names, ProjectIdentity};

use super::{index, Environment, ProjectRegistry, MINT_ATTEMPTS};

/// Project ids in the order given, then `then` forever.
pub(crate) struct Scripted {
    ids: VecDeque<String>,
    then: String,
    minted: u32,
}

impl Names for Scripted {
    fn task_id(&mut self) -> String {
        unreachable!("no task ids here")
    }

    fn project_id(&mut self) -> String {
        self.minted += 1;
        self.ids.pop_front().unwrap_or_else(|| self.then.clone())
    }
}

pub(crate) fn names(ids: &[&str]) -> Scripted {
    Scripted {
        ids: ids.iter().map(|s| s.to_string()).collect(),
        then: "always-the-same".to_string(),
        minted: 0,
    }
}

/// A home and a work tree, apart, on a filesystem.
pub(crate) struct World {
    pub(crate) fs: Arc<dyn Fs>,
    home: PathBuf,
    tree: PathBuf,
    /// The temporary directory a real world is in, removed with it.
    _temp: Option<Rooted>,
}

impl World {
    /// A world on a filesystem of its own: `/home/u` and `/tree`.
    pub(crate) fn new() -> World {
        let fs: Arc<dyn Fs> = Arc::new(Rooted::temp().unwrap());
        World::made(fs, Path::new("/"), None)
    }

    /// A world on the real filesystem, under a temporary directory: for
    /// git's repositories, whose files name real paths.
    pub(crate) fn real() -> World {
        let temp = Rooted::temp().unwrap();
        let top = temp.root().to_path_buf();
        World::made(Arc::new(Os::new(&top).unwrap()), &top, Some(temp))
    }

    fn made(fs: Arc<dyn Fs>, top: &Path, temp: Option<Rooted>) -> World {
        let (home, tree) = (top.join("home/u"), top.join("tree"));
        for dir in [&home, &tree] {
            fs.create_dir_all(dir, Mode::DIR).unwrap();
        }
        World {
            fs,
            home,
            tree,
            _temp: temp,
        }
    }

    /// The home.
    pub(crate) fn home(&self) -> PathBuf {
        self.home.clone()
    }

    /// The work tree, canonical.
    pub(crate) fn tree(&self) -> PathBuf {
        self.tree.clone()
    }

    pub(crate) fn fs(&self) -> Arc<dyn Fs> {
        Arc::clone(&self.fs)
    }

    pub(crate) fn env(&self) -> Environment {
        Environment {
            home: Some(self.home()),
            cwd: Ok(self.tree()),
            root: None,
            project_id: None,
        }
    }

    pub(crate) fn registry(&self, ids: &[&str]) -> ProjectRegistry<Scripted> {
        ProjectRegistry::new(self.fs(), self.env(), names(ids))
    }

    pub(crate) fn index(&self) -> index::Index {
        index::load(&*self.fs, &index::path(&self.home())).unwrap()
    }

    pub(crate) fn save(&self, index: &index::Index) {
        index::save(&*self.fs, &index::path(&self.home()), index).unwrap();
    }

    pub(crate) fn marker(&self) -> Option<String> {
        self.fs
            .read_to_string(&self.tree().join(".project-id"))
            .ok()
    }

    pub(crate) fn store(&self, id: &str) -> PathBuf {
        self.home()
            .join(".ctxloom/projects")
            .join(id)
            .join("state.db")
    }

    /// Write `text` at `path`, its directory made.
    pub(crate) fn write(&self, path: &Path, text: &str) {
        self.mkdir(path.parent().unwrap());
        self.fs
            .write_atomic(path, text.as_bytes(), Perms::Exact(Mode::FILE))
            .unwrap();
    }

    pub(crate) fn read(&self, path: &Path) -> String {
        self.fs.read_to_string(path).unwrap()
    }

    pub(crate) fn mkdir(&self, path: &Path) {
        self.fs.create_dir_all(path, Mode::DIR).unwrap();
    }

    pub(crate) fn link(&self, target: &Path, link: &Path) {
        self.fs.symlink_dir(target, link).unwrap();
    }

    pub(crate) fn exists(&self, path: &Path) -> bool {
        self.fs.exists(path).unwrap()
    }

    pub(crate) fn chmod(&self, path: &Path, bits: u32) {
        self.fs
            .set_permissions(path, Mode::from_bits(bits))
            .unwrap();
    }
}

pub(crate) fn text(path: &Path) -> String {
    path.display().to_string()
}

// ------------------------------------------------------------ work root

#[test]
fn test_the_work_root_is_ctxloom_root_when_set() {
    let world = World::new();
    let mut env = world.env();
    env.root = Some(PathBuf::from("/somewhere/else"));
    let registry = ProjectRegistry::new(world.fs(), env, names(&[]));
    assert_eq!(registry.work_root().unwrap(), Path::new("/somewhere/else"));
}

#[test]
fn test_the_work_root_is_the_nearest_ancestor_holding_git() {
    let world = World::new();
    world.mkdir(&world.tree().join(".git"));
    let deep = world.tree().join("a/b");
    world.mkdir(&deep);
    world.mkdir(&world.tree().join("a/.git"));
    let mut env = world.env();
    env.cwd = Ok(deep);
    env.root = Some(PathBuf::new());
    let registry = ProjectRegistry::new(world.fs(), env, names(&[]));
    assert_eq!(
        registry.work_root().unwrap(),
        world.tree().join("a"),
        "an empty CTXLOOM_ROOT is unset"
    );
}

#[test]
fn test_a_linked_worktrees_git_file_marks_its_root() {
    let world = World::new();
    world.write(&world.tree().join(".git"), "gitdir: /elsewhere\n");
    let mut env = world.env();
    env.cwd = Ok(world.tree().join("sub"));
    let registry = ProjectRegistry::new(world.fs(), env, names(&[]));
    assert_eq!(registry.work_root().unwrap(), world.tree());
}

#[test]
fn test_with_no_git_above_the_work_root_is_the_working_directory() {
    let world = World::new();
    assert_eq!(world.registry(&[]).work_root().unwrap(), world.tree());
}

#[test]
fn test_the_work_root_is_canonical_symlinks_resolved_and_slashes_cleaned() {
    let world = World::new();
    let link = world.home().join("link-to-tree");
    world.link(&world.tree(), &link);
    for root in [link.clone(), link.join(""), link.join(".")] {
        let mut env = world.env();
        env.root = Some(root.clone());
        let registry = ProjectRegistry::new(world.fs(), env, names(&[]));
        assert_eq!(registry.work_root().unwrap(), world.tree(), "{root:?}");
    }
}

#[test]
fn test_a_work_root_that_does_not_resolve_is_cleaned_lexically() {
    let world = World::new();
    let mut env = world.env();
    env.root = Some(PathBuf::from("/no/such/./tree/../root/"));
    let registry = ProjectRegistry::new(world.fs(), env, names(&[]));
    assert_eq!(registry.work_root().unwrap(), Path::new("/no/such/root"));
}

#[test]
fn test_a_relative_ctxloom_root_is_under_the_working_directory() {
    let world = World::new();
    world.mkdir(&world.tree().join("sub"));
    let mut env = world.env();
    env.root = Some(PathBuf::from("sub/"));
    let registry = ProjectRegistry::new(world.fs(), env, names(&[]));
    assert_eq!(registry.work_root().unwrap(), world.tree().join("sub"));
}

#[test]
fn test_a_working_directory_that_cannot_be_read_is_an_error() {
    let world = World::new();
    let mut env = world.env();
    env.cwd = Err("No such file or directory (os error 2)".to_string());
    let mut registry = ProjectRegistry::new(world.fs(), env.clone(), names(&["fresh-new-id"]));
    let error = registry.for_write(None).unwrap_err();
    assert_eq!(error.code(), "project-registry-no-cwd");
    assert!(error.to_string().contains("os error 2"), "{error}");
    assert_eq!(registry.names.minted, 0);
    env.root = Some(world.tree());
    let registry = ProjectRegistry::new(world.fs(), env, names(&[]));
    assert_eq!(
        registry.work_root().unwrap(),
        world.tree(),
        "CTXLOOM_ROOT needs no working directory"
    );
}

// ------------------------------------------------------------ pinned ids

#[test]
fn test_a_pinned_id_wins_over_the_environment_and_the_marker() {
    let world = World::new();
    world.write(&world.tree().join(".project-id"), "from-marker\n");
    let mut env = world.env();
    env.project_id = Some("from-env".to_string());
    let mut registry = ProjectRegistry::new(world.fs(), env, names(&[]));
    let project = registry.for_write(Some("from-flag")).unwrap();
    assert_eq!(project.id, "from-flag");
    assert_eq!(project.root, text(&world.tree()));
    assert_eq!(project.store, world.store("from-flag"));
    let read = registry.for_read(Some("from-flag")).unwrap().unwrap();
    assert_eq!(read, project);
}

#[test]
fn test_ctxloom_project_id_wins_over_the_marker() {
    let world = World::new();
    world.write(&world.tree().join(".project-id"), "from-marker\n");
    let mut env = world.env();
    env.project_id = Some("from-env".to_string());
    let mut registry = ProjectRegistry::new(world.fs(), env, names(&[]));
    assert_eq!(registry.for_read(None).unwrap().unwrap().id, "from-env");
    assert_eq!(registry.for_read(Some("")).unwrap().unwrap().id, "from-env");
}

#[test]
fn test_an_empty_ctxloom_project_id_is_unset() {
    let world = World::new();
    let mut env = world.env();
    env.project_id = Some(String::new());
    let mut registry = ProjectRegistry::new(world.fs(), env, names(&[]));
    assert_eq!(registry.for_read(None).unwrap(), None);
}

#[test]
fn test_an_invalid_pinned_id_is_refused_naming_where_it_came_from() {
    let world = World::new();
    let error = world
        .registry(&[])
        .for_write(Some("../escape"))
        .unwrap_err();
    assert_eq!(error.code(), "project-registry-invalid-id");
    assert!(error.to_string().contains("--project"), "{error}");
    let mut env = world.env();
    env.project_id = Some("a/b".to_string());
    let error = ProjectRegistry::new(world.fs(), env, names(&[]))
        .for_read(None)
        .unwrap_err();
    assert!(error.to_string().contains("CTXLOOM_PROJECT_ID"), "{error}");
}

// ------------------------------------------------------------ markers

#[test]
fn test_the_marker_names_the_project() {
    let world = World::new();
    world.write(&world.tree().join(".project-id"), "  jolly-mute-magma \n");
    let project = world.registry(&[]).for_read(None).unwrap().unwrap();
    assert_eq!(project.id, "jolly-mute-magma");
    assert_eq!(project.store, world.store("jolly-mute-magma"));
}

#[test]
fn test_a_blank_marker_names_nothing() {
    let world = World::new();
    world.write(&world.tree().join(".project-id"), "\n");
    assert_eq!(world.registry(&[]).for_read(None).unwrap(), None);
}

#[test]
fn test_an_invalid_marker_is_refused() {
    let world = World::new();
    world.write(&world.tree().join(".project-id"), "../../etc\n");
    let error = world.registry(&[]).for_read(None).unwrap_err();
    assert_eq!(error.code(), "project-registry-invalid-id");
}

#[test]
fn test_an_unreadable_marker_fails() {
    let world = World::new();
    world.mkdir(&world.tree().join(".project-id"));
    let error = world.registry(&[]).for_read(None).unwrap_err();
    assert_eq!(error.code(), "project-registry-io");
}

#[test]
fn test_gos_older_marker_is_read_and_adopted_only_by_a_write() {
    let world = World::new();
    world.mkdir(&world.tree().join(".ctxloom"));
    world.write(&world.tree().join(".ctxloom/project-id"), "old-go-id\n");
    let mut registry = world.registry(&[]);
    assert_eq!(registry.for_read(None).unwrap().unwrap().id, "old-go-id");
    assert_eq!(world.marker(), None, "a read writes nothing");
    assert_eq!(registry.for_write(None).unwrap().id, "old-go-id");
    assert_eq!(world.marker().as_deref(), Some("old-go-id\n"));
    assert!(
        world.index().projects.is_empty(),
        "adopting registers nothing yet"
    );
}

// ------------------------------------------------------------ registry

#[test]
fn test_the_registry_names_the_project_of_a_registered_root() {
    let world = World::new();
    let mut index = index::Index::default();
    index.register("high-tight-gulf", &text(&world.tree()));
    world.save(&index);
    let mut registry = world.registry(&[]);
    assert_eq!(
        registry.for_read(None).unwrap().unwrap().id,
        "high-tight-gulf"
    );
    assert_eq!(registry.for_write(None).unwrap().id, "high-tight-gulf");
    assert_eq!(registry.names.minted, 0);
}

#[test]
fn test_a_symlinked_or_slashed_ctxloom_root_finds_gos_entry() {
    // The path registered is the canonical one: symlinks resolved.
    let world = World::new();
    let mut index = index::Index::default();
    index.register("high-tight-gulf", &text(&world.tree()));
    world.save(&index);
    let link = world.home().join("link-to-tree");
    world.link(&world.tree(), &link);
    for root in [link.clone(), link.join(""), world.tree().join("")] {
        let mut env = world.env();
        env.root = Some(root.clone());
        let mut registry = ProjectRegistry::new(world.fs(), env, names(&[]));
        let project = registry.for_read(None).unwrap().expect("Go's entry");
        assert_eq!(project.id, "high-tight-gulf", "{root:?}");
        assert_eq!(project.root, text(&world.tree()));
    }
}

#[test]
fn test_an_entry_registered_through_a_symlink_matches_its_target() {
    let world = World::new();
    let link = world.home().join("link-to-tree");
    world.link(&world.tree(), &link);
    let mut index = index::Index::default();
    index.register("high-tight-gulf", &format!("{}/", link.display()));
    world.save(&index);
    let project = world.registry(&[]).for_read(None).unwrap().expect("found");
    assert_eq!(project.id, "high-tight-gulf");
}

#[test]
fn test_a_mint_registers_the_canonical_path() {
    let world = World::new();
    let link = world.home().join("link-to-tree");
    world.link(&world.tree(), &link);
    let mut env = world.env();
    env.root = Some(link);
    ProjectRegistry::new(world.fs(), env, names(&["fresh-new-id"]))
        .for_write(None)
        .unwrap();
    assert_eq!(world.index().projects[0].path, text(&world.tree()));
}

#[test]
fn test_an_unreadable_registry_fails_a_read() {
    let world = World::new();
    let path = index::path(&world.home());
    world.write(&path, "projects: [");
    let error = world.registry(&[]).for_read(None).unwrap_err();
    assert_eq!(error.code(), "project-registry-unreadable");
}

// ------------------------------------------------------------ minting

#[test]
fn test_a_read_with_no_project_finds_none_and_writes_nothing() {
    let world = World::new();
    assert_eq!(
        world.registry(&["fresh-new-id"]).for_read(None).unwrap(),
        None
    );
    assert_eq!(world.marker(), None);
    assert!(!world.exists(&index::path(&world.home())));
}

#[test]
fn test_a_write_with_no_project_mints_and_binds_one() {
    let world = World::new();
    let mut registry = world.registry(&["fresh-new-id"]);
    let project = registry.for_write(None).unwrap();
    assert_eq!(project.id, "fresh-new-id");
    assert_eq!(project.root, text(&world.tree()));
    assert_eq!(project.store, world.store("fresh-new-id"));
    assert_eq!(world.marker().as_deref(), Some("fresh-new-id\n"));
    let index = world.index();
    assert_eq!(index.schema_version, 1);
    assert_eq!(
        index.id_for(&*world.fs, &world.tree()),
        Some("fresh-new-id")
    );
    assert_eq!(
        registry.for_write(None).unwrap(),
        project,
        "bound: not minted again"
    );
    assert_eq!(registry.names.minted, 1);
}

#[test]
fn test_a_mint_skips_ids_already_registered() {
    let world = World::new();
    let mut index = index::Index::default();
    index.register("taken-id", "/another/tree");
    world.save(&index);
    let project = world
        .registry(&["taken-id", "free-id"])
        .for_write(None)
        .unwrap();
    assert_eq!(project.id, "free-id");
    assert_eq!(world.index().projects.len(), 2);
}

#[test]
fn test_a_mint_gives_up_when_every_id_is_taken() {
    let world = World::new();
    let mut index = index::Index::default();
    index.register("always-the-same", "/another/tree");
    world.save(&index);
    let mut registry = world.registry(&[]);
    let error = registry.for_write(None).unwrap_err();
    assert_eq!(error.code(), "project-registry-exhausted");
    assert_eq!(registry.names.minted, MINT_ATTEMPTS);
}

#[test]
fn test_binding_under_the_lock_takes_the_id_another_process_bound_first() {
    // The marker check before the lock found nothing; by the time the lock
    // is held another process has bound the root. `bind` re-checks.
    let world = World::new();
    let mut registry = world.registry(&["never-minted"]);
    world.write(&world.tree().join(".project-id"), "won-the-race\n");
    assert_eq!(registry.bind(&world.tree()).unwrap(), "won-the-race");
    assert_eq!(registry.names.minted, 0);
}

#[test]
fn test_a_custom_lock_wait_is_the_one_a_write_waits_for() {
    // The lock is held here, through the registry's own lock file, so the
    // write must wait: it gives up after its short wait, long before the
    // default would have.
    let world = World::new();
    let held =
        index::lock_within(&*world.fs, &index::path(&world.home()), index::LOCK_WAIT).unwrap();
    let wait = std::time::Duration::from_millis(50);
    let mut registry = world.registry(&["never-minted"]).with_lock_wait(wait);
    let started = std::time::Instant::now();
    let error = registry.for_write(None).unwrap_err();
    let waited = started.elapsed();
    assert_eq!(error.code(), "project-registry-lock-timeout");
    assert!(waited >= wait, "it waited its wait: {waited:?}");
    assert!(
        waited < index::LOCK_WAIT / 2,
        "it gave up long before the default wait: {waited:?}"
    );
    assert_eq!(registry.names.minted, 0);
    drop(held);
}

#[test]
fn test_without_a_home_there_is_no_store_and_no_registry() {
    let world = World::new();
    for home in [None, Some(PathBuf::new())] {
        let mut env = world.env();
        env.home = home;
        let error = ProjectRegistry::new(world.fs(), env, names(&[]))
            .for_write(Some("pinned"))
            .unwrap_err();
        assert_eq!(error.code(), "project-registry-no-home");
    }
}

// ------------------------------------------------------------ linked worktrees

/// Repositories and worktrees made by git itself, in temporary
/// directories, with no configuration but their own. git writes real
/// paths into what it makes, so these are on the real filesystem; a
/// `Rooted` is only what owns (and removes) each temporary directory.
pub(crate) mod git {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use file_system::Rooted;

    pub struct Repo {
        dir: Rooted,
        bare: bool,
    }

    fn run(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A new temporary directory, its real path canonical.
    pub fn temp() -> Rooted {
        Rooted::temp().unwrap()
    }

    impl Repo {
        /// A repository with one commit, checked out.
        pub fn new() -> Repo {
            let repo = Repo {
                dir: temp(),
                bare: false,
            };
            run(repo.dir.root(), &["init", "-q", "main"]);
            run(
                &repo.root(),
                &["commit", "-q", "--allow-empty", "-m", "first"],
            );
            repo
        }

        /// A bare repository with one commit.
        pub fn bare() -> Repo {
            let source = Repo::new();
            let repo = Repo {
                dir: temp(),
                bare: true,
            };
            let root = repo.root();
            run(
                source.dir.root(),
                &[
                    "clone",
                    "-q",
                    "--bare",
                    &source.root().display().to_string(),
                    &root.display().to_string(),
                ],
            );
            repo
        }

        /// The checkout (or the bare repository), canonical.
        pub fn root(&self) -> PathBuf {
            self.dir.root().join("main")
        }

        /// A linked worktree named `name` beside the checkout, canonical.
        pub fn worktree(&self, name: &str) -> PathBuf {
            self.worktree_at(&self.dir.root().join(name))
        }

        /// A linked worktree at `at`, a canonical path.
        pub fn worktree_at(&self, at: &Path) -> PathBuf {
            let branch = at.file_name().unwrap().to_string_lossy().to_string();
            let mut args = vec!["worktree", "add", "-q", "-b", &branch];
            let shown = at.display().to_string();
            args.push(&shown);
            if self.bare {
                args.push("main");
            }
            run(&self.root(), &args);
            at.to_path_buf()
        }
    }
}

impl World {
    /// The world, working in `cwd`.
    pub(crate) fn env_in(&self, cwd: &Path) -> Environment {
        Environment {
            cwd: Ok(cwd.to_path_buf()),
            ..self.env()
        }
    }
}

#[test]
fn test_a_linked_worktree_reads_its_primary_checkouts_project() {
    let world = World::real();
    let repo = git::Repo::new();
    let linked = repo.worktree("linked");
    world.write(&repo.root().join(".project-id"), "jolly-mute-magma\n");
    let mut registry =
        ProjectRegistry::new(world.fs(), world.env_in(&linked.join(".")), names(&[]));
    let project = registry.for_read(None).unwrap().unwrap();
    assert_eq!(project.id, "jolly-mute-magma");
    assert_eq!(
        project.root,
        text(&repo.root()),
        "the project's root is the primary"
    );
    assert_eq!(
        registry.work_root().unwrap(),
        linked,
        "the work root is the worktree"
    );
}

#[test]
fn test_a_linked_worktree_finds_its_primary_checkouts_registry_entry() {
    let world = World::real();
    let repo = git::Repo::new();
    let linked = repo.worktree("linked");
    let mut index = index::Index::default();
    index.register("high-tight-gulf", &text(&repo.root()));
    index.register("own-old-id", &text(&linked));
    world.save(&index);
    let project = ProjectRegistry::new(world.fs(), world.env_in(&linked), names(&[]))
        .for_read(None)
        .unwrap()
        .unwrap();
    assert_eq!(
        project.id, "high-tight-gulf",
        "the primary's, not its own entry's"
    );
}

#[test]
fn test_a_write_in_a_new_worktree_binds_the_primary_checkout_and_marks_only_it() {
    let world = World::real();
    let repo = git::Repo::new();
    let linked = repo.worktree("linked");
    let project = ProjectRegistry::new(world.fs(), world.env_in(&linked), names(&["fresh-new-id"]))
        .for_write(None)
        .unwrap();
    assert_eq!(project.id, "fresh-new-id");
    assert!(
        !world.exists(&linked.join(".project-id")),
        "no marker in the worktree, to be committed and merged back"
    );
    assert_eq!(
        world.read(&repo.root().join(".project-id")),
        "fresh-new-id\n"
    );
    assert_eq!(
        world.index().id_for(&*world.fs, &repo.root()),
        Some("fresh-new-id")
    );
    assert_eq!(world.index().id_for(&*world.fs, &linked), None);
    let primary = ProjectRegistry::new(world.fs(), world.env_in(&repo.root()), names(&[]))
        .for_read(None)
        .unwrap()
        .unwrap();
    assert_eq!(primary.id, "fresh-new-id", "one project for both");
}

#[test]
fn test_a_linked_worktree_with_its_own_marker_keeps_its_own_project() {
    let world = World::real();
    let repo = git::Repo::new();
    let linked = repo.worktree("linked");
    world.write(&repo.root().join(".project-id"), "jolly-mute-magma\n");
    world.write(&linked.join(".project-id"), "its-own-id\n");
    let project = ProjectRegistry::new(world.fs(), world.env_in(&linked), names(&[]))
        .for_read(None)
        .unwrap()
        .unwrap();
    assert_eq!(project.id, "its-own-id");
    assert_eq!(project.root, text(&linked));
}

#[test]
fn test_a_linked_worktree_with_gos_older_marker_keeps_its_own_project() {
    let world = World::real();
    let repo = git::Repo::new();
    let linked = repo.worktree("linked");
    world.write(&repo.root().join(".project-id"), "jolly-mute-magma\n");
    world.mkdir(&linked.join(".ctxloom"));
    world.write(&linked.join(".ctxloom/project-id"), "go-minted-id\n");
    let project = ProjectRegistry::new(world.fs(), world.env_in(&linked), names(&[]))
        .for_write(None)
        .unwrap();
    assert_eq!(project.id, "go-minted-id");
    assert_eq!(
        world.read(&linked.join(".project-id")),
        "go-minted-id\n",
        "adopted where it was"
    );
}

// ------------------------------------------------------------ every project

#[test]
fn test_every_project_is_each_database_directory_by_id_with_its_registered_root() {
    let world = World::new();
    let projects = world.home().join(".ctxloom/projects");
    for id in ["zesty-lone-fern", "brisk-tall-oak"] {
        world.mkdir(&projects.join(id));
    }
    world.write(&projects.join("not-a-dir"), "");
    world.mkdir(&projects.join("Not An Id"));
    let mut index = index::Index::default();
    let link = world.home().join("link-to-tree");
    world.link(&world.tree(), &link);
    index.register("zesty-lone-fern", &text(&link));
    index.register("zesty-lone-fern", "/elsewhere");
    world.save(&index);

    let every = world.registry(&[]).every_project().unwrap();
    let seen: Vec<(&str, &str)> = every
        .iter()
        .map(|p| (p.id.as_str(), p.root.as_str()))
        .collect();
    let tree = text(&world.tree());
    assert_eq!(
        seen,
        [("brisk-tall-oak", ""), ("zesty-lone-fern", tree.as_str())],
        "by id; the first registered path, canonical; none unregistered"
    );
    assert_eq!(every[1].store, world.store("zesty-lone-fern"));
}

#[test]
fn test_every_project_is_none_before_any_and_creates_nothing() {
    let world = World::new();
    assert_eq!(world.registry(&[]).every_project().unwrap(), []);
    assert!(!world.exists(&world.home().join(".ctxloom")));
}

#[test]
fn test_every_project_without_a_home_fails() {
    let world = World::new();
    let mut env = world.env();
    env.home = None;
    let error = ProjectRegistry::new(world.fs(), env, names(&[]))
        .every_project()
        .unwrap_err();
    assert_eq!(error.code(), "project-registry-no-home");
}

// ------------------------------------------------------------ rootless

#[test]
fn test_a_directory_in_no_git_repository_and_no_ctxloom_root_is_rootless() {
    let world = World::new();
    assert_eq!(
        world.registry(&[]).rootless_directory().unwrap(),
        Some(text(&world.tree()))
    );
}

#[test]
fn test_a_directory_in_a_git_repository_or_under_ctxloom_root_is_not_rootless() {
    let world = World::new();
    let mut env = world.env();
    env.root = Some(world.tree());
    let mut registry = ProjectRegistry::new(world.fs(), env, names(&[]));
    assert_eq!(registry.rootless_directory().unwrap(), None);
    world.mkdir(&world.tree().join(".git"));
    let deep = world.tree().join("a");
    world.mkdir(&deep);
    let mut env = world.env();
    env.cwd = Ok(deep);
    let mut registry = ProjectRegistry::new(world.fs(), env, names(&[]));
    assert_eq!(registry.rootless_directory().unwrap(), None);
}

#[test]
fn test_rootlessness_with_no_working_directory_fails() {
    let world = World::new();
    let mut env = world.env();
    env.cwd = Err("gone".to_string());
    let error = ProjectRegistry::new(world.fs(), env, names(&[]))
        .rootless_directory()
        .unwrap_err();
    assert_eq!(error.code(), "project-registry-no-cwd");
}
