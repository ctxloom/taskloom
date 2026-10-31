//! Why taskloom's configuration could not be read.

use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};

use crate::KEYS;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ConfigError {
    /// A config file that exists could not be read.
    Unreadable { path: String, detail: String },
    /// The work root's config file (or its directory) could not be
    /// written.
    Unwritable { path: String, detail: String },
    /// A config file that is not taskloom's: not YAML, a key it does not
    /// know, or a value of the wrong shape.
    Malformed { path: String, detail: String },
    /// A config file written for a newer taskloom.
    Newer {
        path: String,
        version: u64,
        current: u64,
    },
    /// An environment variable or `--config-set` naming no config key.
    UnknownKey { source: String, key: String },
    /// A `--config-set` entry that is not `<path>=<value>`.
    NotAnOverride { entry: String },
    /// A config file's list with a blank item (`source` the file, `value`
    /// its key).
    BlankItem { source: String, value: String },
    /// A key that moved into the project's store (`tag_schema`,
    /// `tag_permits`) set by `source`: a config file, an environment
    /// variable or `--config-set`. Refused, naming `taskloom schema
    /// import-config`, which moves a work root's file's into its store.
    KeyMoved { source: String, key: String },
    /// Tag permits written where there is no work root, so no project
    /// config file to write them to.
    NoWorkRoot,
    /// An edit of the config file at `path` that does not read back (with
    /// serde_norway, the reader) as the file before it with only its
    /// `tag_permits` replaced by those meant: refused, nothing written.
    /// `detail` says how it differs. A bug in the editing, never the
    /// person's.
    EditRefused { path: String, detail: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for ConfigError {
    fn code(&self) -> &str {
        match self {
            ConfigError::Unreadable { .. } => "tasks-config-unreadable",
            ConfigError::Unwritable { .. } => "tasks-config-unwritable",
            ConfigError::Malformed { .. } => "tasks-config-malformed",
            ConfigError::Newer { .. } => "tasks-config-newer",
            ConfigError::UnknownKey { .. } => "tasks-config-unknown-key",
            ConfigError::NotAnOverride { .. } => "tasks-config-not-an-override",
            ConfigError::BlankItem { .. } => "tasks-config-blank-item",
            ConfigError::KeyMoved { .. } => "tasks-config-key-moved",
            ConfigError::NoWorkRoot => "tasks-config-no-work-root",
            ConfigError::EditRefused { .. } => "tasks-config-edit-refused",
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        let s = |v: &String| Arg::from(v.as_str());
        match self {
            ConfigError::Unreadable { path, detail }
            | ConfigError::Unwritable { path, detail }
            | ConfigError::Malformed { path, detail }
            | ConfigError::EditRefused { path, detail } => {
                vec![("path", s(path)), ("ext_err_txt", s(detail))]
            }
            ConfigError::Newer {
                path,
                version,
                current,
            } => vec![
                ("path", s(path)),
                ("version", (*version).into()),
                ("current", (*current).into()),
            ],
            ConfigError::UnknownKey { source, key } => vec![
                ("source", s(source)),
                ("key", s(key)),
                ("known", KEYS.join(", ").into()),
            ],
            ConfigError::NotAnOverride { entry } => vec![("entry", s(entry))],
            ConfigError::BlankItem { source, value } => {
                vec![("source", s(source)), ("value", s(value))]
            }
            ConfigError::KeyMoved { source, key } => vec![("source", s(source)), ("key", s(key))],
            ConfigError::NoWorkRoot => Vec::new(),
        }
    }
}

/// This crate's en-US messages, its name first: what an edge's catalog
/// embeds to render its diagnostics.
pub const EN_US: (&str, &str) = (
    "tasks-config",
    include_str!("../locales/en-US/tasks-config.ftl"),
);

/// One value of every error, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let path = || "/home/u/.taskloom/config.yaml".to_string();
    vec![
        Box::new(ConfigError::Unreadable {
            path: path(),
            detail: "permission denied".to_string(),
        }),
        Box::new(ConfigError::Malformed {
            path: path(),
            detail: "unknown field `homeing`".to_string(),
        }),
        Box::new(ConfigError::Newer {
            path: path(),
            version: 2,
            current: 1,
        }),
        Box::new(ConfigError::UnknownKey {
            source: "TASKLOOM_CONFIG_TAGS".to_string(),
            key: "tags".to_string(),
        }),
        Box::new(ConfigError::NotAnOverride {
            entry: "tag_schema".to_string(),
        }),
        Box::new(ConfigError::BlankItem {
            source: path(),
            value: "tag_schema".to_string(),
        }),
        Box::new(ConfigError::KeyMoved {
            source: "TASKLOOM_CONFIG_TAG_SCHEMA".to_string(),
            key: "tag_schema".to_string(),
        }),
        Box::new(ConfigError::NoWorkRoot),
        Box::new(ConfigError::EditRefused {
            path: path(),
            detail: "schema_version changed".to_string(),
        }),
        Box::new(ConfigError::Unwritable {
            path: path(),
            detail: "read-only file system".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
