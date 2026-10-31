//! The names' shape: so many words, from the right lists.

use harp_core::{ADJECTIVES, NOUNS};
use ports::Names;

use super::HarpNames;

/// `name`'s words, each checked against the list its place draws from.
fn assert_words(name: &str, adjectives: usize) {
    let words: Vec<&str> = name.split('-').collect();
    assert_eq!(words.len(), adjectives + 1, "{name}");
    for word in &words[..adjectives] {
        assert!(
            ADJECTIVES.contains(word),
            "{word} in {name} is an adjective"
        );
    }
    assert!(NOUNS.contains(&words[adjectives]), "{name} ends in a noun");
}

#[test]
fn test_a_task_id_is_an_adjective_and_a_noun() {
    for _ in 0..50 {
        assert_words(&HarpNames.task_id(), 1);
    }
}

#[test]
fn test_a_project_id_is_two_adjectives_and_a_noun() {
    for _ in 0..50 {
        assert_words(&HarpNames.project_id(), 2);
    }
}

#[test]
fn test_names_are_drawn_at_random() {
    let drawn: std::collections::BTreeSet<String> = (0..20).map(|_| HarpNames.task_id()).collect();
    assert!(drawn.len() > 1, "{drawn:?}");
}

#[test]
fn test_a_plan_id_is_an_adjective_and_a_noun_as_a_tasks() {
    use ports::Mint;
    for _ in 0..50 {
        assert_words(&HarpNames.mint(), 1);
    }
}

#[test]
fn test_a_keys_task_id_is_an_adjective_and_a_noun() {
    for key in ["r-1", "", "a much longer request id, with spaces"] {
        for position in 0..8 {
            assert_words(&HarpNames.task_id_for(key, position), 1);
        }
    }
}

#[test]
fn test_a_key_derives_the_same_task_id_every_time() {
    for position in 0..8 {
        assert_eq!(
            HarpNames.task_id_for("r-1", position),
            HarpNames.task_id_for("r-1", position)
        );
    }
}

#[test]
fn test_a_keys_positions_are_distinct_task_ids() {
    let order: std::collections::BTreeSet<String> =
        (0..8).map(|p| HarpNames.task_id_for("r-1", p)).collect();
    assert_eq!(order.len(), 8, "{order:?}");
}

// Fixed vectors: the derivation is what a retry relies on, across processes
// and releases. A change to the seed or harp's scheme fails them.
#[test]
fn test_a_keys_task_id_is_fixed() {
    assert_eq!(super::seed(""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(super::seed("a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(HarpNames.task_id_for("r-1", 0), "uncertain-marrow");
    assert_eq!(HarpNames.task_id_for("r-1", 1), "cuddly-scrambler");
}

#[test]
fn test_keys_derive_task_ids_spread_over_the_space() {
    let derived: std::collections::BTreeSet<String> = (0..50)
        .map(|n| HarpNames.task_id_for(&format!("r-{n}"), 0))
        .collect();
    assert!(derived.len() > 45, "{derived:?}");
}
