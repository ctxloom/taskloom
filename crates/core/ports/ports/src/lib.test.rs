//! The little the ports hold besides traits.

use angzarr_diagnostic::Report;

use crate::{Configuration, ConfigurationWriter, JsonlOp, Mint, PlanFiles, TagPermits};

#[test]
fn test_a_jsonl_op_is_spelled_as_the_log_spells_it() {
    let ops = [
        JsonlOp::Add,
        JsonlOp::Status,
        JsonlOp::Text,
        JsonlOp::Remove,
        JsonlOp::Tag,
        JsonlOp::Untag,
    ];
    let spelled: Vec<&str> = ops.iter().map(|op| op.as_str()).collect();
    assert_eq!(spelled, ["add", "status", "text", "remove", "tag", "untag"]);
}

/// A configuration that knows nothing of tag permits: only a schema.
struct SchemaOnly;

impl Configuration for SchemaOnly {
    fn tag_schema(&mut self) -> Result<Vec<String>, Report> {
        Ok(Vec::new())
    }
}

// Tag permits are opt-in: a configuration that does not read them (a
// test's, ctxloom's) declares none, which is permissive, never an empty
// set, which would refuse every tag.
#[test]
fn test_a_configuration_knowing_nothing_of_permits_declares_none() {
    assert_eq!(SchemaOnly.tag_permits(), Ok(None));
}

// No plan_roots is the defaults, not an empty list of roots, which would
// find no plan.
#[test]
fn test_a_configuration_knowing_nothing_of_plan_roots_sets_none() {
    assert_eq!(SchemaOnly.plan_roots(), Ok(None));
}

/// Plan files whose every answer is its own, to see a borrow pass each on.
struct Files;

impl PlanFiles for Files {
    fn work_root(&self) -> Result<String, Report> {
        Ok("/work".to_string())
    }
    fn read(&self, path: &str) -> Option<String> {
        (path == "/work/a.plan.md").then(|| "# A".to_string())
    }
    fn replace(&self, path: &str, _text: &str) -> Result<(), Report> {
        match path == "/work/a.plan.md" {
            true => Ok(()),
            false => Err(Report::new("x-missing")),
        }
    }
    fn create(&self, path: &str, _text: &str) -> Result<bool, Report> {
        Ok(path == "/work/new.plan.md")
    }
    fn scan(&self, root: &str) -> Vec<String> {
        vec![format!("{root}/a.plan.md")]
    }
}

// The plans use cases take their ports by reference: a borrowed port is
// the port, answer for answer.
#[test]
fn test_borrowed_plan_files_answer_as_the_files_do() {
    let files = &Files;
    assert_eq!(PlanFiles::work_root(&files), Ok("/work".to_string()));
    assert_eq!(
        PlanFiles::read(&files, "/work/a.plan.md"),
        Some("# A".to_string())
    );
    assert_eq!(PlanFiles::read(&files, "/work/b.plan.md"), None);
    assert_eq!(PlanFiles::replace(&files, "/work/a.plan.md", ""), Ok(()));
    assert!(PlanFiles::replace(&files, "/work/b.plan.md", "").is_err());
    assert_eq!(PlanFiles::create(&files, "/work/new.plan.md", ""), Ok(true));
    assert_eq!(PlanFiles::create(&files, "/work/a.plan.md", ""), Ok(false));
    assert_eq!(
        PlanFiles::scan(&files, "/work/plans"),
        vec!["/work/plans/a.plan.md".to_string()]
    );
}

struct Words;

impl Mint for Words {
    fn mint(&self) -> String {
        "brisk-cedar-otter".to_string()
    }
}

#[test]
fn test_a_borrowed_mint_mints_as_the_mint_does() {
    assert_eq!(Mint::mint(&&Words), "brisk-cedar-otter");
}

/// A writer that knows nothing of the moved keys.
struct PermitsOnly;

impl ConfigurationWriter for PermitsOnly {
    fn write_tag_permits(&mut self, _permits: &TagPermits) -> Result<(), Report> {
        Ok(())
    }
}

// A configuration writer that knows nothing of tag_schema and tag_permits
// holds neither, so retiring them takes nothing out.
#[test]
fn test_a_writer_knowing_nothing_of_the_moved_keys_retires_none() {
    assert_eq!(PermitsOnly.retire_moved(), Ok(false));
}
