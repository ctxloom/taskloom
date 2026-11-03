//! The lenient reads of a plan file: what its frontmatter says of a title
//! and the sessions when the YAML as a whole does not parse (a broken plan
//! is still listed), its title and its name.

use crate::PLAN_SUFFIX;

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

/// A plan's title: the frontmatter's `title:` when it has one, else the
/// first `# ` heading of `body`, else `name`.
pub(crate) fn title_of(front_title: Option<&str>, body: &str, name: &str) -> String {
    if let Some(title) = front_title.filter(|t| !t.trim().is_empty()) {
        return title.to_string();
    }
    body.lines()
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

#[cfg(test)]
#[path = "rules.test.rs"]
mod tests;
