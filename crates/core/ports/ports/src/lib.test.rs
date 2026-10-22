//! The little the ports hold besides traits.

use crate::JsonlOp;

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
