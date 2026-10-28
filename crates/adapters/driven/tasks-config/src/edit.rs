//! Editing a config file's `tag_permits` in place, with yaml-rt-core: the
//! fewest edits that make the block the permits meant (a key added to or
//! removed from a namespace's list, a namespace or flat tag added or
//! removed), so every comment and line the edits do not touch is kept as
//! written, inside the block as well as outside it.
//!
//! serde_norway stays the reader. Every edit is read back with it before
//! anything is written ([`verified`]), and refused unless its
//! `tag_permits` are exactly those meant and every other key reads as it
//! did before: a fault in the editor costs a refused write, never a
//! config silently changed.
//!
//! yaml-rt-core 0.3.2 cannot replace a scalar with a block sequence in
//! place (`replace_at` refuses it), so a namespace changing from open
//! (`area: open`) to closed (`area: [cli]`) is a removal and an addition.
//!
//! RED: the proposed interface, its behaviour stubbed: the edit changes
//! nothing and the check admits anything. See edit.test.rs and the
//! permits tests in lib.test.rs.

use ports::TagPermits;
use yaml_rt_core::YamlDoc;

use crate::ConfigError;

/// `source`, the config file at `path`, with its `tag_permits` made
/// `permits` by the fewest edits, checked by [`verified`]; the block
/// appended when it has none.
// RED stub: parsed and given back unedited; write_tag_permits does not
// call it yet.
#[allow(dead_code)]
pub(crate) fn edited(
    path: &str,
    source: &str,
    permits: &TagPermits,
) -> Result<String, ConfigError> {
    let _ = permits;
    let doc = YamlDoc::parse(source).map_err(|e| ConfigError::Malformed {
        path: path.to_string(),
        detail: e.to_string(),
    })?;
    Ok(doc.as_source().to_string())
}

/// Whether `after`, an edit of `before` (the config file at `path`),
/// reads with serde_norway as `before` does with only its `tag_permits`
/// replaced by `permits`: refused ([`ConfigError::EditRefused`], saying
/// how it differs) when it does not parse, when its permits are not
/// exactly `permits`, or when any other key reads otherwise. Comments and
/// layout are not read, so are not compared.
// RED stub: admits anything; nothing calls it yet but its tests.
#[allow(dead_code)]
pub(crate) fn verified(
    path: &str,
    before: &str,
    after: &str,
    permits: &TagPermits,
) -> Result<(), ConfigError> {
    let _ = (path, before, after, permits);
    Ok(())
}

#[cfg(test)]
#[path = "edit.test.rs"]
mod tests;
