//! The plan files over a temp-rooted filesystem: a work root at `/tree`
//! (`os::fixture`), plans under it and one outside it.

use std::sync::Arc;

use angzarr_diagnostic::{code, Diagnostic, Report};
use file_system::{Fs, Perms, Rooted};
use os::Mode;
use ports::PlanFiles;

use super::*;

fn tree() -> (Arc<dyn Fs>, FsPlanFiles) {
    let fs: Arc<dyn Fs> = Arc::new(Rooted::temp().unwrap());
    let files = FsPlanFiles::finding_root(Arc::clone(&fs), || Ok(os::fixture("/tree")));
    (fs, files)
}

fn put(fs: &dyn Fs, path: &str, text: &str) {
    let path = os::fixture(path);
    fs.create_dir_all(path.parent().unwrap(), Mode::DIR)
        .unwrap();
    fs.write_atomic(&path, text.as_bytes(), Perms::Exact(Mode::FILE))
        .unwrap();
}

fn at(path: &str) -> String {
    slashed(&os::fixture(path))
}

#[test]
fn the_work_root_is_the_found_one_slash_separated() {
    let (_fs, files) = tree();
    let root = files.work_root().unwrap();
    assert!(root.ends_with("/tree"), "{root}");
    assert!(!root.contains('\\'), "{root}");
}

#[test]
fn no_work_root_is_the_finders_report() {
    let fs: Arc<dyn Fs> = Arc::new(Rooted::temp().unwrap());
    let files = FsPlanFiles::finding_root(fs, || Err(Report::new("no-root-here")));
    assert_eq!(files.work_root().unwrap_err().code(), "no-root-here");
}

#[test]
fn read_gives_the_text_and_none_for_a_missing_file_or_one_that_is_not_utf8() {
    let (fs, files) = tree();
    put(&*fs, "/tree/docs/design.plan.md", "inside");
    put(&*fs, "/home/b/v1.plan.md", "outside");
    fs.write_atomic(
        &os::fixture("/tree/bin.plan.md"),
        &[0xff, 0xfe],
        Perms::Exact(Mode::FILE),
    )
    .unwrap();
    assert_eq!(
        files.read(&at("/tree/docs/design.plan.md")),
        Some("inside".to_string())
    );
    assert_eq!(
        files.read(&at("/home/b/v1.plan.md")),
        Some("outside".to_string())
    );
    assert_eq!(files.read(&at("/tree/docs/none.plan.md")), None);
    assert_eq!(files.read(&at("/tree/bin.plan.md")), None);
}

#[test]
fn create_makes_the_file_and_its_directories_and_never_overwrites() {
    let (fs, files) = tree();
    let path = at("/tree/.taskloom/plans/a.plan.md");
    assert_eq!(files.create(&path, "first"), Ok(true));
    assert_eq!(files.create(&path, "second"), Ok(false));
    assert_eq!(
        fs.read_to_string(&os::fixture("/tree/.taskloom/plans/a.plan.md"))
            .unwrap(),
        "first"
    );
}

#[test]
fn create_in_a_place_that_cannot_hold_it_is_the_create_error() {
    let (fs, files) = tree();
    put(&*fs, "/tree/blocker", "a file, not a directory");
    let error = files
        .create(&at("/tree/blocker/a.plan.md"), "x")
        .unwrap_err();
    assert_eq!(error.code(), code!("create"));
}

#[test]
fn replace_swaps_the_text_whole_and_a_missing_directory_is_the_replace_error() {
    let (fs, files) = tree();
    put(&*fs, "/tree/a.plan.md", "old");
    assert_eq!(files.replace(&at("/tree/a.plan.md"), "new"), Ok(()));
    assert_eq!(files.read(&at("/tree/a.plan.md")), Some("new".to_string()));
    let error = files
        .replace(&at("/tree/nowhere/a.plan.md"), "x")
        .unwrap_err();
    assert_eq!(error.code(), code!("replace"));
}

#[test]
fn scan_finds_plan_files_at_any_depth_sorted_and_skips_what_is_not_a_plan_or_is_vcs() {
    let (fs, files) = tree();
    for path in [
        "/tree/b.plan.md",
        "/tree/a.plan.md",
        "/tree/docs/deep/c.plan.md",
        "/tree/.taskloom/plans/d.plan.md",
        "/tree/notes.md",
        "/tree/x.plan.txt",
        "/tree/.git/e.plan.md",
        "/tree/node_modules/f.plan.md",
        "/tree/target/g.plan.md",
    ] {
        put(&*fs, path, "x");
    }
    assert_eq!(
        files.scan(&at("/tree")),
        [
            at("/tree/.taskloom/plans/d.plan.md"),
            at("/tree/a.plan.md"),
            at("/tree/b.plan.md"),
            at("/tree/docs/deep/c.plan.md"),
        ]
    );
    assert_eq!(files.scan(&at("/tree/nothing")), Vec::<String>::new());
}

#[test]
fn scan_does_not_follow_a_link_to_a_directory() {
    let (fs, files) = tree();
    put(&*fs, "/elsewhere/z.plan.md", "x");
    fs.create_dir_all(&os::fixture("/tree"), Mode::DIR).unwrap();
    if fs
        .symlink_dir(&os::fixture("/elsewhere"), &os::fixture("/tree/link"))
        .is_ok()
    {
        assert_eq!(files.scan(&at("/tree")), Vec::<String>::new());
    }
}

#[test]
fn every_error_has_a_catalog_message_with_exactly_its_args() {
    use fluent_syntax::ast::{Entry, Expression, InlineExpression, PatternElement};

    let resource = fluent_syntax::parser::parse(EN_US.1).expect("the catalog parses");
    let ids: Vec<&str> = resource
        .body
        .iter()
        .filter_map(|e| match e {
            Entry::Message(m) => Some(m.id.name),
            _ => None,
        })
        .collect();
    let samples = diagnostic_samples();
    assert_eq!(ids, samples.iter().map(|s| s.code()).collect::<Vec<_>>());
    for sample in samples {
        let Some(Entry::Message(message)) = resource
            .body
            .iter()
            .find(|e| matches!(e, Entry::Message(m) if m.id.name == sample.code()))
        else {
            panic!("no message for {}", sample.code());
        };
        let mut used: Vec<&str> = message
            .value
            .iter()
            .flat_map(|p| &p.elements)
            .filter_map(|el| match el {
                PatternElement::Placeable {
                    expression: Expression::Inline(InlineExpression::VariableReference { id }),
                } => Some(id.name),
                _ => None,
            })
            .collect();
        used.sort_unstable();
        let mut args: Vec<&str> = sample.args().iter().map(|(n, _)| *n).collect();
        args.sort_unstable();
        assert_eq!(used, args, "{}", sample.code());
        assert_eq!(sample.to_string(), Report::of(&*sample).to_string());
    }
}

/// The catalog is exported under the crate's package name, and every code
/// is under that name, with its message using exactly its args.
#[test]
fn test_every_code_is_under_the_crates_name_with_its_message() {
    let (name, text) = crate::EN_US;
    assert_eq!(name, env!("CARGO_PKG_NAME"));
    assert_eq!(
        angzarr_diagnostic::catalog::check(name, text, &crate::diagnostic_samples()),
        []
    );
}
