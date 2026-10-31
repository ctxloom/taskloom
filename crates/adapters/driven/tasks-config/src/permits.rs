//! The `tag_permits` block: its shape as the reader decodes it.
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
                    return Err(ConfigError::NotAPermit {
                        path: path.to_string(),
                        namespace,
                        value: word,
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
