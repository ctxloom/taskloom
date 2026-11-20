//! A plan file's frontmatter: found between its `---` fences, read as the
//! plan's metadata, and edited in place through angzarr-yaml's editor. The
//! editor knows no fences, so the text between them is sliced out, edited
//! and spliced back; every byte outside the edit, the body first, is as it
//! was. A file with no frontmatter gets a fence block on its first write.
//!
//! The schema: `id` (a harp), `title`, `status`, `tags`, `governs`,
//! `supersedes` and `sessions`. Any other key is kept and ignored.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};
use angzarr_yaml::edit::Document;
use angzarr_yaml::Value;

/// `flow_depth` for a value written in block style all the way down.
const BLOCK: usize = usize::MAX;

/// The fence.
const FENCE: &str = "---";

/// A file's text, split at its frontmatter fences.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Split<'a> {
    /// The text does not start with a fence: no frontmatter.
    Absent,
    /// It starts with a fence that nothing closes.
    Unclosed,
    Fenced {
        /// The opening fence line, with its line break.
        open: &'a str,
        /// What is between the fences: whole lines, or nothing.
        front: &'a str,
        /// The closing fence line and everything after it.
        rest: &'a str,
    },
}

/// The lines of `text`, each with its line break.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split_inclusive('\n')
}

/// Whether `line` (with its break) is a fence: `---`, or `...` closing.
fn is_fence(line: &str, closing_dots: bool) -> bool {
    let line = line.trim_end_matches(['\r', '\n']);
    line == FENCE || (closing_dots && line == "...")
}

pub(crate) fn split(text: &str) -> Split<'_> {
    let Some(first) = lines(text).next().filter(|l| is_fence(l, false)) else {
        return Split::Absent;
    };
    let mut at = first.len();
    for line in lines(&text[first.len()..]) {
        if is_fence(line, true) {
            return Split::Fenced {
                open: first,
                front: &text[first.len()..at],
                rest: &text[at..],
            };
        }
        at += line.len();
    }
    Split::Unclosed
}

/// The markdown after the frontmatter; the whole text where there is none.
pub(crate) fn body(text: &str) -> &str {
    match split(text) {
        Split::Fenced { rest, .. } => lines(rest).nth(1).map_or("", |_| {
            let first = lines(rest).next().map_or(0, str::len);
            &rest[first..]
        }),
        Split::Absent | Split::Unclosed => text,
    }
}

/// A plan's metadata as its frontmatter writes it. Absent keys are `None`
/// or empty.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Meta {
    pub id: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub tags: Vec<String>,
    pub governs: Vec<String>,
    pub supersedes: Vec<String>,
    pub sessions: Vec<String>,
}

/// Why a frontmatter cannot be read or edited.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FrontError {
    /// An opening fence that nothing closes.
    Unclosed,
    /// The YAML between the fences does not parse; `detail` is the
    /// parser's.
    Yaml { detail: String },
    /// The frontmatter is not a mapping.
    NotAMapping,
    /// A key holds a value of the wrong shape (a mapping or a list where
    /// a scalar goes, a mapping in a list).
    Shape { key: String },
    /// The editor refused an edit, or what it wrote did not read back as
    /// meant: nothing is written.
    Edit { key: String, detail: String },
}

impl fmt::Display for FrontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for FrontError {
    fn code(&self) -> &str {
        match self {
            FrontError::Unclosed => code!("front-unclosed"),
            FrontError::Yaml { .. } => code!("front-yaml"),
            FrontError::NotAMapping => code!("front-not-a-mapping"),
            FrontError::Shape { .. } => code!("front-shape"),
            FrontError::Edit { .. } => code!("front-edit"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            FrontError::Unclosed | FrontError::NotAMapping => Vec::new(),
            FrontError::Yaml { detail } => vec![("ext_err_txt", detail.as_str().into())],
            FrontError::Shape { key } => vec![("key", key.as_str().into())],
            FrontError::Edit { key, detail } => vec![
                ("key", key.as_str().into()),
                ("ext_err_txt", detail.as_str().into()),
            ],
        }
    }
}

/// The document between `text`'s fences; None where `text` has no
/// frontmatter.
fn document(text: &str) -> Result<Option<Document>, FrontError> {
    match split(text) {
        Split::Absent => Ok(None),
        Split::Unclosed => Err(FrontError::Unclosed),
        Split::Fenced { front, .. } => {
            Document::parse(front)
                .map(Some)
                .map_err(|e| FrontError::Yaml {
                    detail: e.to_string(),
                })
        }
    }
}

/// The scalar at `key` of `map`: None where absent or null.
fn scalar(map: &Value, key: &str) -> Result<Option<String>, FrontError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Scalar(s)) => Ok(Some(s.clone())),
        Some(_) => Err(FrontError::Shape {
            key: key.to_string(),
        }),
    }
}

/// The list at `key` of `map`: a sequence of scalars (a lone scalar is
/// one item); empty where absent or null.
fn list(map: &Value, key: &str) -> Result<Vec<String>, FrontError> {
    let shape = || FrontError::Shape {
        key: key.to_string(),
    };
    match map.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Scalar(s)) => Ok(vec![s.clone()]),
        Some(Value::Seq(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_string).ok_or_else(shape))
            .collect(),
        Some(Value::Map(_)) => Err(shape()),
    }
}

/// The metadata `text`'s frontmatter holds; none where it has none.
pub fn read(text: &str) -> Result<Meta, FrontError> {
    let Some(doc) = document(text)? else {
        return Ok(Meta::default());
    };
    let value = doc.value();
    match value {
        Value::Null => return Ok(Meta::default()),
        Value::Map(_) => {}
        _ => return Err(FrontError::NotAMapping),
    }
    Ok(Meta {
        id: scalar(value, "id")?,
        title: scalar(value, "title")?,
        status: scalar(value, "status")?,
        tags: list(value, "tags")?,
        governs: list(value, "governs")?,
        supersedes: list(value, "supersedes")?,
        sessions: list(value, "sessions")?,
    })
}

/// One change to a frontmatter.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Edit {
    /// `key: value`, a scalar set in place or added at the end.
    Set(&'static str, String),
    /// `value` appended to the list at `key`, made if there is none.
    Push(&'static str, String),
    /// `value` taken out of the list at `key`.
    Remove(&'static str, String),
}

impl Edit {
    fn key(&self) -> &'static str {
        match self {
            Edit::Set(key, _) | Edit::Push(key, _) | Edit::Remove(key, _) => key,
        }
    }
}

/// `text` with `edits` applied to its frontmatter in order, after `id`
/// (when given) is written as the plan's id: the frontmatter made if the
/// text has none. Everything else stays as it was. The editor reads each
/// edit back, so one that would mean something else is refused.
pub fn edited(text: &str, id: Option<&str>, edits: &[Edit]) -> Result<String, FrontError> {
    let (open, front, rest) = match split(text) {
        Split::Absent => ("---\n", "", text),
        Split::Unclosed => return Err(FrontError::Unclosed),
        Split::Fenced { open, front, rest } => (open, front, rest),
    };
    let fenced = split(text) != Split::Absent;
    let seeded = seed(front, id)?;
    let mut doc = Document::parse(&seeded).map_err(|e| FrontError::Yaml {
        detail: e.to_string(),
    })?;
    if !matches!(doc.value(), Value::Map(_)) {
        return Err(FrontError::NotAMapping);
    }
    for edit in edits {
        apply(&mut doc, edit)?;
    }
    let edited = doc.into_string();
    Ok(match fenced {
        true => format!("{open}{edited}{rest}"),
        false => format!("{open}{edited}{FENCE}\n{rest}"),
    })
}

/// `front` made a mapping that holds `id`: when it holds no mapping yet
/// (nothing or comments), the id line is its first entry; when it is one
/// without an id, the id is added by the edit after (`front` as it is).
fn seed(front: &str, id: Option<&str>) -> Result<String, FrontError> {
    let parsed = angzarr_yaml::parse(front).map_err(|e| FrontError::Yaml {
        detail: e.to_string(),
    })?;
    let Some(id) = id else {
        return Ok(front.to_string());
    };
    let line = angzarr_yaml::write(
        &Value::Map(vec![("id".to_string(), Value::scalar(id))]),
        BLOCK,
    );
    match parsed {
        Value::Null => Ok(format!("{front}{line}")),
        Value::Map(entries) if entries.iter().all(|(k, _)| k != "id") => {
            // Added at the end of the mapping by the editor, after the
            // last line the person wrote.
            let mut doc = Document::parse(front).map_err(|e| FrontError::Yaml {
                detail: e.to_string(),
            })?;
            apply(&mut doc, &Edit::Set("id", id.to_string()))?;
            Ok(doc.into_string())
        }
        _ => Ok(front.to_string()),
    }
}

fn apply(doc: &mut Document, edit: &Edit) -> Result<(), FrontError> {
    let refused = |e: angzarr_yaml::edit::EditError| FrontError::Edit {
        key: edit.key().to_string(),
        detail: e.to_string(),
    };
    match edit {
        Edit::Set(key, value) => doc
            .set(&[], key, &Value::scalar(value.as_str()), 0)
            .map_err(refused),
        Edit::Push(key, value) => {
            if doc
                .value()
                .get(key)
                .is_some_and(|v| matches!(v, Value::Scalar(_)))
            {
                return Err(FrontError::Shape {
                    key: key.to_string(),
                });
            }
            doc.push(&[key], &Value::scalar(value.as_str()), BLOCK)
                .map_err(refused)
        }
        Edit::Remove(key, value) => {
            let at = doc
                .value()
                .get(key)
                .and_then(Value::as_seq)
                .and_then(|items| items.iter().position(|i| i.as_str() == Some(value)));
            match at {
                Some(index) => doc.remove(&[key], index).map_err(refused),
                None => Ok(()),
            }
        }
    }
}

/// The text of a new plan file: a frontmatter of `entries` (each a key and
/// its scalar or list), then `body`.
pub fn fresh(entries: Vec<(&str, Value)>, body: &str) -> String {
    let map = Value::Map(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    );
    format!(
        "{FENCE}\n{}{FENCE}\n{body}",
        angzarr_yaml::write(&map, BLOCK)
    )
}

#[cfg(test)]
#[path = "front.test.rs"]
mod tests;
