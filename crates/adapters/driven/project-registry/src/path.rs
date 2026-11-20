//! Paths compared as identities: symlinks resolved, else cleaned
//! lexically.

use std::path::{Component, Path, PathBuf};

use file_system::Fs;
use os::Flavor;

/// `path` with its symlinks resolved on `fs`, when it exists; else
/// [`clean`]ed: as the host displays it ([`displayed`]), the form Go's
/// shared index.yaml holds.
pub fn canonical(fs: &dyn Fs, path: &Path) -> PathBuf {
    let resolved = fs.canonicalize(path).unwrap_or_else(|_| clean(path));
    displayed(fs.os().flavor(), resolved)
}

/// `path` as `flavor` displays it: on Windows without the verbatim prefix
/// a resolution adds (`\\?\C:\x` is `C:\x`); unchanged on unix, and
/// unchanged when it is not UTF-8.
pub fn displayed(flavor: Flavor, path: PathBuf) -> PathBuf {
    match path.to_str() {
        Some(text) => PathBuf::from(flavor.display_path(text)),
        None => path,
    }
}

/// Whether `a` and `b` name one path as `flavor` compares paths: exactly
/// on unix; on Windows in any case, either separator, no trailing one
/// ([`Flavor::path_key`]). A path that is not UTF-8 is compared exactly.
pub fn same(flavor: Flavor, a: &Path, b: &Path) -> bool {
    match (a.to_str(), b.to_str()) {
        (Some(a), Some(b)) => flavor.path_key(a) == flavor.path_key(b),
        _ => a == b,
    }
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
