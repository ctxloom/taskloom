//! Names from harp-core: adjectives and a noun, joined by '-'. harp-core's
//! one word list is harp's "long" group (1269 adjectives, 4396 nouns), so a
//! task id is a short harp name.

use harp_core::{generate_name_with_options, generate_name_with_options_and_seed, NameOptions};
use ports::{Mint, Names};

/// [`Names`] drawn at random from harp-core's word lists.
#[derive(Clone, Copy, Debug, Default)]
pub struct HarpNames;

/// `components` words, adjectives then a noun, joined by '-'.
fn options(components: u8) -> NameOptions {
    NameOptions {
        components,
        max_element_length: None,
        separator: "-".to_string(),
    }
}

/// A random name of `components` words: adjectives, then a noun.
fn name(components: u8) -> String {
    generate_name_with_options(&options(components))
        .expect("2 and 3 words, joined by '-', are valid")
}

/// The seed `request_id` derives: FNV-1a, 64-bit, of its UTF-8 bytes. Fixed:
/// a change re-derives every key's harp, so a retry across it would add its
/// task again.
fn seed(request_id: &str) -> u64 {
    request_id
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

impl Names for HarpNames {
    /// Adjective-noun ("swift-amber").
    fn task_id(&mut self) -> String {
        name(2)
    }

    /// Adjective-noun, as [`Names::task_id`]'s: the first of harp's seeded
    /// index (scheme v1) for the key's [`seed`].
    fn task_id_for(&mut self, request_id: &str) -> String {
        generate_name_with_options_and_seed(&options(2), seed(request_id))
            .expect("2 words, joined by '-', are valid")
    }

    /// Adjective-adjective-noun ("jolly-mute-magma").
    fn project_id(&mut self) -> String {
        name(3)
    }
}

impl Mint for HarpNames {
    /// A plan's harp id: adjective-noun, as a task's ("brisk-otter").
    fn mint(&self) -> String {
        name(2)
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
