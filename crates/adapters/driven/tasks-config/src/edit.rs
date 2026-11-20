//! Editing a config file's `tag_permits` in place, with angzarr-yaml's
//! editor (`angzarr_yaml::edit`, the one reader's, the owner's ruling of
//! 2026-10-31): the fewest edits that make the block the permits meant (a
//! key added to or removed from a namespace's list, a namespace or flat tag
//! added or removed), so every comment and line the edits do not touch is
//! kept as written, inside the block as well as outside it. A key joins the
//! end of its list, a namespace the end of `namespaces`: the person's order
//! is kept, nothing sorted.
//!
//! The editor applies an edit only if the text reads back as the value
//! meant, so an edit never leaves a file that means something else; and
//! every edit here is checked once more as a whole ([`verified`]): its
//! `tag_permits` exactly those meant and every other key as it was. A
//! fault in the editing costs a refused write, never a config silently
//! changed.

use std::collections::BTreeSet;

use angzarr_yaml::edit::{Document, Seg};
use angzarr_yaml::Value;
use ports::{NamespacePermit, TagPermits};

use crate::permits::{Raw, KEY, OPEN};
use crate::ConfigError;

/// `flow_depth` for a value written in block style all the way down.
const BLOCK: usize = usize::MAX;
/// `flow_depth` for a value written on its key's line: `[cli, mcp]`.
const FLOW: usize = 1;

/// The path to `namespaces`.
const NAMESPACES: [Seg<'static>; 2] = [Seg::Key(KEY), Seg::Key("namespaces")];

/// `source`, the config file at `path`, with its `tag_permits` made
/// `permits` by the fewest edits, checked by [`verified`]; the block
/// appended when it has none. `source` itself when it already permits
/// exactly them.
pub(crate) fn edited(
    path: &str,
    source: &str,
    permits: &TagPermits,
) -> Result<String, ConfigError> {
    let refused = |detail: String| ConfigError::EditRefused {
        path: path.to_string(),
        detail,
    };
    let mut doc = Document::parse(source).map_err(|e| refused(e.to_string()))?;
    let held = held(path, doc.value()).map_err(&refused)?;
    if held.as_ref() == Some(permits) {
        return Ok(source.to_string());
    }
    edit(&mut doc, held.as_ref(), permits).map_err(|e| refused(e.to_string()))?;
    let after = doc.into_string();
    verified(path, source, &after, permits)?;
    Ok(after)
}

/// `source`, the config file at `path`, with the keys that moved into the
/// store (`tag_permits`, `tag_schema`) taken out, everything else kept as
/// written; `None` when it holds neither. Read back first: refused
/// ([`ConfigError::EditRefused`]) unless every other key reads as it did.
pub(crate) fn retired(path: &str, source: &str) -> Result<Option<String>, ConfigError> {
    let refused = |detail: String| ConfigError::EditRefused {
        path: path.to_string(),
        detail,
    };
    let mut doc = Document::parse(source).map_err(|e| refused(e.to_string()))?;
    let held: Vec<&str> = MOVED
        .into_iter()
        .filter(|key| doc.value().get(key).is_some())
        .collect();
    if held.is_empty() {
        return Ok(None);
    }
    let mut rest = doc.value().clone();
    for key in held {
        rest.remove(key);
        let before = doc.text();
        doc.remove_key(&[], key)
            .map_err(|e| refused(e.to_string()))?;
        // The editor takes the comment lines directly above an entry with
        // it; here they belong to what is kept.
        let kept = comments_above_kept(&before, &doc.text());
        doc = Document::parse(&kept).map_err(|e| refused(e.to_string()))?;
    }
    if *doc.value() != rest {
        return Err(refused(
            "a key other than the moved ones changed".to_string(),
        ));
    }
    Ok(Some(doc.into_string()))
}

/// The keys that moved into the store.
const MOVED: [&str; 2] = [KEY, "tag_schema"];

/// `after`, `before` with a run of whole lines taken out, with the comment
/// lines (and blank ones among them) that led that run put back.
fn comments_above_kept(before: &str, after: &str) -> String {
    let was: Vec<&str> = before.split_inclusive('\n').collect();
    let now: Vec<&str> = after.split_inclusive('\n').collect();
    let head = was.iter().zip(&now).take_while(|(a, b)| a == b).count();
    // One run was taken out, so what follows it is the rest of `after`.
    let cut = &was[head..was.len() - (now.len() - head)];
    let leading = cut
        .iter()
        .take_while(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
        .count();
    let mut out: String = now[..head].concat();
    out.push_str(&cut[..leading].concat());
    out.push_str(&now[head..].concat());
    out
}

/// Whether `after`, an edit of `before` (the config file at `path`),
/// reads as `before` does with only its `tag_permits` replaced by
/// `permits`: refused ([`ConfigError::EditRefused`], saying how it
/// differs) when it does not parse, when its permits are not exactly
/// `permits`, or when any other key reads otherwise. Comments and layout
/// are not read, so are not compared.
pub(crate) fn verified(
    path: &str,
    before: &str,
    after: &str,
    permits: &TagPermits,
) -> Result<(), ConfigError> {
    let refused = |detail: String| ConfigError::EditRefused {
        path: path.to_string(),
        detail,
    };
    let (rest_before, _) = read_back(path, before).map_err(&refused)?;
    let (rest_after, read) =
        read_back(path, after).map_err(|why| refused(format!("the edit does not read: {why}")))?;
    if read.as_ref() != Some(permits) {
        return Err(refused(format!(
            "its {KEY} read as {read:?}, not the {permits:?} meant"
        )));
    }
    if rest_before != rest_after {
        return Err(refused(format!("a key other than {KEY} changed")));
    }
    Ok(())
}

/// `text` (the config file at `path`) as the reader reads it: every key
/// but `tag_permits`, and the permits it declares (`None` for none, or
/// none under the key).
fn read_back(path: &str, text: &str) -> Result<(Value, Option<TagPermits>), String> {
    let mut value = angzarr_yaml::parse(text).map_err(|e| e.to_string())?;
    let held = held(path, &value)?;
    value.remove(KEY);
    Ok((value, held))
}

/// The permits `doc` (the config file at `path`) declares: `None` for no
/// block, or none under the key.
fn held(path: &str, doc: &Value) -> Result<Option<TagPermits>, String> {
    match doc.get(KEY) {
        None | Some(Value::Null) => Ok(None),
        Some(raw) => {
            let raw: Raw = angzarr_yaml::from_value(raw).map_err(|e| e.to_string())?;
            raw.permits(path).map(Some).map_err(|e| e.to_string())
        }
    }
}

/// `doc` edited from holding `held` to `permits`.
fn edit(
    doc: &mut Document,
    held: Option<&TagPermits>,
    permits: &TagPermits,
) -> Result<(), angzarr_yaml::edit::EditError> {
    let Some(held) = held else {
        // No block, or one with nothing under it: written whole, in place
        // of what there was, at the end of the file.
        if doc.value().get(KEY).is_some() {
            doc.remove_key(&[], KEY)?;
        }
        return doc.set(&[], KEY, &block(permits), BLOCK);
    };
    if permits.namespaces.is_empty() && permits.flat.is_empty() {
        return doc.set(&[], KEY, &Value::Map(Vec::new()), BLOCK);
    }
    namespaces(doc, held, permits)?;
    listed(doc, "tags", &held.flat, &permits.flat)
}

/// The `namespaces` edits from `held` to `permits`: those gone removed,
/// those new added after the rest, those changed edited in place.
fn namespaces(
    doc: &mut Document,
    held: &TagPermits,
    permits: &TagPermits,
) -> Result<(), angzarr_yaml::edit::EditError> {
    let has = doc
        .value()
        .get(KEY)
        .is_some_and(|block| block.get("namespaces").is_some());
    if !has {
        if permits.namespaces.is_empty() {
            return Ok(());
        }
        return doc.set(
            &NAMESPACES[..1],
            "namespaces",
            &namespaces_value(permits),
            BLOCK,
        );
    }
    for name in held.namespaces.keys() {
        if !permits.namespaces.contains_key(name) {
            doc.remove_key(&NAMESPACES, name)?;
        }
    }
    for (name, permit) in &permits.namespaces {
        match (held.namespaces.get(name), permit) {
            (Some(was), now) if was == now => {}
            (Some(NamespacePermit::Closed(was)), NamespacePermit::Closed(now)) => {
                listed_at(doc, &[KEY, "namespaces", name], was, now)?;
            }
            (_, now) => doc.set(&NAMESPACES, name, &permit_value(now), FLOW)?,
        }
    }
    Ok(())
}

/// The edits to the list at `tag_permits.<key>` from `held` to `keys`.
fn listed(
    doc: &mut Document,
    key: &str,
    held: &BTreeSet<String>,
    keys: &BTreeSet<String>,
) -> Result<(), angzarr_yaml::edit::EditError> {
    listed_at(doc, &[KEY, key], held, keys)
}

/// The edits to the list at `at` from `held` to `keys`: each key gone
/// removed, each new one added at the end. The list is added whole, as a
/// flow list, when there is none.
fn listed_at(
    doc: &mut Document,
    at: &[&str],
    held: &BTreeSet<String>,
    keys: &BTreeSet<String>,
) -> Result<(), angzarr_yaml::edit::EditError> {
    if held == keys {
        return Ok(());
    }
    let (last, parent) = at.split_last().expect("a path names a key");
    let parent: Vec<Seg<'_>> = parent.iter().map(|k| Seg::Key(k)).collect();
    let list = value_at(doc.value(), at);
    if list.and_then(Value::as_seq).is_none() {
        return doc.set(&parent, last, &list_value(keys), FLOW);
    }
    for key in held.difference(keys) {
        let index = value_at(doc.value(), at)
            .and_then(Value::as_seq)
            .and_then(|items| items.iter().position(|v| v.as_str() == Some(key)))
            .expect("a held key is in its list");
        doc.remove(at, index)?;
    }
    for key in keys.difference(held) {
        doc.push(at, &Value::scalar(key.as_str()), FLOW)?;
    }
    Ok(())
}

/// The value at the key path `at` in `doc`.
fn value_at<'v>(doc: &'v Value, at: &[&str]) -> Option<&'v Value> {
    at.iter().try_fold(doc, |value, key| value.get(key))
}

/// What a namespace's permit is written as: `open`, or its keys.
fn permit_value(permit: &NamespacePermit) -> Value {
    match permit {
        NamespacePermit::Open => Value::scalar(OPEN),
        NamespacePermit::Closed(keys) => list_value(keys),
    }
}

/// `keys` as a list.
fn list_value(keys: &BTreeSet<String>) -> Value {
    Value::Seq(keys.iter().map(|k| Value::scalar(k.as_str())).collect())
}

/// What `namespaces` maps to.
fn namespaces_value(permits: &TagPermits) -> Value {
    Value::Map(
        permits
            .namespaces
            .iter()
            .map(|(name, permit)| (name.clone(), permit_value(permit)))
            .collect(),
    )
}

/// What `tag_permits` maps to: `namespaces`, then `tags`, each when it
/// has any; `{}` for permits permitting nothing.
fn block(permits: &TagPermits) -> Value {
    let mut entries = Vec::new();
    if !permits.namespaces.is_empty() {
        entries.push(("namespaces".to_string(), namespaces_value(permits)));
    }
    if !permits.flat.is_empty() {
        entries.push(("tags".to_string(), list_value(&permits.flat)));
    }
    Value::Map(entries)
}

#[cfg(test)]
#[path = "edit.test.rs"]
mod tests;
