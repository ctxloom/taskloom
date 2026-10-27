//! What taskloom's MCP text is: the reference text (`go/`, which
//! `scripts/go-companion-fixtures.sh` regenerates from its source), with
//! the factual changes the human reviewed and approved applied.
//!
//! Each change is an exact `(old, new)` pair on the fixture's raw bytes,
//! before any JSON parse, so a pair in `tools.json` matches its escapes
//! (`<` for `<`). Each `old` must occur exactly once: if the source
//! rewords a sentence a pair changes, the pair no longer matches and the
//! parity tests fail until the pair is reviewed again; if the source
//! changes any other text, the expected text changes and the byte
//! comparison fails, as it would with no pairs at all. The numbers are the
//! review's (the port plan's Decided: "The prose Rust taskloom serves").

/// The reference MCP server instructions, reviewed.
pub fn instructions() -> String {
    applied(include_str!("go/instructions.txt"), INSTRUCTIONS)
}

/// The reference tools, as tools/list answers them, reviewed.
pub fn tools() -> String {
    let tools = applied(include_str!("go/tools.json"), TOOLS);
    applied_to_each(&tools, EVERY_TOOL, 5)
}

pub const INSTRUCTIONS: &[(&str, &str)] = &[
    // 1. Rust's store is SQLite; it writes no jsonl.
    (
        "Never read the raw ~/.ctxloom/tasks/<project>.jsonl directly",
        "Never read the raw ~/.ctxloom/projects/<project>/state.db directly",
    ),
    // 8. An explicit empty string is a value, refused as blank.
    (
        "only these tools / the CLI dedup to current state.",
        "only these tools / the CLI dedup to current state.\nOmit optional arguments you don't mean to set. An explicit empty string is a value, and is refused as blank.",
    ),
];

pub const TOOLS: &[(&str, &str)] = &[
    // 2. task_list: repo homing is dropped, and no notice says so.
    (
        r#"pass global=true to aggregate every PRIVATELY-homed project instead (a repo-homed project -- homing: repo -- is registered nowhere global and is never included, even if it is the current project; the result's notice field always says so when global is true)."#,
        r#"pass global=true to aggregate every project instead."#,
    ),
    (
        r#"falls back to global too, with the same notice field explaining why."#,
        r#"falls back to global too, with the notice field explaining why."#,
    ),
    // 3. task_list's global parameter: the same.
    (
        r#"aggregate tasks across every PRIVATELY-homed project instead of just the current one."#,
        r#"aggregate tasks across every project instead of just the current one."#,
    ),
    (
        r#" A repo-homed project (homing: repo, its log checked into \u003crepo\u003e/.taskloom/tasks.jsonl) is registered nowhere global and is NEVER included, even if it is the current project -- every global listing's notice field says so.""#,
        r#"""#,
    ),
    // 6. task_add and task_tag: two different values for one scalar target
    // in one call are refused (tag-scalar-conflict).
    (
        r#"is rejected at write, as are the reserved words and/or/not and the tagma.* namespace.""#,
        r#"is rejected at write, as are the reserved words and/or/not and the tagma.* namespace. Giving two DIFFERENT values for the same scalar target in one call is refused (tag-scalar-conflict); give one.""#,
    ),
    (
        r#"Filter task_list with tag_query using the resulting tags.""#,
        r#"Filter task_list with tag_query using the resulting tags. Within one call, two different `add` values for the same scalar target are refused (tag-scalar-conflict); the retraction rule above applies across calls, not within one.""#,
    ),
    // 7. task_list's limit: left out is no cap; 0 is refused.
    (
        "0 (default) means no cap.",
        "Omit it for no cap; 0 is refused.",
    ),
];

/// Changes made to each of the five tools' output schemas alike.
pub const EVERY_TOOL: &[(&str, &str)] = &[
    // 5. No result names its store (`path`): where a project's store lies
    // is the binary's business, not the caller's.
    (
        "        \"path\": {\n          \"type\": \"string\"\n        },\n",
        "",
    ),
    (
        "      \"required\": [\n        \"path\",\n",
        "      \"required\": [\n",
    ),
];

/// `text` with each pair applied to every occurrence of its `old`, which
/// is found exactly `times` times.
pub fn applied_to_each(text: &str, pairs: &[(&str, &str)], times: usize) -> String {
    let mut text = text.to_string();
    for (old, new) in pairs {
        let found = text.matches(old).count();
        assert!(
            found == times,
            "a reviewed change's old text occurs {found} times, not {times}, in Go's text: \
             Go has changed it, so review the change again: {old:?}"
        );
        text = text.replace(old, new);
    }
    text
}

/// `go` with each pair applied, each `old` found exactly once.
pub fn applied(go: &str, pairs: &[(&str, &str)]) -> String {
    let mut text = go.to_string();
    for (old, new) in pairs {
        let found = text.matches(old).count();
        assert!(
            found == 1,
            "a reviewed change's old text occurs {found} times, not once, in Go's text: \
             Go has changed it, so review the change again: {old:?}"
        );
        text = text.replacen(old, new, 1);
    }
    text
}
