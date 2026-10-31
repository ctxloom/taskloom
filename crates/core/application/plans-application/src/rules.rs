//! The pure rules: where a plan's path resolves, what its text's hash is,
//! and what its frontmatter says.

use sha2::{Digest, Sha256};

use crate::{PathError, PLANS_DIR, PLAN_SUFFIX};

/// `given` (from `plan add --path`) as the work-root-relative path the
/// store records: normalized (`.` and `..` resolved, `/`-separated), an
/// absolute path inside `work_root` made relative. Refused when it leaves
/// the work root or is no `.plan.md`.
pub fn resolve_path(work_root: &str, given: &str) -> Result<String, PathError> {
    if given.is_empty() {
        return Err(PathError::Empty);
    }
    let path = match locate(work_root, given) {
        Located::Inside(path) => path,
        Located::Outside(_) => return Err(PathError::OutsideWorkRoot(given.to_string())),
    };
    match path.ends_with(PLAN_SUFFIX) {
        true => Ok(path),
        false => Err(PathError::NotAPlanFile(given.to_string())),
    }
}

/// Where a path names a file: inside the work root (work-root-relative),
/// or outside it (absolute). Both normalized.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Located {
    Inside(String),
    Outside(String),
}

/// `given`, normalized and placed against `work_root`: a relative path is
/// the work root's, an absolute one inside it made relative. A relative
/// path whose `..` leaves the work root is outside it, as the absolute
/// path it names.
pub(crate) fn locate(work_root: &str, given: &str) -> Located {
    let root = work_root.trim_end_matches('/');
    let absolute = if given.starts_with('/') {
        normalized(given)
    } else {
        normalized(&format!("{root}/{given}"))
    };
    match absolute
        .strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        Some(inside) if !inside.is_empty() => Located::Inside(inside.to_string()),
        _ => Located::Outside(absolute),
    }
}

/// An absolute `path` with `.`, `..` and empty components resolved; `..`
/// past the root stays at the root, as the filesystem resolves it.
fn normalized(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}

/// The path a plan's file has when `plan add` names none.
pub fn default_path(harp_id: &str) -> String {
    format!("{PLANS_DIR}/{harp_id}{PLAN_SUFFIX}")
}

/// sha-256 of `text`, 64 lowercase hex digits.
pub fn content_hash(text: &[u8]) -> String {
    Sha256::digest(text)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// What a plan file's YAML frontmatter says, as far as plans read it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Frontmatter {
    pub title: String,
    pub sessions: Vec<String>,
}

/// The frontmatter of `text`: `title:` and the `sessions:` list (a block
/// list, or a flow list `[a, b]`), empty when it has none. Only these two
/// keys are read, each a plain or quoted scalar.
pub fn frontmatter(text: &str) -> Frontmatter {
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return Frontmatter::default();
    }
    let mut found = Frontmatter::default();
    let mut in_sessions = false;
    for line in lines {
        let line = line.trim_end();
        if line == "---" || line == "..." {
            return found;
        }
        if in_sessions {
            if let Some(item) = line.trim_start().strip_prefix("- ") {
                if line.starts_with(char::is_whitespace) || line.starts_with('-') {
                    found.sessions.push(scalar(item));
                    continue;
                }
            }
            in_sessions = false;
        }
        if let Some(value) = line.strip_prefix("title:") {
            found.title = scalar(value);
        } else if let Some(value) = line.strip_prefix("sessions:") {
            let value = value.trim();
            match value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                Some(flow) => {
                    found.sessions = flow
                        .split(',')
                        .map(scalar)
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                None => in_sessions = value.is_empty(),
            }
        }
    }
    // No closing `---`: no frontmatter.
    Frontmatter::default()
}

/// A YAML scalar as plans write and read one: trimmed, its quotes taken
/// off (`"..."` with `\"` and `\\` escapes, `'...'` with `''`).
fn scalar(value: &str) -> String {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match (c, chars.clone().next()) {
                ('\\', Some(next @ ('"' | '\\'))) => {
                    out.push(next);
                    chars.next();
                }
                _ => out.push(c),
            }
        }
        return out;
    }
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    value.to_string()
}

/// `value` as a YAML scalar [`frontmatter`] reads back: plain where it can
/// be, double-quoted where YAML would read it otherwise.
pub(crate) fn yaml_scalar(value: &str) -> String {
    let plain = !value.is_empty()
        && value.trim() == value
        && !value.contains(": ")
        && !value.contains(" #")
        && !value.ends_with(':')
        && !value.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c));
    match plain {
        true => value.to_string(),
        false => format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")),
    }
}

/// The text `plan add` writes for a new plan: its frontmatter (title, and
/// the session it was added in) and its title as the heading.
pub(crate) fn new_plan_text(title: &str, session: &str) -> String {
    let mut text = format!("---\ntitle: {}\n", yaml_scalar(title));
    if !session.is_empty() {
        text.push_str(&format!("sessions:\n  - {}\n", yaml_scalar(session)));
    }
    text.push_str(&format!("---\n\n# {title}\n"));
    text
}

/// A plan's title from its text: the frontmatter's `title:`, else its first
/// `# ` heading, else `name`.
pub(crate) fn title_of(text: &str, name: &str) -> String {
    let title = frontmatter(text).title;
    if !title.trim().is_empty() {
        return title;
    }
    text.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .filter(|heading| !heading.is_empty())
        .unwrap_or(name)
        .to_string()
}

/// A plan's name: its file's base name without `.plan.md` (or `.md`).
pub fn plan_name(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.strip_suffix(PLAN_SUFFIX)
        .or_else(|| base.strip_suffix(".md"))
        .unwrap_or(base)
        .to_string()
}

/// `path` (work-root-relative) under `work_root`, absolute.
pub(crate) fn joined(work_root: &str, path: &str) -> String {
    format!("{}/{path}", work_root.trim_end_matches('/'))
}

#[cfg(test)]
#[path = "rules.test.rs"]
mod tests;
