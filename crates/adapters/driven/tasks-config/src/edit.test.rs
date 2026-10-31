//! The safety net under every edit (ruling 11): an edit is written only if
//! it reads back, with serde_norway, as the file before it with only its
//! tag_permits replaced by those meant.

use std::collections::{BTreeMap, BTreeSet};

crate::english_for!(&[crate::EN_US.1]);

use angzarr_embedded_diagnostic::Diagnostic;
use ports::{NamespacePermit, TagPermits};

use super::verified;

const PATH: &str = "/work/tree/.taskloom/config.yaml";

const BEFORE: &str = "# Ours.\nschema_version: 1\ntag_permits:\n  namespaces:\n    area:\n      - cli  # the command line\n  tags:\n    - urgent\ntag_schema:\n  - 'tagma.arity:\"area:x\"=scalar'\n";

/// `area:` closed to `keys`, and the flat `urgent`.
fn meant(keys: &[&str]) -> TagPermits {
    TagPermits {
        namespaces: BTreeMap::from([(
            "area".to_string(),
            NamespacePermit::Closed(keys.iter().map(|k| k.to_string()).collect()),
        )]),
        flat: BTreeSet::from(["urgent".to_string()]),
    }
}

// An edit that reads back as meant passes, its comments and layout
// whatever they are (they are not read); one adding a key other than the
// one meant, leaving the block as it was, changing or dropping a key
// outside the block, or not parsing, is refused, naming the file. Each
// refusal is the same diagnostic: the write it stops is a bug.
#[test]
fn test_an_edit_is_refused_unless_it_reads_back_as_meant() {
    let faithful = BEFORE.replace(
        "      - cli  # the command line\n",
        "      - cli\n      - mcp  # added\n",
    );
    assert_eq!(
        verified(PATH, BEFORE, &faithful, &meant(&["cli", "mcp"])),
        Ok(())
    );

    let refused = [
        (
            "the wrong key added",
            BEFORE.replace("      - cli  #", "      - cli\n      - docs  #"),
        ),
        ("the block left as it was", BEFORE.to_string()),
        (
            "another key changed",
            faithful.replace("schema_version: 1", "schema_version: 2"),
        ),
        (
            "another key dropped",
            faithful.replace("tag_schema:\n  - 'tagma.arity:\"area:x\"=scalar'\n", ""),
        ),
        (
            "a list outside the block changed",
            faithful.replace(
                "tag_schema:\n  - 'tagma.arity:\"area:x\"=scalar'\n",
                "tag_schema:\n  - b\n  - a\n",
            ),
        ),
        ("not YAML", format!("{faithful}  - [unclosed\n")),
    ];
    for (why, after) in refused {
        let error = verified(PATH, BEFORE, &after, &meant(&["cli", "mcp"])).unwrap_err();
        assert_eq!(error.code(), "tasks-config-edit-refused", "{why}");
        assert!(error.english().starts_with(PATH), "{why}: {error}");
    }
}

// The comparison is of what the file says, so a file whose other keys
// were in a different order, or quoted differently, before the edit is
// not refused for it: the edit does not touch them.
#[test]
fn test_an_edit_is_judged_by_what_the_file_says_not_how() {
    let before = "tag_schema: [\"a\"]\ntag_permits:\n  tags: [urgent]\nschema_version: 1\n";
    let after = "tag_schema: [\"a\"]\ntag_permits:\n  tags: [urgent]\n  namespaces:\n    area: [cli, mcp]\nschema_version: 1\n";
    assert_eq!(
        verified(PATH, before, after, &meant(&["cli", "mcp"])),
        Ok(())
    );
    let refused = after.replace("[cli, mcp]", "[cli]");
    assert!(verified(PATH, before, &refused, &meant(&["cli", "mcp"])).is_err());
}

// ------------------------------------------------------------ edited

fn open(names: &[&str]) -> Vec<(String, NamespacePermit)> {
    names
        .iter()
        .map(|n| (n.to_string(), NamespacePermit::Open))
        .collect()
}

fn closed(name: &str, keys: &[&str]) -> (String, NamespacePermit) {
    (
        name.to_string(),
        NamespacePermit::Closed(keys.iter().map(|k| k.to_string()).collect()),
    )
}

fn of(namespaces: Vec<(String, NamespacePermit)>, flat: &[&str]) -> TagPermits {
    TagPermits {
        namespaces: namespaces.into_iter().collect(),
        flat: flat.iter().map(|k| k.to_string()).collect(),
    }
}

fn edit(before: &str, permits: &TagPermits) -> String {
    super::edited(PATH, before, permits).unwrap()
}

// yaml-rt-core takes the blank lines after a block list's last item with
// it; they are put back, whether the list keeps items or is left empty.
#[test]
fn test_removing_a_lists_last_item_keeps_the_blank_lines_after_it() {
    let before = "tag_permits:\n  namespaces:\n    area:\n      - cli\n      - mcp  # m\n\n    queue: open\n  tags:\n    - urgent\n\n\n# The schema.\nschema_version: 1\n";
    let after = edit(
        before,
        &of(
            vec![closed("area", &["cli"]), open(&["queue"]).remove(0)],
            &[],
        ),
    );
    assert_eq!(
        after,
        "tag_permits:\n  namespaces:\n    area:\n      - cli\n\n    queue: open\n  tags: []\n\n\n# The schema.\nschema_version: 1\n"
    );
}

// A namespace turning open or closed keeps the comment beside it, in
// either style; a block list turning open keeps the comment above it.
#[test]
fn test_a_namespace_turning_open_or_closed_keeps_its_comment() {
    let before = "tag_permits:\n  namespaces:\n    # Where.\n    area:\n      - cli  # c\n    queue: open  # q\n    sig: [a, b]  # s\n";
    let after = edit(
        before,
        &of(
            vec![
                open(&["area"]).remove(0),
                closed("queue", &["nightly", "weekly"]),
                open(&["sig"]).remove(0),
            ],
            &[],
        ),
    );
    assert_eq!(
        after,
        "tag_permits:\n  namespaces:\n    # Where.\n    area: open\n    queue: [nightly, weekly]  # q\n    sig: open  # s\n"
    );
}

// A quoted `open` has no spelling yaml-rt-core can lend, so the value is
// replaced whole: the edit still reads back as meant.
#[test]
fn test_a_quoted_open_is_replaced_whole() {
    let after = edit(
        "tag_permits:\n  namespaces:\n    area: 'open'\n",
        &of(vec![closed("area", &["cli"])], &[]),
    );
    assert_eq!(after, "tag_permits:\n  namespaces:\n    area: [cli]\n");
}

// Permits permitting nothing are `{}`, the block's comments with it; the
// rest of the file stays.
#[test]
fn test_permitting_nothing_is_an_empty_mapping() {
    let after = edit(
        "a: 1\ntag_permits:\n  # Ours.\n  tags:\n    - urgent\n\nb: 2\n",
        &TagPermits::default(),
    );
    assert_eq!(after, "a: 1\ntag_permits: {}\n\nb: 2\n");
}

// A block with nothing under it is no permits: written whole, at the end.
#[test]
fn test_a_block_with_nothing_under_it_is_written_whole() {
    let after = edit("tag_permits:\nhoming: home\n", &of(vec![], &["urgent"]));
    assert_eq!(after, "homing: home\ntag_permits:\n  tags:\n    - urgent\n");
}

// A list or `namespaces` the block lacks is added: a list in flow style,
// a namespaces of one open entry in flow style (a one-line block fragment
// does not parse), of more in block style.
#[test]
fn test_what_the_block_lacks_is_added() {
    let before = "tag_permits:\n  tags: [urgent]\n";
    assert_eq!(
        edit(before, &of(open(&["relates"]), &["urgent"])),
        "tag_permits:\n  tags: [urgent]\n  namespaces: {relates: open}\n"
    );
    assert_eq!(
        edit(
            before,
            &of(vec![closed("area", &["cli", "mcp"])], &["urgent"])
        ),
        "tag_permits:\n  tags: [urgent]\n  namespaces:\n    area:\n      - cli\n      - mcp\n"
    );
    let before = "tag_permits:\n  namespaces:\n    relates: open\n";
    assert_eq!(
        edit(before, &of(open(&["relates"]), &["blocked", "urgent"])),
        "tag_permits:\n  namespaces:\n    relates: open\n  tags: [blocked, urgent]\n"
    );
    assert_eq!(edit(before, &of(vec![], &[])), "tag_permits: {}\n");
}

// A file of comments alone, or ending without a line break, is followed
// by the block whole.
#[test]
fn test_a_file_of_comments_is_followed_by_the_block() {
    let permits = of(open(&["relates"]), &[]);
    assert_eq!(
        edit("# mine", &permits),
        "# mine\ntag_permits:\n  namespaces:\n    relates: open\n"
    );
    assert_eq!(edit("", &TagPermits::default()), "tag_permits: {}\n");
}

// Names are keys in a JSON Pointer, escaped as one: a `/` or `~` in a
// namespace still names it.
#[test]
fn test_a_namespace_named_with_a_pointers_escapes_is_edited() {
    let before = "tag_permits:\n  namespaces:\n    a/b: [x]\n    c~d: open\n";
    let after = edit(before, &of(vec![closed("a/b", &["x", "z"])], &[]));
    assert_eq!(after, "tag_permits:\n  namespaces:\n    a/b: [x, z]\n");
}

// Already as meant: the source back, unedited.
#[test]
fn test_permits_already_as_meant_give_the_source_back() {
    let before = "tag_permits:   # ours\n  tags: [urgent]\n";
    assert_eq!(edit(before, &of(vec![], &["urgent"])), before);
}

// A namespace the permits do not change is not touched, however it is
// spelled.
#[test]
fn test_a_namespace_left_as_it_was_keeps_its_spelling() {
    let before = "tag_permits:\n  namespaces:\n    area: 'open'\n    sig: [x]\n";
    let after = edit(
        before,
        &of(
            vec![open(&["area"]).remove(0), closed("sig", &["x", "z"])],
            &[],
        ),
    );
    assert_eq!(
        after,
        "tag_permits:\n  namespaces:\n    area: 'open'\n    sig: [x, z]\n"
    );
}

// ------------------------------------------------------------ retiring
//
// `taskloom schema import-config` takes the moved keys out of the work
// root's file once they are in the store: the rest of the file, its other
// keys, comments and layout, as written.

use super::retired;

#[test]
fn test_retiring_takes_out_the_moved_keys_and_keeps_the_rest() {
    let source = "schema_version: 1\n# kept\ntag_permits:\n  namespaces:\n    area: [cli]\n    relates: open\n  tags: [urgent]\n";
    assert_eq!(
        retired(PATH, source).unwrap(),
        Some("schema_version: 1\n# kept\n".to_string())
    );
    assert_eq!(
        retired(PATH, BEFORE).unwrap(),
        Some("# Ours.\nschema_version: 1\n".to_string())
    );
    let middle = "tag_schema:\n  - 'tagma.arity:\"a:x\"=scalar'\nhoming: home  # the repo\nschema_version: 1\n";
    assert_eq!(
        retired(PATH, middle).unwrap(),
        Some("homing: home  # the repo\nschema_version: 1\n".to_string())
    );
}

#[test]
fn test_a_file_holding_neither_key_is_left_alone() {
    for source in ["", "# nothing\n", "schema_version: 1\nhoming: home\n"] {
        assert_eq!(retired(PATH, source).unwrap(), None, "{source:?}");
    }
}
