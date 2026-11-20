//! The watch's resume cursor: where a watcher is on each task stream.
//!
//! PROPOSAL (pending owner review): the cursor is opaque to a subscriber, a
//! string to keep and hand back as `--since`. It is base64url, unpadded, of
//! the JSON object `{"v":1,"project":"<id>","streams":{"<stream>":<next>}}`:
//!
//! - `v`, the cursor's version, which is the lines' too; a cursor of
//!   another version is refused naming the one supported ([`VERSION`]);
//! - `project`, whose streams these are, so a cursor is never replayed
//!   against another project's store;
//! - `streams`, each task stream by `tasks/<root>[@<edition>]` (the root a
//!   hyphenated UUID, the edition absent for the main timeline) to the next
//!   sequence the watcher has yet to see. A stream that is not there is
//!   read from its start.
//!
//! A cursor is checked on its own content ([`Cursor::parse`]) before any
//! state is read: whether it decodes, its version and its streams' names.
//! Whose it is ([`Cursor::check_project`]) needs the project resolved.

use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;
use serde::{Deserialize, Serialize};
use tasks_application::{Positions, StreamKey};
use uuid::Uuid;

use crate::WatchError;

/// The version of the cursor and of every line of the stream.
pub const VERSION: u64 = 1;

/// The domain whose streams are watched, as a stream's name begins.
const DOMAIN: &str = "tasks";

/// Unpadded on the way out, padded or not on the way in.
pub(crate) const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// A cursor as read: whose it is, and the next sequence of each stream.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cursor {
    pub project: String,
    pub positions: Positions,
}

/// The cursor's JSON.
#[derive(Serialize, Deserialize)]
struct Document {
    v: u64,
    project: String,
    streams: std::collections::BTreeMap<String, u64>,
}

/// A stream's name in a cursor and on a line.
pub fn name(stream: &StreamKey) -> String {
    match &stream.edition {
        None => format!("{DOMAIN}/{}", stream.root),
        Some(edition) => format!("{DOMAIN}/{}@{edition}", stream.root),
    }
}

/// The stream `text` names.
fn stream_named(text: &str) -> Option<StreamKey> {
    let rest = text.strip_prefix(DOMAIN)?.strip_prefix('/')?;
    let (root, edition) = match rest.split_once('@') {
        Some((root, edition)) => (root, Some(edition)),
        None => (rest, None),
    };
    if edition == Some("") {
        return None;
    }
    let root: Uuid = root.parse().ok()?;
    let edition = edition.map(str::to_string);
    Some(StreamKey { edition, root })
}

impl Cursor {
    /// `text` as a cursor, checked on its own content.
    pub fn parse(text: &str) -> Result<Cursor, WatchError> {
        let malformed = || WatchError::CursorMalformed;
        let bytes = BASE64.decode(text).map_err(|_| malformed())?;
        let json: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| malformed())?;
        let found = json.get("v").and_then(serde_json::Value::as_u64);
        match found {
            Some(VERSION) => {}
            Some(found) => {
                return Err(WatchError::CursorVersion {
                    found,
                    supported: VERSION,
                })
            }
            None => return Err(malformed()),
        }
        let document: Document = serde_json::from_value(json).map_err(|_| malformed())?;
        let mut positions = Positions::new();
        for (name, next) in document.streams {
            positions.insert(stream_named(&name).ok_or_else(malformed)?, next);
        }
        Ok(Cursor {
            project: document.project,
            positions,
        })
    }

    /// Refused unless the cursor is `project`'s.
    pub fn check_project(&self, project: &str) -> Result<(), WatchError> {
        match self.project == project {
            true => Ok(()),
            false => Err(WatchError::CursorForeign {
                cursor_project: self.project.clone(),
                project: project.to_string(),
            }),
        }
    }
}

/// The cursor of `project` at `positions`, as text.
pub fn encode(project: &str, positions: &Positions) -> String {
    let document = Document {
        v: VERSION,
        project: project.to_string(),
        streams: positions.iter().map(|(k, next)| (name(k), *next)).collect(),
    };
    let json = serde_json::to_vec(&document).expect("a cursor is JSON");
    BASE64.encode(json)
}

#[cfg(test)]
#[path = "cursor.test.rs"]
mod tests;
