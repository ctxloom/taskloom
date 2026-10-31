//! Editing a config file's `tag_permits` in place, with yaml-rt-core: the
//! fewest edits that make the block the permits meant (a key added to or
//! removed from a namespace's list, a namespace or flat tag added or
//! removed), so every comment and line the edits do not touch is kept as
//! written, inside the block as well as outside it. A key joins the end of
//! its list, a namespace the end of `namespaces`: the person's order is
//! kept, nothing sorted.
//!
//! serde_norway stays the reader. Every edit is read back with it before
//! anything is written ([`verified`]), and refused unless its
//! `tag_permits` are exactly those meant and every other key reads as it
//! did before: a fault in the editor costs a refused write, never a
//! config silently changed.
//!
//! yaml-rt-core 0.3.2's quirks, worked around here:
//!
//! - A block collection of one line, as a fragment of its own, is put on
//!   its key's line, where it does not parse: a one-item block list
//!   ("compact block collection value is not allowed"), or a one-entry
//!   mapping ("plain scalar contains a nested mapping separator"). So a
//!   list an edit adds is a flow list (`area: [cli]`), and a `namespaces`
//!   added with one open namespace is a flow mapping (`{relates: open}`).
//!   A block written whole, where the file has none, is in block style.
//! - Replacing a value replaces its trailing comment with it (a scalar's
//!   span runs to the end of its line), and a scalar cannot be replaced by
//!   a block list in place. So a namespace turning open or closed has only
//!   its value's spelling replaced, the comment beside it kept
//!   ([`set_value`]); a block list turning open has its entry rewritten
//!   as `<key>: open`, the comments above it kept (those of its items go
//!   with them).
//! - Removing a block list's last item removes the blank lines after it
//!   too, so they are put back ([`remove`]).

use std::collections::BTreeSet;

use ports::{NamespacePermit, TagPermits};
use serde_norway::Value;
use yaml_rt_core::{JsonPointer, NodeId, NodeKind, YamlDoc, YamlFragment};

use crate::permits::{self, Raw, KEY};
use crate::ConfigError;

/// `source`, the config file at `path`, with its `tag_permits` made
/// `permits` by the fewest edits, checked by [`verified`]; the block
/// appended when it has none. `source` itself when it already permits
/// exactly them.
pub(crate) fn edited(
    path: &str,
    source: &str,
    permits: &TagPermits,
) -> Result<String, ConfigError> {
    let (_, held) = read_back(path, source).map_err(|detail| ConfigError::Malformed {
        path: path.to_string(),
        detail,
    })?;
    if held.as_ref() == Some(permits) {
        return Ok(source.to_string());
    }
    let after =
        edit(source, held.as_ref(), permits).map_err(|detail| ConfigError::EditRefused {
            path: path.to_string(),
            detail,
        })?;
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
    let mapping = |text: &str| -> Result<serde_norway::Mapping, String> {
        let value: Value = serde_norway::from_str(text).map_err(|e| e.to_string())?;
        Ok(match value {
            Value::Mapping(mapping) => mapping,
            _ => serde_norway::Mapping::new(),
        })
    };
    let mut rest = mapping(source).map_err(|detail| ConfigError::Malformed {
        path: path.to_string(),
        detail,
    })?;
    let held: Vec<&str> = MOVED
        .into_iter()
        .filter(|key| rest.remove(*key).is_some())
        .collect();
    if held.is_empty() {
        return Ok(None);
    }
    // Each key's entry is cut as text, from the start of its key's line to
    // the end of its value's (and the line break after it): yaml-rt-core's
    // own removal takes the comments above an entry with it, which belong
    // to what is kept.
    let mut after = source.to_string();
    for key in held {
        let doc = YamlDoc::parse(&after).map_err(|e| refused(e.to_string()))?;
        let value = node(&doc, &[key]).map_err(&refused)?;
        let entry = entry(&doc, &[key], value).map_err(&refused)?;
        let span = doc
            .node(entry)
            .ok_or_else(|| refused("no node".to_string()))?
            .span();
        let text = doc.as_source();
        let start = text[..span.start as usize]
            .rfind('\n')
            .map_or(0, |at| at + 1);
        let end = match text[span.end as usize..].find('\n') {
            Some(at) => span.end as usize + at + 1,
            None => text.len(),
        };
        // A block value's span runs through its last line break already.
        let end = match text[..span.end as usize].ends_with('\n') {
            true => span.end as usize,
            false => end,
        };
        after = format!("{}{}", &text[..start], &text[end..]);
    }
    let read = mapping(&after).map_err(|why| refused(format!("the edit does not read: {why}")))?;
    if read != rest {
        return Err(refused(
            "a key other than the moved ones changed".to_string(),
        ));
    }
    Ok(Some(after))
}

/// The keys that moved into the store.
const MOVED: [&str; 2] = [KEY, "tag_schema"];

/// Whether `after`, an edit of `before` (the config file at `path`),
/// reads with serde_norway as `before` does with only its `tag_permits`
/// replaced by `permits`: refused ([`ConfigError::EditRefused`], saying
/// how it differs) when it does not parse, when its permits are not
/// exactly `permits`, or when any other key reads otherwise. Comments and
/// layout are not read, so are not compared.
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

/// `text` (the config file at `path`) as serde_norway reads it: every key
/// but `tag_permits`, and the permits it declares (`None` for none, or
/// none under the key).
fn read_back(path: &str, text: &str) -> Result<(Value, Option<TagPermits>), String> {
    let mut value: Value = serde_norway::from_str(text).map_err(|e| e.to_string())?;
    // A file of nothing (or of comments) is a mapping of no keys.
    if value.is_null() {
        value = Value::Mapping(serde_norway::Mapping::new());
    }
    let held = match value.as_mapping_mut() {
        Some(mapping) => mapping.remove(KEY),
        None => None,
    };
    let held = match held {
        None | Some(Value::Null) => None,
        Some(raw) => {
            let raw: Raw = serde_norway::from_value(raw).map_err(|e| e.to_string())?;
            Some(raw.permits(path).map_err(|e| e.to_string())?)
        }
    };
    Ok((value, held))
}

/// `source` edited from holding `held` to `permits`; why not, when
/// yaml-rt-core refuses an edit.
fn edit(source: &str, held: Option<&TagPermits>, permits: &TagPermits) -> Result<String, String> {
    let mut doc = YamlDoc::parse(source).map_err(|e| e.to_string())?;
    if doc.document_count() == 0 {
        // Nothing but comments, or nothing at all: the block is the file's
        // first key, written whole.
        return Ok(format!("{}{}", terminated(source), written(permits)));
    }
    let Some(held) = held else {
        // No block, or one with nothing under it: written whole, in place
        // of what there was, at the end of the file.
        if doc.resolve_pointer(0, &pointer(&[KEY])).is_ok() {
            doc.remove_at(0, &pointer(&[KEY]))
                .map_err(|e| e.to_string())?;
        }
        add(&mut doc, &[KEY], &permits::block(permits))?;
        return Ok(doc.to_string());
    };
    if permits.namespaces.is_empty() && permits.flat.is_empty() {
        set_value(&mut doc, &[KEY], "{}")?;
        return Ok(doc.to_string());
    }
    namespaces(&mut doc, held, permits)?;
    listed(&mut doc, &[KEY, "tags"], &held.flat, &permits.flat)?;
    Ok(doc.to_string())
}

/// The `namespaces` edits from `held` to `permits`: those gone removed,
/// those new added after the rest, those changed edited in place.
fn namespaces(doc: &mut YamlDoc, held: &TagPermits, permits: &TagPermits) -> Result<(), String> {
    const AT: [&str; 2] = [KEY, "namespaces"];
    if doc.resolve_pointer(0, &pointer(&AT)).is_err() {
        if permits.namespaces.is_empty() {
            return Ok(());
        }
        // yaml-rt-core puts a one-line block fragment on its key's line,
        // where it does not parse: one namespace, open, goes in flow style.
        let block = permits::namespaces(&permits.namespaces);
        let fragment = match block.lines().count() {
            1 => format!("{{{}}}", block.trim_end()),
            _ => block,
        };
        return add(doc, &AT, &fragment);
    }
    for name in held.namespaces.keys() {
        if !permits.namespaces.contains_key(name) {
            remove(doc, &[KEY, "namespaces", name])?;
        }
    }
    for (name, permit) in &permits.namespaces {
        let at = [KEY, "namespaces", name.as_str()];
        match (held.namespaces.get(name), permit) {
            (None, _) => add(doc, &at, &permits::value(permit))?,
            (Some(was), now) if was == now => {}
            (Some(NamespacePermit::Closed(was)), NamespacePermit::Closed(now)) => {
                listed(doc, &at, was, now)?;
            }
            (Some(_), now) => set_value(doc, &at, &permits::value(now))?,
        }
    }
    Ok(())
}

/// The edits to the list at `at` from `held` to `keys`: each key gone
/// removed, each new one added at the end. The list is added whole, as a
/// flow list, when there is none.
fn listed(
    doc: &mut YamlDoc,
    at: &[&str],
    held: &BTreeSet<String>,
    keys: &BTreeSet<String>,
) -> Result<(), String> {
    if held == keys {
        return Ok(());
    }
    if doc.resolve_pointer(0, &pointer(at)).is_err() {
        return add(doc, at, &permits::flow(keys));
    }
    for key in held.difference(keys) {
        let list = doc
            .resolve_pointer(0, &pointer(at))
            .map_err(|e| e.to_string())?;
        let index = doc
            .sequence_items(list)
            .position(|item| doc.scalar_value(item).is_ok_and(|v| v == key.as_str()))
            .ok_or_else(|| format!("{key} is not in the list at {}", pointer(at).as_str()))?;
        let item = index.to_string();
        let mut path = at.to_vec();
        path.push(&item);
        remove(doc, &path)?;
    }
    for key in keys.difference(held) {
        let mut path = at.to_vec();
        path.push("-");
        add(doc, &path, &permits::scalar(key))?;
    }
    Ok(())
}

/// `fragment` added at `at`.
fn add(doc: &mut YamlDoc, at: &[&str], fragment: &str) -> Result<(), String> {
    let fragment = YamlFragment::parse(fragment).map_err(|e| e.to_string())?;
    doc.add_at(0, &pointer(at), &fragment)
        .map_err(|e| e.to_string())
}

/// The value at `at` removed, the blank lines after its collection kept:
/// yaml-rt-core takes those after a block list's last item with it.
fn remove(doc: &mut YamlDoc, at: &[&str]) -> Result<(), String> {
    let parent = &at[..at.len() - 1];
    let blank_before = blank_after(doc, parent)?;
    doc.remove_at(0, &pointer(at)).map_err(|e| e.to_string())?;
    let lost = blank_before.saturating_sub(blank_after(doc, parent)?);
    if lost != 0 {
        let node = node(doc, parent)?;
        let text = slice(doc, node)?;
        doc.replace_node_text(node, format!("{text}{}", "\n".repeat(lost)))
            .map_err(|e| e.to_string())?;
        doc.commit_edits().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// How many blank lines follow the line the value at `at` ends on.
fn blank_after(doc: &YamlDoc, at: &[&str]) -> Result<usize, String> {
    let end = doc.node(node(doc, at)?).ok_or("no node")?.span().end as usize;
    let after = &doc.as_source()[end..];
    let Some((_, rest)) = after.split_once('\n') else {
        return Ok(0);
    };
    let mut lines = rest.split('\n').collect::<Vec<_>>();
    // The text after the last line break is no line of its own.
    lines.pop();
    Ok(lines
        .iter()
        .take_while(|line| line.trim().is_empty())
        .count())
}

/// The value at `at` spelled `text` in place, the comment beside it kept:
/// a plain scalar's spelling replaced, a flow collection replaced, and a
/// block collection's whole entry rewritten as `<key>: <text>`.
fn set_value(doc: &mut YamlDoc, at: &[&str], text: &str) -> Result<(), String> {
    let value = node(doc, at)?;
    let kind = doc.node(value).ok_or("no node")?.kind();
    let replacement = match kind {
        NodeKind::Scalar => {
            let whole = doc.node(value).ok_or("no node")?.span();
            let spelled = slice(doc, value)?;
            match doc
                .borrowable_scalar_span(value)
                .map_err(|e| e.to_string())?
            {
                // Only the spelling: what is after it (the comment) stays.
                Some(span) => {
                    let start = (span.start - whole.start) as usize;
                    let end = (span.end - whole.start) as usize;
                    format!("{}{text}{}", &spelled[..start], &spelled[end..])
                }
                None => text.to_string(),
            }
        }
        NodeKind::BlockMapping | NodeKind::BlockSequence => {
            let entry = entry(doc, at, value)?;
            let written = slice(doc, entry)?;
            let indent: String = written.chars().take_while(|c| *c == ' ').collect();
            let key = at.last().ok_or("no key")?;
            let rewritten = format!("{indent}{}: {text}", permits::scalar(key));
            doc.replace_node_text(entry, rewritten)
                .map_err(|e| e.to_string())?;
            return doc.commit_edits().map_err(|e| e.to_string());
        }
        _ => text.to_string(),
    };
    doc.replace_node_text(value, replacement)
        .map_err(|e| e.to_string())?;
    doc.commit_edits().map_err(|e| e.to_string())
}

/// The mapping entry holding `value`, the value at `at`.
fn entry(doc: &YamlDoc, at: &[&str], value: NodeId) -> Result<NodeId, String> {
    let mapping = node(doc, &at[..at.len() - 1])?;
    let key = doc
        .mapping_entries(mapping)
        .find(|(_, v)| *v == value)
        .map(|(k, _)| k)
        .ok_or("no entry")?;
    doc.node(key)
        .and_then(|k| k.parent())
        .filter(|e| {
            doc.node(*e)
                .is_some_and(|n| n.kind() == NodeKind::MappingEntry)
        })
        .ok_or_else(|| "no entry".to_string())
}

/// The node at `at`.
fn node(doc: &YamlDoc, at: &[&str]) -> Result<NodeId, String> {
    doc.resolve_pointer(0, &pointer(at))
        .map_err(|e| e.to_string())
}

/// `node`'s source text.
fn slice(doc: &YamlDoc, node: NodeId) -> Result<String, String> {
    let span = doc.node(node).ok_or("no node")?.span();
    Ok(doc.source().slice(span).to_string())
}

/// The JSON Pointer to `at`'s keys.
fn pointer(at: &[&str]) -> JsonPointer {
    let escaped: String = at
        .iter()
        .map(|token| format!("/{}", token.replace('~', "~0").replace('/', "~1")))
        .collect();
    JsonPointer::parse(&escaped).expect("an escaped pointer parses")
}

/// `source`, ending in a line break unless it is empty.
fn terminated(source: &str) -> String {
    match source.is_empty() || source.ends_with('\n') {
        true => source.to_string(),
        false => format!("{source}\n"),
    }
}

/// The block, written whole: `tag_permits:` and what it maps to.
fn written(permits: &TagPermits) -> String {
    let block = permits::block(permits);
    if block == "{}" {
        return format!("{KEY}: {{}}\n");
    }
    format!("{KEY}:\n{}", permits::indented(&block))
}

#[cfg(test)]
#[path = "edit.test.rs"]
mod tests;
