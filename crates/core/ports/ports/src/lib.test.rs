//! The little the ports hold besides traits.

use angzarr_embedded_diagnostic::Report;

use crate::{Configuration, ConfigurationWriter, JsonlOp, Mint, PlanFiles, TagPermits, Tasks};

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

/// Plan files whose every answer is its own, to see a borrow pass each on.
struct Files;

impl PlanFiles for Files {
    fn work_root(&self) -> Result<String, Report> {
        Ok("/work".to_string())
    }
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        (path == "a.plan.md").then(|| b"# A".to_vec())
    }
    fn create(&self, path: &str, _text: &[u8]) -> Result<bool, Report> {
        Ok(path == "new.plan.md")
    }
    fn plan_files(&self, dir: &str) -> Vec<String> {
        vec![format!("{dir}/a.plan.md")]
    }
}

// The plans use cases take their ports by reference: a borrowed port is
// the port, answer for answer.
#[test]
fn test_borrowed_plan_files_answer_as_the_files_do() {
    let files = &Files;
    assert_eq!(PlanFiles::work_root(&files), Ok("/work".to_string()));
    assert_eq!(PlanFiles::read(&files, "a.plan.md"), Some(b"# A".to_vec()));
    assert_eq!(PlanFiles::read(&files, "b.plan.md"), None);
    assert_eq!(PlanFiles::create(&files, "new.plan.md", b""), Ok(true));
    assert_eq!(PlanFiles::create(&files, "a.plan.md", b""), Ok(false));
    assert_eq!(
        PlanFiles::plan_files(&files, "plans"),
        vec!["plans/a.plan.md".to_string()]
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

/// Tasks that keep no dead letters of their own.
struct NoTasks;

impl Tasks for NoTasks {
    async fn exists(&mut self, _harp_id: &str) -> Result<bool, Report> {
        Ok(false)
    }
    async fn tagged(&mut self, _key: &str) -> Result<Vec<(String, String)>, Report> {
        Ok(Vec::new())
    }
    async fn untag(&mut self, _harp_id: &str, _key: &str, _value: &str) -> Result<(), Report> {
        Ok(())
    }
}

#[test]
fn test_tasks_dead_letter_nothing_by_default() {
    assert_eq!(NoTasks.dead_letters(), Vec::<String>::new());
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
