//! Taskloom's configuration, layered, as far as the tag schema needs it.
//! Each layer beats the one before:
//!
//! 1. `~/.taskloom/config.yaml`;
//! 2. `<root>/.taskloom/config.yaml`, the work root's;
//! 3. `TASKLOOM_CONFIG_<KEY>` environment variables (`TASKLOOM_CONFIG_TAG_SCHEMA`),
//!    an empty one unset;
//! 4. `--config-set <path>=<value>` overrides, in order.
//!
//! `tag_schema` is a list, and a layer that sets it replaces the whole list
//! below it: lists are never merged. An environment variable or override
//! gives the list comma-separated; a comma inside a double-quoted span does
//! not split, so an enum declaration's members (`tagma.enum:"k:x"="a,b"`)
//! stay one declaration. A blank item (`a,,b`, a trailing comma, or `''`
//! in a file's list) is refused, never dropped; an empty value is an empty
//! list. A missing file
//! is no layer; a file that cannot be read, is not YAML, names a key taskloom does not know or is a newer
//! `schema_version` is refused, as is an environment variable or override
//! naming no key. Files are read through the [`Fs`] it is given. `homing` (repo homing is dropped) and `schema_version`
//! are known and ignored. Nothing is written: there is no older version to
//! upgrade from.

mod error;

use std::ffi::OsString;
use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use annals_diagnostic::Report;
use file_system::Fs;
use ports::Configuration;
use serde::Deserialize;

pub use error::{diagnostic_samples, ConfigError, EN_US};

/// The directory a config file is in, at the home and the work root.
pub const DIR: &str = ".taskloom";
/// The config file's name.
pub const FILE: &str = "config.yaml";
/// What an environment variable's name starts with to override a key.
pub const ENV_PREFIX: &str = "TASKLOOM_CONFIG_";
/// The newest `schema_version` this taskloom reads.
pub const SCHEMA_VERSION: u64 = 1;
/// The config keys taskloom knows.
pub const KEYS: [&str; 3] = ["homing", "schema_version", "tag_schema"];

/// One config file's keys.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct File {
    schema_version: Option<u64>,
    #[allow(dead_code)]
    homing: Option<String>,
    tag_schema: Option<Vec<String>>,
}

/// Taskloom's configuration over a filesystem and the environment.
#[derive(Clone, Debug)]
pub struct TaskloomConfig {
    fs: Arc<dyn Fs>,
    home: Option<PathBuf>,
    root: FindRoot,
    env: Vec<(OsString, OsString)>,
    overrides: Vec<String>,
}

/// Where the work root is, asked each time the configuration is read.
#[derive(Clone)]
struct FindRoot(Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>);

impl fmt::Debug for FindRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FindRoot")
    }
}

impl TaskloomConfig {
    /// Configuration read from `home`'s and `root`'s config files on `fs`,
    /// when they are known, and from `env`, the process's environment as
    /// given (a variable whose name or value is not UTF-8 is skipped), with
    /// `overrides` (`<path>=<value>`, as `--config-set` gives them)
    /// applied last: bound once, where the configuration is made.
    pub fn new(
        fs: Arc<dyn Fs>,
        home: Option<PathBuf>,
        root: Option<PathBuf>,
        env: Vec<(OsString, OsString)>,
        overrides: Vec<String>,
    ) -> Self {
        TaskloomConfig::finding_root(fs, home, move || root.clone(), env, overrides)
    }

    /// [`TaskloomConfig::new`], the work root found by `root` each time the
    /// configuration is read, and not before: making it does no I/O.
    pub fn finding_root(
        fs: Arc<dyn Fs>,
        home: Option<PathBuf>,
        root: impl Fn() -> Option<PathBuf> + Send + Sync + 'static,
        env: Vec<(OsString, OsString)>,
        overrides: Vec<String>,
    ) -> Self {
        TaskloomConfig {
            fs,
            home,
            root: FindRoot(Arc::new(root)),
            env,
            overrides,
        }
    }

    fn files(&self) -> impl Iterator<Item = PathBuf> {
        [self.home.clone(), (self.root.0)()]
            .into_iter()
            .flatten()
            .filter(|dir| !dir.as_os_str().is_empty())
            .map(|dir| dir.join(DIR).join(FILE))
    }

    fn resolve(&self) -> Result<Vec<String>, ConfigError> {
        let mut tag_schema = Vec::new();
        for path in self.files() {
            if let Some(list) = read(&*self.fs, &path)?.tag_schema {
                tag_schema = list;
            }
        }
        for (name, value) in &self.env {
            // Configuration reads only UTF-8: any other variable is
            // skipped, not refused.
            let (Some(name), Some(value)) = (name.to_str(), value.to_str()) else {
                continue;
            };
            // An empty variable is unset, as the environment's are
            // everywhere: no layer, never an empty list.
            let Some(key) = name.strip_prefix(ENV_PREFIX).filter(|_| !value.is_empty()) else {
                continue;
            };
            if let Some(list) = set(name, &key.to_ascii_lowercase(), value)? {
                tag_schema = list;
            }
        }
        for entry in &self.overrides {
            let (path, value) = entry
                .split_once('=')
                .filter(|(path, _)| !path.trim().is_empty())
                .ok_or_else(|| ConfigError::NotAnOverride {
                    entry: entry.clone(),
                })?;
            let key = path.trim().to_ascii_lowercase();
            if let Some(list) = set("--config-set", &key, value)? {
                tag_schema = list;
            }
        }
        Ok(tag_schema)
    }
}

impl Configuration for TaskloomConfig {
    fn tag_schema(&mut self) -> Result<Vec<String>, Report> {
        self.resolve().map_err(|e| Report::of(&e))
    }
}

/// The config file at `path` on `fs`; no keys when there is none.
fn read(fs: &dyn Fs, path: &Path) -> Result<File, ConfigError> {
    let shown = || path.display().to_string();
    let text = match fs.read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(File::default()),
        Err(e) => {
            return Err(ConfigError::Unreadable {
                path: shown(),
                detail: e.to_string(),
            })
        }
    };
    // An empty file, or one of comments, is a document of nothing.
    let file: Option<File> = serde_norway::from_str(&text).map_err(|e| ConfigError::Malformed {
        path: shown(),
        detail: e.to_string(),
    })?;
    let file = file.unwrap_or_default();
    if let Some(version) = file.schema_version.filter(|v| *v > SCHEMA_VERSION) {
        return Err(ConfigError::Newer {
            path: shown(),
            version,
            current: SCHEMA_VERSION,
        });
    }
    // A blank item is refused, never dropped, as a comma-separated list's
    // is.
    let blank = |list: &Vec<String>| list.iter().any(|item| item.trim().is_empty());
    if file.tag_schema.as_ref().is_some_and(blank) {
        return Err(ConfigError::BlankItem {
            source: shown(),
            value: "tag_schema".to_string(),
        });
    }
    Ok(file)
}

/// What `source` setting `key` to `value` does to the tag schema: `Some`
/// list when it sets it, `None` for another known key.
fn set(source: &str, key: &str, value: &str) -> Result<Option<Vec<String>>, ConfigError> {
    match key {
        "tag_schema" => split(source, value).map(Some),
        "homing" | "schema_version" => Ok(None),
        _ => Err(ConfigError::UnknownKey {
            source: source.to_string(),
            key: key.to_string(),
        }),
    }
}

/// `value`'s comma-separated items, trimmed, as `source` gives them; a
/// comma inside a double-quoted span (where `\` escapes the next
/// character) does not split. A value of nothing but blanks is no items;
/// otherwise a blank item is refused, never dropped, as is a quote never
/// closed.
fn split(source: &str, value: &str) -> Result<Vec<String>, ConfigError> {
    let refused = |error: fn(String, String) -> ConfigError| {
        Err(error(source.to_string(), value.to_string()))
    };
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let mut item = String::new();
    let (mut quoted, mut escaped) = (false, false);
    for c in value.chars() {
        if escaped {
            escaped = false;
        } else if quoted && c == '\\' {
            escaped = true;
        } else if c == '"' {
            quoted = !quoted;
        } else if c == ',' && !quoted {
            items.push(std::mem::take(&mut item));
            continue;
        }
        item.push(c);
    }
    if quoted {
        return refused(|source, value| ConfigError::Unterminated { source, value });
    }
    items.push(item);
    let items: Vec<String> = items.into_iter().map(|i| i.trim().to_string()).collect();
    if items.iter().any(String::is_empty) {
        return refused(|source, value| ConfigError::BlankItem { source, value });
    }
    Ok(items)
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
