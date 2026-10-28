//! The little the ports hold besides traits.

use annals_diagnostic::Report;

use crate::{Configuration, JsonlOp};

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
