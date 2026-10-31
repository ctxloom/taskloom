//! What taskloom's loadout is: the reference document (`go/loadout.yaml`,
//! which `scripts/go-companion-fixtures.sh` regenerates from its source),
//! with the factual changes the human reviewed and approved applied.
//!
//! Each change is an exact `(old, new)` pair on the fixture's raw bytes, and
//! each `old` must occur exactly once: if the source rewords a sentence a
//! pair changes, the pair no longer matches and the parity tests fail until
//! the pair is reviewed again; if the source changes any other text, the
//! expected text changes and the byte comparison fails, as it would with
//! no pairs at all. The numbers are the review's (the port plan's Decided:
//! "The prose Rust taskloom serves"; 12 on, the owner's ruling of
//! 2026-10-30 that taskloom serves no MCP). The install placeholder awaits
//! the human.

/// The reference loadout document, reviewed.
pub fn loadout() -> String {
    applied(include_str!("go/loadout.yaml"), LOADOUT)
}

pub const LOADOUT: &[(&str, &str)] = &[
    // 5. run.description: built from the Rust repository; the SQLite store.
    (
        "(built from github.com/ctxloom/ctxloom, cmd/taskloom). Tasks live in a per-project append-only log\n    \
         at ~/.ctxloom/tasks/<project-id>.jsonl keyed by harp IDs",
        "(built from github.com/ctxloom/taskloom, crates/root/bin/taskloom). Tasks live in a per-project append-only log\n    \
         at ~/.ctxloom/projects/<project-id>/state.db keyed by harp IDs",
    ),
    // 6. run.installation: Rust has no install channel yet.
    (
        "    The ctxloom install script and `brew install ctxloom/tap/taskloom` ship the\n    \
         taskloom binary (fallback: go install github.com/ctxloom/ctxloom/cmd/taskloom@latest).\n",
        "    \u{ab}HUMAN: how the Rust taskloom binary is installed; it has no install channel yet.\u{bb}\n",
    ),
    // 9. The fragment: the SQLite store.
    (
        "(~/.ctxloom/tasks/<project-id>.jsonl) and are keyed by harp IDs",
        "(~/.ctxloom/projects/<project-id>/state.db) and are keyed by harp IDs",
    ),
    // 10. The fragment: the formats Rust's --format accepts.
    (
        "`--format` also accepts yaml, toml, text, markdown).",
        "`--format` also accepts text).",
    ),
    // 12. CLI only (owner ruling, 2026-10-30): taskloom serves no MCP, so the
    // bundle's version is a major one.
    (
        "  version: 1.2.0\n",
        "  version: 2.0.0\n",
    ),
    // 13. CLI only: run.description.
    (
        "through the task MCP tools served by `taskloom mcp` (registered by this\n    \
         bundle) or the `taskloom` CLI.",
        "through the `taskloom` CLI alone (it serves no MCP).",
    ),
    // 14. CLI only: run.notes ships no MCP server.
    (
        "    This bundle ships two ctxloom hooks and one MCP server:\n",
        "    This bundle ships two ctxloom hooks:\n",
    ),
    // 15. CLI only: run.notes' MCP server item.
    (
        "\n      \
         3. MCP server `taskloom` \u{2192} `taskloom mcp`\n         \
         Serves task_list / task_add / task_set_status / task_edit over\n         \
         stdio from the standalone taskloom binary. Requires `taskloom` on\n         \
         PATH; a missing binary degrades to no task tools, never a blocked\n         \
         startup.\n\
         \n",
        "\n",
    ),
    // 16. CLI only: run.installation delivers no MCP server.
    (
        "(it delivers the hooks and MCP server\n",
        "(it delivers the hooks\n",
    ),
    // 17. CLI only: no `manage` to register a server with.
    (
        "Without ctxloom, the binary\n    \
         self-registers: `taskloom manage install`.\n",
        "Without ctxloom, put\n    \
         `taskloom` on PATH: the CLI needs no registering.\n",
    ),
    // 18. CLI only: no mcp block (it held notes 7 and 8 of the review).
    (
        "  mcp:\n    \
         taskloom:\n      \
         command: taskloom\n      \
         args:\n        \
         - mcp\n      \
         notes: |\n        \
         The standalone task-store MCP server (built from github.com/ctxloom/ctxloom,\n        \
         cmd/taskloom). The\n        \
         project and session resolve per call from CTXLOOM_PROJECT_ID /\n        \
         CTXLOOM_SESSION_HARP (exported by ctxloom run) or the working\n        \
         directory.\n      \
         installation: brew install ctxloom/tap/taskloom\n\
         \n",
        "",
    ),
    // 19. CLI only: the fragment's MCP tools become the CLI, with the
    // MCP instructions' rules an agent still needs.
    (
        "        ## MCP tools (served by `taskloom mcp`)\n\
         \n        \
         - `task_list({statuses?, term?, include_completed?, include_summary?})`\n          \
         \u{2014} list/filter tasks. Set `include_summary: true` to also get\n          \
         per-status counts plus the in-progress harp IDs.\n        \
         - `task_add({text, status?, trigger?})` \u{2014} add a task with a fresh\n          \
         harp ID. Default status is `\"To Do\"`; `\"Deferred\"` requires a\n          \
         `trigger` (the condition that should revive it).\n        \
         - `task_set_status({harp_id, status, trigger?})` \u{2014} move a task\n          \
         between statuses.\n        \
         - `task_edit({harp_id, text})` \u{2014} replace a task's text in place.\n\
         \n        \
         Tasks are created and updated only through these tools (or the\n        \
         `taskloom` CLI). The harp ID appears in `task_list` output so you can\n        \
         reference a specific task in later calls.\n\
         \n",
        "        ## The CLI is the only surface\n\
         \n        \
         taskloom serves no MCP. Read with `list` (`--term`, `--tag-query`,\n        \
         `--status`, `--all`, `--compact`, `--limit`), `show <harp>...`,\n        \
         `summary`, `tags`, `permits`, `schema`; write with `add <text>`\n        \
         (`--tag`, `--status`, `--trigger`), `edit <harp> <text>`, `status\n        \
         <harp> <status>` and `tag <harp> --add/--remove`. `<command> --help`\n        \
         is the reference. Echo a task's harp back when you name it later.\n\
         \n        \
         - `--tag-query` is POSTFIX: tags first, operator after\n          \
         (`human:decision/area:cli/and`, `human:decision/not`).\n        \
         - Enumerate with `--compact` and/or `--limit`: a bare `list --json`\n          \
         returns every task's whole body.\n        \
         - A command's stderr may carry DEAD LETTER lines: each almost\n          \
         certainly marks a taskloom bug, so show them to the user.\n        \
         - Never read the store (`state.db`) directly: it is an append-only\n          \
         log, and only the CLI folds it to current state.\n\
         \n",
    ),
    // 20. CLI only: no second surface to share JSON keys with.
    (
        "        `Task`'s JSON tags are a cross-surface contract: the CLI and the MCP tools\n        \
         emit the same snake_case keys, so one jq filter works against either.\n\
         \n",
        "",
    ),
    // 21. CLI only: no MCP parameter names to differ from.
    (
        "        Note the CLI flag names differ from the MCP parameter names: `--status`\n        \
         (singular) and `--all`, not `statuses`/`include_completed`. A mistyped flag",
        "        `--status` is singular (repeat it), and `--all` is what shows finished\n        \
         and Deferred tasks. A mistyped flag",
    ),
    // 22. Record only what is still to do (the MCP instructions' rule).
    (
        "If it satisfies a task only in part, edit\n        \
         the task to record what is now done and what remains, instead of\n        \
         leaving it whole and letting the next person redo the finished half.",
        "If it satisfies a task only in part, edit\n        \
         the task down to what REMAINS: record only what is still to do. A\n        \
         task carrying its own finished half reads as work never started.",
    ),
    // 23. The first line is the subject (the MCP instructions' rule).
    (
        "        A task earns its length with judgment a later reader cannot recompute.\n",
        "        The first line of a task's text is its SUBJECT (what the task IS, in\n        \
         ~80 characters: lists show only that line); provenance goes on a\n        \
         later line.\n\
         \n        \
         A task earns its length with judgment a later reader cannot recompute.\n",
    ),
    // 11. The fragment: tags, read through the CLI (`schema`, `tags`,
    // `permits`), and tag permits, which only Rust has (the settled tag
    // vocabulary, session inept-sixth-stick, 2026-10-27); the owner's
    // advisory on permitting (2026-10-30), naming every command that
    // changes the store's tag rules now that they exist
    // (red/self-hosted-schema).
    (
        "        this paragraph is the reason.\n\
         \n",
        "        this paragraph is the reason.\n\
         \n        \
         Tags are `(namespace:)key(=value)`. Before choosing one, read\n        \
         `taskloom schema --json` (enums, ranges and scalar targets, enforced\n        \
         on write), `taskloom tags --json` (what is in use) and `taskloom\n        \
         permits --json`. Where the project declares tag permits, a tag they\n        \
         do not permit is REFUSED on write: choose a permitted tag that says\n        \
         the same thing, or propose the one you need to the human. Never coin\n        \
         a near-twin that happens to pass.\n\
         \n        \
         Agents must not run `taskloom schema declare`, `taskloom permit`,\n        \
         `taskloom revoke` or `taskloom schema import-config` without\n        \
         explicit permission from the human for that specific change \u{2014} they\n        \
         change the store's tag rules, agents can widen them and frequently\n        \
         will; ask first. Reading the rules (`taskloom schema`, `taskloom\n        \
         schema export`, `taskloom schema check`) needs no permission.\n\
         \n",
    ),
];

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
