//! Agents' MCP configurations behind the [`AgentConfigs`] port: Claude
//! Code's, the user's `~/.claude.json` and a project's `<dir>/.mcp.json`,
//! each an `mcpServers` object of named servers.
//!
//! A registration edits only the server's own member and leaves every
//! other byte of the file as it was, splicing as hew splices, so every
//! taskloom writes the same bytes. An entry already under the name is
//! taken out first when it runs the owner's program, and refused when it
//! runs another. A file that is not JSON is copied beside itself
//! (`<path>.corrupt-<unix seconds>`) and left alone. A write replaces the
//! file whole, atomically, keeping its permissions (a new file is private,
//! 0600). The whole read-modify-write, a dry run's included, runs under
//! the file's lock (file-lock's [`file_lock::lock_file`]:
//! `~/.ctxloom/locks/<FlatName(path)>.lock` under the reap interlock), so
//! it takes turns with every other ctxloom-family binary's write of the
//! file.
//!
//! This adapter keeps no application records of what it wrote; it reads
//! the ones an earlier taskloom left, for one fact: whether that write
//! made the `mcpServers` object itself, which an uninstall then takes out
//! once it is empty (see [`records`]).

mod error;
mod exec;
mod json;
pub mod records;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use angzarr_embedded_diagnostic::Report;
use file_system::{Fs, Kind, Perms};
use os::Mode;
use ports::{AgentConfigs, McpServer, Rewritten, Scope};

#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::ConfigError;
pub use error::EN_US;

/// Claude Code, as `--engine` names it.
pub const CLAUDE_CODE: &str = "claude-code";

/// The object a configuration's servers are in.
pub const SERVERS: &str = "mcpServers";

/// The agents' configuration files under a home, on the filesystem `fs`,
/// the PATH a server's command is looked for on (with PATHEXT, the
/// extensions Windows tries), and how long a write waits for the file's
/// lock.
#[derive(Clone, Debug)]
pub struct AgentConfigFiles {
    pub fs: Arc<dyn Fs>,
    pub home: Option<PathBuf>,
    pub path: Option<OsString>,
    /// PATHEXT, as the environment gives it (read on Windows only).
    pub pathext: Option<String>,
    /// The waits for the file's lock ([`file_lock::Waits::DEFAULT`] in
    /// use).
    pub waits: file_lock::Waits,
}

impl AgentConfigs for AgentConfigFiles {
    fn engines(&self) -> Vec<String> {
        vec![CLAUDE_CODE.to_string()]
    }

    fn present(&mut self, engine: &str, scope: &Scope) -> bool {
        if engine != CLAUDE_CODE {
            return false;
        }
        let (dir, file) = match scope {
            Scope::User => match &self.home {
                Some(home) => (home.as_path(), ".claude.json"),
                None => return false,
            },
            Scope::Project(dir) => (dir.as_path(), ".mcp.json"),
        };
        // What cannot be looked at is not there.
        let there = |path: PathBuf| self.fs.exists(&path).unwrap_or(false);
        there(dir.join(file)) || there(dir.join(".claude"))
    }

    fn config_path(&mut self, engine: &str, scope: &Scope) -> Result<PathBuf, Report> {
        if engine != CLAUDE_CODE {
            return Err(Report::of(&ConfigError::UnknownEngine {
                engine: engine.to_string(),
            }));
        }
        match scope {
            Scope::User => {
                let home = self.home.as_ref().ok_or_else(|| self.no_home())?;
                Ok(go_join(home, ".claude.json"))
            }
            Scope::Project(dir) => Ok(go_join(dir, ".mcp.json")),
        }
    }

    fn register(
        &mut self,
        engine: &str,
        path: &Path,
        name: &str,
        owner: &str,
        server: Option<&McpServer>,
        dry_run: bool,
    ) -> Result<Rewritten, Report> {
        if engine != CLAUDE_CODE {
            return Err(Report::of(&ConfigError::UnknownEngine {
                engine: engine.to_string(),
            }));
        }
        self.rewrite(path, name, owner, server, dry_run)
    }

    fn installed(&mut self, _: &str, path: &Path, name: &str) -> Result<Option<bool>, Report> {
        let Some(src) = read(&*self.fs, path).map_err(|e| Report::of(&e))? else {
            return Ok(None);
        };
        let parse = |detail: String| Report::of(&ConfigError::Parse { detail });
        let Some(doc) = json::parse_first(&src).map_err(|e| parse(e.to_string()))? else {
            return Ok(Some(false));
        };
        if doc.members().is_none() {
            return Err(parse("the configuration is not a JSON object".to_string()));
        }
        let held = doc
            .member(SERVERS)
            .and_then(|(_, servers)| servers.value.member(name))
            .is_some();
        Ok(Some(held))
    }

    fn resolvable(&mut self, command: &str) -> bool {
        let Some(path) = &self.path else {
            return false;
        };
        let flavor = self.fs.os().flavor();
        let names = flavor.candidates(command, self.pathext.as_deref());
        flavor.split_search_path_os(path).iter().any(|dir| {
            names
                .iter()
                .any(|name| executable(&*self.fs, &dir.join(name)))
        })
    }
}

impl AgentConfigFiles {
    /// That there is no home, naming the variable this host reads it from.
    fn no_home(&self) -> Report {
        Report::of(&ConfigError::NoHome {
            flavor: self.fs.os().flavor(),
        })
    }

    /// taskloom's application records, under the home.
    fn records(&self) -> Option<PathBuf> {
        Some(
            self.home
                .as_ref()?
                .join(".ctxloom/records")
                .join(records::TASKLOOM_RECORDS),
        )
    }

    /// The whole read-modify-write of `path`, under the file's lock, so any
    /// ctxloom-family binary rewriting it meanwhile takes its turn. A dry
    /// run takes it too.
    fn rewrite(
        &self,
        path: &Path,
        name: &str,
        owner: &str,
        server: Option<&McpServer>,
        dry_run: bool,
    ) -> Result<Rewritten, Report> {
        let home = self.home.as_ref().ok_or_else(|| self.no_home())?;
        let _locked =
            file_lock::lock_file(&*self.fs, home, path, self.waits).map_err(|e| Report::of(&e))?;
        self.rewrite_locked(path, name, owner, server, dry_run)
            .map_err(|e| Report::of(&e))
    }

    fn rewrite_locked(
        &self,
        path: &Path,
        name: &str,
        owner: &str,
        server: Option<&McpServer>,
        dry_run: bool,
    ) -> Result<Rewritten, ConfigError> {
        let found = read(&*self.fs, path)?;
        let before = found.clone().unwrap_or_else(|| b"{}".to_vec());
        let made_by_go = || {
            self.records().is_some_and(|dir| {
                records::added(
                    &*self.fs,
                    &dir,
                    &path.display().to_string(),
                    &format!("/{SERVERS}"),
                )
            })
        };
        let after = match splice(&before, path, name, owner, server, made_by_go) {
            Ok(after) => after,
            Err(Splice::Unparsed(detail)) => {
                return Err(corrupt(&*self.fs, path, &before, &detail))
            }
            Err(Splice::Refused(error)) => return Err(error),
        };
        let changed = after != before;
        if changed && !dry_run {
            write(&*self.fs, path, &after)?;
        }
        Ok(Rewritten {
            changed,
            after: String::from_utf8_lossy(&after).into_owned(),
        })
    }
}

/// Why a splice wrote nothing.
enum Splice {
    /// The file is not JSON a server can be written into.
    Unparsed(String),
    Refused(ConfigError),
}

/// `src` with `server` registered under `name` (or, for none, with the
/// entry under `name` taken out), each entry already there that runs
/// `owner` taken out first. `made_by_go` says whether the last recorded
/// write made the servers object, so an uninstall that empties it takes it
/// out too.
fn splice(
    src: &[u8],
    path: &Path,
    name: &str,
    owner: &str,
    server: Option<&McpServer>,
    made_by_go: impl FnOnce() -> bool,
) -> Result<Vec<u8>, Splice> {
    let doc = json::parse(src).map_err(|e| Splice::Unparsed(e.to_string()))?;
    if doc.members().is_none() {
        // An install refuses it as unparsable; an uninstall finds nothing
        // of its own in it.
        return match server {
            Some(_) => Err(Splice::Unparsed(
                "the configuration is not a JSON object".to_string(),
            )),
            None => Ok(src.to_vec()),
        };
    }
    let mut out = src.to_vec();
    let mut took_out = false;
    if let Some((_, servers)) = doc.member(SERVERS) {
        if let Some((index, entry)) = servers.value.member(name) {
            if owned(&entry.value, owner) {
                out = json::remove_member(src, &servers.value, index).apply(src);
                took_out = true;
            } else if server.is_some() {
                return Err(Splice::Refused(ConfigError::Foreign {
                    path: path.display().to_string(),
                    name: name.to_string(),
                }));
            }
        }
    }
    let doc = json::parse(&out).map_err(|e| Splice::Unparsed(e.to_string()))?;
    let servers = doc.member(SERVERS).map(|(index, m)| (index, &m.value));
    let Some(server) = server else {
        let emptied = servers.filter(|(_, s)| s.members().is_some_and(<[_]>::is_empty));
        return match emptied {
            Some((index, _)) if took_out && made_by_go() => {
                Ok(json::remove_member(&out, &doc, index).apply(&out))
            }
            _ => Ok(out),
        };
    };
    let entry = entry(server);
    let edit = match servers {
        None => {
            let mut servers = BTreeMap::new();
            servers.insert(name.to_string(), entry);
            json::append_member(&out, &doc, SERVERS, &json::Value::Object(servers))
        }
        Some((_, servers)) if servers.members().is_some() => {
            json::append_member(&out, servers, name, &entry)
        }
        Some((_, servers)) if servers.kind == json::Kind::Null => {
            return Err(Splice::Refused(ConfigError::NotAnObject {
                path: path.display().to_string(),
                pointer: format!("/{SERVERS}"),
            }))
        }
        Some(_) => {
            return Err(Splice::Unparsed(format!(
                "{SERVERS} is not an object of servers"
            )))
        }
    };
    Ok(edit.apply(&out))
}

/// A server's entry, as a stdio server is written: its command, and its
/// args when it has any.
fn entry(server: &McpServer) -> json::Value {
    let mut entry = BTreeMap::new();
    entry.insert(
        "command".to_string(),
        json::Value::String(server.command.clone()),
    );
    if !server.args.is_empty() {
        let args = server.args.iter().cloned().map(json::Value::String);
        entry.insert("args".to_string(), json::Value::Array(args.collect()));
    }
    json::Value::Object(entry)
}

/// Whether the entry `value` runs `owner`: a command line, or an object
/// whose `command` is one.
fn owned(value: &json::Node, owner: &str) -> bool {
    match &value.kind {
        json::Kind::String(command) => exec::runs(command, owner),
        json::Kind::Object(_) => match value.member("command") {
            Some((_, command)) => match &command.value.kind {
                json::Kind::String(command) => exec::runs(command, owner),
                _ => false,
            },
            None => false,
        },
        _ => false,
    }
}

/// The file's bytes, or `None` when there is none.
fn read(fs: &dyn Fs, path: &Path) -> Result<Option<Vec<u8>>, ConfigError> {
    match fs.read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ConfigError::Read {
            path: path.display().to_string(),
            detail: e.to_string(),
        }),
    }
}

/// Copy the unparsable `src` of `path` beside it, and say so.
fn corrupt(fs: &dyn Fs, path: &Path, src: &[u8], detail: &str) -> ConfigError {
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let backup = PathBuf::from(format!("{}.corrupt-{stamp}", path.display()));
    // Private, as the file it copies may hold secrets.
    match fs.write_atomic(&backup, src, Perms::KeepOr(Mode::PRIVATE_FILE)) {
        Ok(()) => ConfigError::Corrupt {
            file,
            detail: detail.to_string(),
            backup: backup.display().to_string(),
        },
        Err(e) => ConfigError::CorruptUnsaved {
            file,
            detail: detail.to_string(),
            reason: e.to_string(),
        },
    }
}

/// Replace the file at `path` with `bytes` whole, atomically, keeping its
/// permissions (0600 for a new one), its directory made if missing.
fn write(fs: &dyn Fs, path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    let failed = |e: std::io::Error| ConfigError::Write {
        path: path.display().to_string(),
        detail: e.to_string(),
    };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        // As std's create_dir_all makes them: 0777 before the umask.
        fs.create_dir_all(dir, Mode::from_bits(0o777))
            .map_err(failed)?;
    }
    fs.write_atomic(path, bytes, Perms::KeepOr(Mode::PRIVATE_FILE))
        .map_err(failed)
}

/// Whether `path` is a file this process may run.
fn executable(fs: &dyn Fs, path: &Path) -> bool {
    fs.metadata(path)
        .is_ok_and(|meta| meta.kind == Kind::File && meta.executable)
}

/// `dir` joined with `name` and cleaned lexically, so a path reads (and
/// the records name it) the same however it was spelled: `.` and
/// `.mcp.json` are `.mcp.json`.
pub fn go_join(dir: &Path, name: &str) -> PathBuf {
    let joined = dir.join(name);
    let mut parts: Vec<Component> = Vec::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                Some(Component::Normal(_)) => {
                    parts.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => parts.push(component),
            },
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return PathBuf::from(".");
    }
    parts.iter().collect()
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../../adapters/driving/presentation/src/english.test.rs"]
mod english;
