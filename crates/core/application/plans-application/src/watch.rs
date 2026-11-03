//! What a watch of the plan files needs of the plans: each file's metadata
//! and body as they are ([`Snapshot`]), and what differs between two
//! snapshots ([`changes`]), file by file and key by key. A change carries
//! which metadata changed and never the body's text: whoever wants the
//! text reads the file.

use std::collections::BTreeMap;

use crate::front::{FrontError, Meta};

/// One plan file as a watch sees it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Watched {
    pub id: Option<String>,
    pub meta: Meta,
    /// The markdown after the frontmatter.
    pub body: String,
    /// Why the frontmatter could not be read, when it could not.
    pub invalid: Option<FrontError>,
}

/// Every plan file, by its absolute path.
pub type Snapshot = BTreeMap<String, Watched>;

/// What happened to a file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    Created,
    Changed,
    Deleted,
}

/// How one frontmatter key differs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Delta {
    /// A scalar key (`id`, `title`, `status`): from and to, none where
    /// the key is absent.
    Scalar {
        key: &'static str,
        from: Option<String>,
        to: Option<String>,
    },
    /// A list key (`tags`, `governs`, `supersedes`, `sessions`): the items
    /// it gained and lost.
    List {
        key: &'static str,
        added: Vec<String>,
        removed: Vec<String>,
    },
    /// The body differs (its text is not carried).
    Body,
}

/// What happened to one plan file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Change {
    pub event: Event,
    /// The plan's id: the new one, else the last known (a deleted file's);
    /// none where the file has none.
    pub id: Option<String>,
    pub path: String,
    pub deltas: Vec<Delta>,
    /// Why the file's frontmatter cannot be read, when it cannot.
    pub invalid: Option<FrontError>,
}

/// The scalar keys of `meta`, in order.
fn scalars(meta: &Meta) -> [(&'static str, &Option<String>); 3] {
    [
        ("id", &meta.id),
        ("title", &meta.title),
        ("status", &meta.status),
    ]
}

/// The list keys of `meta`, in order.
fn lists(meta: &Meta) -> [(&'static str, &Vec<String>); 4] {
    [
        ("tags", &meta.tags),
        ("governs", &meta.governs),
        ("supersedes", &meta.supersedes),
        ("sessions", &meta.sessions),
    ]
}

/// The keys whose values differ between `from` (none: the file is new,
/// every key it has is an addition) and `to`.
fn deltas(from: Option<&Watched>, to: &Watched) -> Vec<Delta> {
    let empty = Meta::default();
    let before = from.map_or(&empty, |w| &w.meta);
    let mut found = Vec::new();
    for ((key, was), (_, now)) in scalars(before).into_iter().zip(scalars(&to.meta)) {
        if was != now {
            found.push(Delta::Scalar {
                key,
                from: was.clone(),
                to: now.clone(),
            });
        }
    }
    for ((key, was), (_, now)) in lists(before).into_iter().zip(lists(&to.meta)) {
        let added: Vec<String> = now.iter().filter(|i| !was.contains(i)).cloned().collect();
        let removed: Vec<String> = was.iter().filter(|i| !now.contains(i)).cloned().collect();
        if !added.is_empty() || !removed.is_empty() {
            found.push(Delta::List {
                key,
                added,
                removed,
            });
        }
    }
    if from.is_some_and(|w| w.body != to.body) {
        found.push(Delta::Body);
    }
    found
}

/// What differs between `old` and `new`, file by file in path order: a
/// file in `new` alone was created, in `old` alone deleted, and in both
/// with different metadata or body changed. Files that read the same say
/// nothing, whatever their bytes (a comment edited in the frontmatter).
pub fn changes(old: &Snapshot, new: &Snapshot) -> Vec<Change> {
    let mut paths: Vec<&String> = old.keys().chain(new.keys()).collect();
    paths.sort();
    paths.dedup();
    let mut found = Vec::new();
    for path in paths {
        match (old.get(path), new.get(path)) {
            (None, Some(now)) => found.push(Change {
                event: Event::Created,
                id: now.id.clone(),
                path: path.clone(),
                deltas: deltas(None, now),
                invalid: now.invalid.clone(),
            }),
            (Some(was), None) => found.push(Change {
                event: Event::Deleted,
                id: was.id.clone(),
                path: path.clone(),
                deltas: Vec::new(),
                invalid: None,
            }),
            (Some(was), Some(now)) if was != now => found.push(Change {
                event: Event::Changed,
                id: now.id.clone(),
                path: path.clone(),
                deltas: deltas(Some(was), now),
                invalid: now.invalid.clone(),
            }),
            _ => {}
        }
    }
    found
}

#[cfg(test)]
#[path = "watch.test.rs"]
mod tests;
