//! The block's spelling: what a new block, a flow list and a key are
//! written as, each reading back as what it says.

use std::collections::{BTreeMap, BTreeSet};

use ports::{NamespacePermit, TagPermits};

use super::{block, flow, scalar, value};

fn keys(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

#[test]
fn test_a_new_block_is_block_style_sorted_two_spaces_a_level() {
    let permits = TagPermits {
        namespaces: BTreeMap::from([
            ("relates".to_string(), NamespacePermit::Open),
            (
                "area".to_string(),
                NamespacePermit::Closed(keys(&["mcp", "cli"])),
            ),
            ("none".to_string(), NamespacePermit::Closed(BTreeSet::new())),
        ]),
        flat: keys(&["urgent"]),
    };
    assert_eq!(
        block(&permits),
        "namespaces:\n  area:\n    - cli\n    - mcp\n  none: []\n  relates: open\ntags:\n  - urgent\n"
    );
    let flat_only = TagPermits {
        flat: keys(&["urgent"]),
        ..TagPermits::default()
    };
    assert_eq!(block(&flat_only), "tags:\n  - urgent\n");
    let namespaces_only = TagPermits {
        namespaces: BTreeMap::from([("sig".to_string(), NamespacePermit::Open)]),
        ..TagPermits::default()
    };
    assert_eq!(block(&namespaces_only), "namespaces:\n  sig: open\n");
    assert_eq!(block(&TagPermits::default()), "{}");
}

#[test]
fn test_an_edits_value_is_open_or_a_flow_list() {
    assert_eq!(value(&NamespacePermit::Open), "open");
    assert_eq!(value(&NamespacePermit::Closed(keys(&["b", "a"]))), "[a, b]");
    assert_eq!(flow(&keys(&[])), "[]");
}

// A key is written plain only where it reads back as the same string:
// anything a reader could take for a number, a boolean or null, or that
// YAML gives a meaning (a leading `-`, a `:`, a `#`), is single-quoted.
#[test]
fn test_a_key_is_plain_only_where_it_reads_back_as_itself() {
    for plain in ["cli", "blocks-release", "a.b", "_x", "x1", "Cli"] {
        assert_eq!(scalar(plain), plain);
    }
    for (key, quoted) in [
        ("1", "'1'"),
        ("true", "'true'"),
        ("No", "'No'"),
        ("null", "'null'"),
        ("-x", "'-x'"),
        ("a:b", "'a:b'"),
        ("a b", "'a b'"),
        ("a#b", "'a#b'"),
        ("it's", "'it''s'"),
        ("", "''"),
    ] {
        assert_eq!(scalar(key), quoted, "{key}");
    }
}
