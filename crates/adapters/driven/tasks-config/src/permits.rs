//! The `tag_permits` block: its shape as serde_norway reads it, and its
//! spelling as a permit writes it.
//!
//! ```yaml
//! tag_permits:
//!   namespaces:
//!     area: [cli, mcp]   # closed: only these keys
//!     relates: open      # open: any key
//!   tags: [urgent]       # flat tags (no namespace)
//! ```
//!
//! Either list may be in block or flow style; both keys may be left out,
//! and `tag_permits: {}` permits nothing.

use std::collections::{BTreeMap, BTreeSet};

use ports::{NamespacePermit, TagPermits};
use serde::Deserialize;

use crate::ConfigError;

/// The config key the permits are under.
pub(crate) const KEY: &str = "tag_permits";

/// What an open namespace's value is.
pub(crate) const OPEN: &str = "open";

/// The block, as written.
#[derive(Deserialize, Clone, Default, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct Raw {
    #[serde(default)]
    namespaces: BTreeMap<String, RawNamespace>,
    #[serde(default)]
    tags: Vec<String>,
}

/// A namespace's value, as written: `open`, or its keys.
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
enum RawNamespace {
    Keys(Vec<String>),
    Word(String),
}

impl Raw {
    /// The permits the block written in the config file at `path` declares:
    /// refused when a namespace is neither `open` nor a list of keys, or
    /// when a name or key is blank (never dropped).
    pub(crate) fn permits(self, path: &str) -> Result<TagPermits, ConfigError> {
        let blank = || ConfigError::BlankItem {
            source: path.to_string(),
            value: KEY.to_string(),
        };
        let mut namespaces = BTreeMap::new();
        for (namespace, raw) in self.namespaces {
            if namespace.trim().is_empty() {
                return Err(blank());
            }
            let permit = match raw {
                RawNamespace::Word(word) if word == OPEN => NamespacePermit::Open,
                RawNamespace::Word(word) => {
                    return Err(ConfigError::Malformed {
                        path: path.to_string(),
                        detail: format!(
                            "{KEY}: namespaces: {namespace} is {word}, which is neither {OPEN} nor a list of keys"
                        ),
                    })
                }
                RawNamespace::Keys(keys) => NamespacePermit::Closed(keyed(keys).ok_or_else(blank)?),
            };
            namespaces.insert(namespace, permit);
        }
        Ok(TagPermits {
            namespaces,
            flat: keyed(self.tags).ok_or_else(blank)?,
        })
    }
}

/// `keys` as a set; `None` when one is blank.
fn keyed(keys: Vec<String>) -> Option<BTreeSet<String>> {
    (!keys.iter().any(|k| k.trim().is_empty())).then(|| keys.into_iter().collect())
}

/// What `tag_permits` maps to in a new block, in block style, two spaces
/// a level, ending in a line break: `{}` for permits permitting nothing.
pub(crate) fn block(permits: &TagPermits) -> String {
    if permits.namespaces.is_empty() && permits.flat.is_empty() {
        return "{}".to_string();
    }
    let mut out = String::new();
    if !permits.namespaces.is_empty() {
        out.push_str("namespaces:\n");
        out.push_str(&indented(&namespaces(&permits.namespaces)));
    }
    if !permits.flat.is_empty() {
        out.push_str("tags:\n");
        for key in &permits.flat {
            out.push_str(&format!("  - {}\n", scalar(key)));
        }
    }
    out
}

/// What `namespaces` maps to, in block style, each closed namespace's keys
/// a block list, ending in a line break.
pub(crate) fn namespaces(namespaces: &BTreeMap<String, NamespacePermit>) -> String {
    let mut out = String::new();
    for (namespace, permit) in namespaces {
        let name = scalar(namespace);
        match permit {
            NamespacePermit::Open => out.push_str(&format!("{name}: {OPEN}\n")),
            NamespacePermit::Closed(keys) if keys.is_empty() => {
                out.push_str(&format!("{name}: []\n"));
            }
            NamespacePermit::Closed(keys) => {
                out.push_str(&format!("{name}:\n"));
                for key in keys {
                    out.push_str(&format!("  - {}\n", scalar(key)));
                }
            }
        }
    }
    out
}

/// `text`'s lines, each two spaces further in.
pub(crate) fn indented(text: &str) -> String {
    text.lines().map(|line| format!("  {line}\n")).collect()
}

/// What a namespace's permit is written as in an edit: `open`, or its keys
/// as a flow list.
pub(crate) fn value(permit: &NamespacePermit) -> String {
    match permit {
        NamespacePermit::Open => OPEN.to_string(),
        NamespacePermit::Closed(keys) => flow(keys),
    }
}

/// `keys` as a flow list: `[cli, mcp]`.
pub(crate) fn flow<'k>(keys: impl IntoIterator<Item = &'k String>) -> String {
    let keys: Vec<String> = keys.into_iter().map(|k| scalar(k)).collect();
    format!("[{}]", keys.join(", "))
}

/// `text` as a YAML scalar reading back as the string it is: plain where
/// that is unambiguous, single-quoted otherwise.
pub(crate) fn scalar(text: &str) -> String {
    let plain = text
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && !RESERVED.contains(&text.to_ascii_lowercase().as_str());
    if plain {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "''"))
    }
}

/// Plain words a YAML reader may take for something other than a string.
const RESERVED: [&str; 11] = [
    "true", "false", "null", "yes", "no", "on", "off", "y", "n", "nan", "inf",
];

#[cfg(test)]
#[path = "permits.test.rs"]
mod tests;
