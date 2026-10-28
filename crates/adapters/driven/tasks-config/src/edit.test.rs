//! The safety net under every edit (ruling 11): an edit is written only if
//! it reads back, with serde_norway, as the file before it with only its
//! tag_permits replaced by those meant.

use std::collections::{BTreeMap, BTreeSet};

use annals_diagnostic::Diagnostic;
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
        assert!(error.to_string().starts_with(PATH), "{why}: {error}");
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
