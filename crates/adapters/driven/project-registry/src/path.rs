//! Paths compared as identities: symlinks resolved, else cleaned
//! lexically.

use std::path::{Component, Path, PathBuf};

use file_system::Fs;

/// `path` with its symlinks resolved on `fs`, when it exists; else
/// [`clean`]ed.
pub fn canonical(fs: &dyn Fs, path: &Path) -> PathBuf {
    fs.canonicalize(path).unwrap_or_else(|_| clean(path))
}

/// `path` cleaned lexically: no `.`, no empty or trailing separator, and
/// each `..` taking the name before it; a `..` above the root is dropped,
/// and one above a relative start is kept. An empty result is ".".
pub fn clean(path: &Path) -> PathBuf {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                Some(Component::Normal(_)) => {
                    parts.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => parts.push(part),
            },
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return PathBuf::from(".");
    }
    parts.iter().collect()
}

#[cfg(test)]
#[path = "path.test.rs"]
mod tests;
