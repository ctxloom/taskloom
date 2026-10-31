//! Taskloom's configuration: its files, the environment and the
//! `--config-set` overrides, as far as the use cases ask of them. Each
//! layer is read:
//!
//! 1. `~/.taskloom/config.yaml`;
//! 2. `<root>/.taskloom/config.yaml`, the work root's;
//! 3. `TASKLOOM_CONFIG_<KEY>` environment variables, an empty one unset;
//! 4. `--config-set <path>=<value>` overrides.
//!
//! The tag rules moved into the project's store (its schema stream,
//! docs/plans/self-hosted-schema.md): a layer still setting `tag_schema` or
//! `tag_permits` is refused ([`ConfigError::KeyMoved`], naming `taskloom
//! schema import-config`), whatever it sets them to, and nothing reads
//! them for a rule. The import alone reads the work root's file's
//! ([`Configuration::tag_schema`], [`Configuration::tag_permits`]) and
//! then takes them out of it ([`ConfigurationWriter::retire_moved`]), by
//! the fewest edits, every other key and comment kept (the `edit` module).
//!
//! A missing file is no layer; a file that cannot be read, is not YAML,
//! names a key taskloom does not know or is a newer `schema_version` is
//! refused, as is an environment variable or override naming no key.
//! Files are read through the [`Fs`] it is given. `homing` (repo homing is
//! dropped) and `schema_version` are known and ignored.

mod edit;
mod error;
mod permits;

use std::ffi::OsString;
use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use angzarr_embedded_diagnostic::Report;
use file_system::{Fs, Perms};
use os::Mode;
use ports::{Configuration, ConfigurationWriter, TagPermits};
use serde::Deserialize;

#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{ConfigError, EN_US};

/// The directory a config file is in, at the home and the work root.
pub const DIR: &str = ".taskloom";
/// The config file's name.
pub const FILE: &str = "config.yaml";
/// What an environment variable's name starts with to override a key.
pub const ENV_PREFIX: &str = "TASKLOOM_CONFIG_";
/// The newest `schema_version` this taskloom reads.
pub const SCHEMA_VERSION: u64 = 1;
/// The config keys taskloom knows.
pub const KEYS: [&str; 4] = ["homing", "schema_version", "tag_permits", "tag_schema"];

/// One config file's keys.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct File {
    schema_version: Option<u64>,
    #[allow(dead_code)]
    homing: Option<String>,
    tag_schema: Option<Vec<String>>,
    /// Read from the work root's file alone ([`permits`]).
    tag_permits: Option<permits::Raw>,
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

    /// Refused where any layer sets a key that moved into the store, or
    /// cannot be read: each file read (and held to being taskloom's), each
    /// variable and override named.
    fn moved_keys(&self) -> Result<(), ConfigError> {
        for path in self.files() {
            let file = read(&*self.fs, &path)?;
            let held = [
                ("tag_schema", file.tag_schema.is_some()),
                (permits::KEY, file.tag_permits.is_some()),
            ];
            if let Some((key, _)) = held.into_iter().find(|(_, set)| *set) {
                return Err(ConfigError::KeyMoved {
                    source: path.display().to_string(),
                    key: key.to_string(),
                });
            }
        }
        let flavor = self.fs.os().flavor();
        for (name, value) in &self.env {
            // Configuration reads only UTF-8: any other variable is
            // skipped, not refused.
            let (Some(name), Some(value)) = (name.to_str(), value.to_str()) else {
                continue;
            };
            // An empty variable is unset, as the environment's are
            // everywhere: no layer. The prefix is matched as the host
            // compares names (any case on Windows).
            let prefixed = flavor.env_has_prefix(name, ENV_PREFIX) && !value.is_empty();
            let Some(key) = name.get(ENV_PREFIX.len()..).filter(|_| prefixed) else {
                continue;
            };
            known(name, &key.to_ascii_lowercase())?;
        }
        for entry in &self.overrides {
            let (path, _) = entry
                .split_once('=')
                .filter(|(path, _)| !path.trim().is_empty())
                .ok_or_else(|| ConfigError::NotAnOverride {
                    entry: entry.clone(),
                })?;
            known("--config-set", &path.trim().to_ascii_lowercase())?;
        }
        Ok(())
    }
}

impl Configuration for TaskloomConfig {
    fn moved(&mut self) -> Result<(), Report> {
        self.moved_keys().map_err(|e| Report::of(&e))
    }

    fn tag_schema(&mut self) -> Result<Vec<String>, Report> {
        let Some(root) = self.work_root() else {
            return Ok(Vec::new());
        };
        let path = root.join(DIR).join(FILE);
        let file = read(&*self.fs, &path).map_err(|e| Report::of(&e))?;
        Ok(file.tag_schema.unwrap_or_default())
    }

    fn tag_permits(&mut self) -> Result<Option<TagPermits>, Report> {
        self.permits().map_err(|e| Report::of(&e))
    }
}

impl ConfigurationWriter for TaskloomConfig {
    fn write_tag_permits(&mut self, permits: &TagPermits) -> Result<(), Report> {
        self.write_permits(permits).map_err(|e| Report::of(&e))
    }

    fn retire_moved(&mut self) -> Result<bool, Report> {
        self.retire().map_err(|e| Report::of(&e))
    }
}

impl TaskloomConfig {
    /// The work root, when there is one.
    fn work_root(&self) -> Option<PathBuf> {
        (self.root.0)().filter(|dir| !dir.as_os_str().is_empty())
    }

    /// The work root's `tag_permits`, as the import reads them.
    fn permits(&self) -> Result<Option<TagPermits>, ConfigError> {
        let Some(root) = self.work_root().map(|dir| dir.join(DIR).join(FILE)) else {
            return Ok(None);
        };
        let shown = root.display().to_string();
        read(&*self.fs, &root)?
            .tag_permits
            .map(|raw| raw.permits(&shown))
            .transpose()
    }

    /// `permits` written to the work root's config file by the fewest
    /// edits ([`edit::edited`]), the file and its directory made when
    /// there is none; nothing written when it already permits exactly
    /// them.
    fn write_permits(&self, permits: &TagPermits) -> Result<(), ConfigError> {
        let dir = self.work_root().ok_or(ConfigError::NoWorkRoot)?.join(DIR);
        let path = dir.join(FILE);
        let shown = path.display().to_string();
        let source = text_of(&*self.fs, &path)?.unwrap_or_default();
        // The file must read as taskloom's before it is edited.
        parsed(&path, &source)?;
        let edited = edit::edited(&shown, &source, permits)?;
        if edited == source {
            return Ok(());
        }
        let unwritable = |e: std::io::Error| ConfigError::Unwritable {
            path: shown.clone(),
            detail: e.to_string(),
        };
        self.fs
            .create_dir_all(&dir, Mode::DIR)
            .map_err(unwritable)?;
        self.fs
            .write_atomic(&path, edited.as_bytes(), Perms::KeepOr(Mode::FILE))
            .map_err(unwritable)
    }
}

/// The config file at `path` on `fs`; no keys when there is none.
fn read(fs: &dyn Fs, path: &Path) -> Result<File, ConfigError> {
    match text_of(fs, path)? {
        Some(text) => parsed(path, &text),
        None => Ok(File::default()),
    }
}

/// The text of the config file at `path` on `fs`; `None` when there is
/// none.
fn text_of(fs: &dyn Fs, path: &Path) -> Result<Option<String>, ConfigError> {
    match fs.read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ConfigError::Unreadable {
            path: path.display().to_string(),
            detail: e.to_string(),
        }),
    }
}

/// `text`, the config file at `path`, as taskloom reads it.
fn parsed(path: &Path, text: &str) -> Result<File, ConfigError> {
    let shown = || path.display().to_string();
    // An empty file, or one of comments, is a document of nothing.
    let file: Option<File> = serde_norway::from_str(text).map_err(|e| ConfigError::Malformed {
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
    // Refused here, whichever file it is in, as the schema's list is: a
    // config file is taskloom's whole or not at all.
    if let Some(raw) = &file.tag_permits {
        raw.clone().permits(&shown())?;
    }
    Ok(file)
}

impl TaskloomConfig {
    /// The work root's config file with the moved keys taken out
    /// ([`edit::retired`]): whether it held either. Nothing is written
    /// where there is no work root, no file, or neither key in it.
    fn retire(&self) -> Result<bool, ConfigError> {
        let Some(dir) = self.work_root().map(|root| root.join(DIR)) else {
            return Ok(false);
        };
        let path = dir.join(FILE);
        let Some(source) = text_of(&*self.fs, &path)? else {
            return Ok(false);
        };
        let shown = path.display().to_string();
        let Some(edited) = edit::retired(&shown, &source)? else {
            return Ok(false);
        };
        self.fs
            .write_atomic(&path, edited.as_bytes(), Perms::KeepOr(Mode::FILE))
            .map_err(|e| ConfigError::Unwritable {
                path: shown,
                detail: e.to_string(),
            })?;
        Ok(true)
    }
}

/// Whether `source` may set `key`: a key it knows, and none that moved.
fn known(source: &str, key: &str) -> Result<(), ConfigError> {
    match key {
        "homing" | "schema_version" => Ok(()),
        "tag_schema" | permits::KEY => Err(ConfigError::KeyMoved {
            source: source.to_string(),
            key: key.to_string(),
        }),
        _ => Err(ConfigError::UnknownKey {
            source: source.to_string(),
            key: key.to_string(),
        }),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../../adapters/driving/presentation/src/english.test.rs"]
mod english;
