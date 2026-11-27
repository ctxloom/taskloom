use super::shell_word;

#[test]
fn test_a_word_is_quoted_only_where_a_shell_needs_it() {
    assert_eq!(
        shell_word("@terminal,a%b+c-d.e/f:g=h_i09"),
        "@terminal,a%b+c-d.e/f:g=h_i09"
    );
    assert_eq!(shell_word("To Do"), "'To Do'");
    assert_eq!(shell_word("level<=2"), "'level<=2'");
    assert_eq!(shell_word("it's"), r"'it'\''s'");
    assert_eq!(shell_word(""), "''");
    assert_eq!(
        shell_word("caf\u{e9}"),
        "'caf\u{e9}'",
        "ASCII alone is bare"
    );
}
