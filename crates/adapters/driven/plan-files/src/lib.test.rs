//! The plan files over a temp-rooted filesystem: a work root at `/tree`
//! (`os::fixture`), plans under it and one outside it.

use std::sync::Arc;

use angzarr_embedded_diagnostic::{Arg, Diagnostic, Report};
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
    assert_eq!(files.read("a.plan.md"), None);
    assert_eq!(
        files.create("a.plan.md", b"x").unwrap_err().code(),
        "no-root-here"
    );
    assert!(files.plan_files(".taskloom/plans").is_empty());
}

#[test]
fn read_finds_a_relative_path_under_the_root_and_an_absolute_one_as_given() {
    let (fs, files) = tree();
    put(&*fs, "/tree/docs/design.plan.md", "inside");
    put(&*fs, "/home/b/v1.plan.md", "outside");
    assert_eq!(files.read("docs/design.plan.md"), Some(b"inside".to_vec()));
    let outside = os::fixture("/home/b/v1.plan.md");
    assert_eq!(
        files.read(&outside.to_string_lossy()),
        Some(b"outside".to_vec())
    );
    assert_eq!(files.read("docs/none.plan.md"), None);
}

#[test]
fn create_makes_the_file_and_its_directories_and_never_overwrites() {
    let (fs, files) = tree();
    assert_eq!(
        files.create(".taskloom/plans/a.plan.md", b"first"),
        Ok(true)
    );
    assert_eq!(
        files.create(".taskloom/plans/a.plan.md", b"second"),
        Ok(false)
    );
    let written = fs
        .read(&os::fixture("/tree/.taskloom/plans/a.plan.md"))
        .unwrap();
    assert_eq!(written, b"first");
}

#[test]
fn a_create_that_fails_says_where_and_why() {
    let (fs, files) = tree();
    // A file where its directory must go.
    put(&*fs, "/tree/.taskloom/plans", "not a directory");
    let refused = files.create(".taskloom/plans/a.plan.md", b"x").unwrap_err();
    assert_eq!(refused.code(), "plan-files-create");
    assert!(
        matches!(&refused.args()[0], ("path", Arg::Str(p)) if p == ".taskloom/plans/a.plan.md"),
        "{refused}"
    );
}

#[test]
fn plan_files_lists_the_plan_files_directly_under_a_directory_sorted() {
    let (fs, files) = tree();
    put(&*fs, "/tree/.taskloom/plans/b.plan.md", "b");
    put(&*fs, "/tree/.taskloom/plans/a.plan.md", "a");
    put(&*fs, "/tree/.taskloom/plans/notes.md", "n");
    put(&*fs, "/tree/.taskloom/plans/deeper/c.plan.md", "c");
    assert_eq!(
        files.plan_files(".taskloom/plans"),
        vec![".taskloom/plans/a.plan.md", ".taskloom/plans/b.plan.md"]
    );
    assert!(files.plan_files("docs").is_empty());
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
