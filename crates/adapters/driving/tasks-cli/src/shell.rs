//! Quoting a word for the shell a printed command is pasted into.

/// `word` as a POSIX shell reads it back: bare when every character is one
/// a shell passes through literally, single-quoted otherwise, so `To Do`
/// or `triage:level<=2` survives being pasted.
pub(crate) fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@%+,-./:=_".contains(c));
    if plain {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[cfg(test)]
#[path = "shell.test.rs"]
mod tests;
