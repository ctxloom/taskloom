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

/// The reference resources, as resources/list answers them, reviewed.
pub fn resources() -> String {
    applied(include_str!("go/resources.json"), RESOURCES)
}

/// The reference tag-schema resource with nothing configured, reviewed.
pub fn tag_schema() -> String {
    applied(include_str!("go/tag-schema.json"), TAG_SCHEMA)
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
    // 9. A failed write's tool error carries the dead letters written while
    // it ran, after its envelope.
    (
        "and is refused as blank.",
        "and is refused as blank.\nA failed write's tool error may add, after its JSON envelope, a text block of DEAD LETTER lines: each almost certainly marks a bug in taskloom, so show them to the user.",
    ),
    // Tag permits (ruling 9): the agent is pointed at what the project
    // permits, and told that only a person permits more.
    (
        "read the `taskloom://tag-schema` and `taskloom://tag-vocabulary` resources before choosing one; a project may declare enums, numeric ranges, and scalar (at-most-one-value) targets that are enforced on write.",
        "read the `taskloom://tag-schema`, `taskloom://tag-vocabulary` and `taskloom://tag-permits` resources before choosing one; a project may declare enums, numeric ranges, and scalar (at-most-one-value) targets that are enforced on write, and may permit only the tags in its tag permits, refusing any other on write. Only a person permits a tag (`taskloom permit`): when one you need is refused, propose it to the user rather than coining a near-twin.",
    ),
];

/// Tag permits: the resource listing what a project permits, after Go's
/// two (ruling 9).
pub const RESOURCES: &[(&str, &str)] = &[(
    "    \"uri\": \"taskloom://tag-vocabulary\"\n  }\n]",
    "    \"uri\": \"taskloom://tag-vocabulary\"\n  },\n  {\n    \"description\": \"What the project's tag permits permit (its .taskloom/config.yaml's tag_permits; the MCP twin of `taskloom permits`): each permitted namespace, open (any key) or closed to its keys, the permitted flat tags, and schema_targets, the targets the tag schema declares, which are permitted without a permit. \\\"declared\\\": false means the project declares no permits, and every tag the schema admits is permitted. Where it declares them, a write adding a tag they do not permit is refused: choose a permitted tag, or propose the one you need to the user, who permits it with `taskloom permit`. No tool permits or revokes a tag.\",\n    \"mimeType\": \"application/json\",\n    \"name\": \"tag-permits\",\n    \"uri\": \"taskloom://tag-permits\"\n  }\n]",
)];

/// triage:blind-gate, declared by Rust's default schema alone (tag
/// permits, ruling 14: decay_fn reads it, so a permitting project must
/// admit it, and the enum refuses a typo on write).
pub const TAG_SCHEMA: &[(&str, &str)] = &[(
    "    {\n      \"target\": \"triage:blocks-release\",",
    "    {\n      \"target\": \"triage:blind-gate\",\n      \"enum\": [\n        \"unit\",\n        \"integration\",\n        \"acceptance\",\n        \"mutation\",\n        \"lint\",\n        \"ci\"\n      ]\n    },\n    {\n      \"target\": \"triage:blocks-release\",",
)];

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
    // 10. task_add: a trigger defers the task, and is refused with any
    // other status (fix L; tightened 2026-10-27).
    (
        r#""The revive condition for a Deferred task: a concrete description of what should bring it back (e.g. \"the v2 API ships\"). REQUIRED when status is \"Deferred\"; ignored otherwise.""#,
        r#""Revive condition. Adds the task Deferred, returning to \"To Do\" when cleared. Required with \"Deferred\"; refused with any other status.""#,
    ),
    // 11. task_set_status: a trigger alone defers the task, keeping its
    // status to return to; clear_trigger revives it; leaving Deferred by a
    // status clears the trigger; status is no longer required (fix L;
    // tightened 2026-10-27).
    (
        r#"Use \"Deferred\" with a `trigger` to park a task on a named revive condition (it then hides from the active list until the condition fires).""#,
        r#"A `trigger` (alone, or with \"Deferred\") defers the task until its condition fires, keeping the status it held. `clear_trigger: true` returns it to that status, or to `status`. Moving a Deferred task to any other status clears its trigger.""#,
    ),
    (
        r#""properties": {
        "harp_id": {
          "description": "The task's harp ID (e.g. \"swift-amber-falcon\") as returned by task_list or task_add.",
          "type": "string"
        },
        "status": {
          "description": "Target status. Standard values: \"In Progress\", \"To Do\", \"Deferred\", \"Done\", \"Archived\".",
          "type": "string"
        },
        "trigger": {
          "description": "The revive condition when moving to \"Deferred\": what should bring the task back. REQUIRED for \"Deferred\" unless the task already carries a trigger (then it is preserved). Ignored for other statuses.",
          "type": "string"
        }
      },
      "required": [
        "harp_id",
        "status"
      ],"#,
        r#""properties": {
        "clear_trigger": {
          "description": "true revives a Deferred task to its prior status, or to `status`. Not with `trigger`.",
          "type": "boolean"
        },
        "harp_id": {
          "description": "The task's harp ID (e.g. \"swift-amber-falcon\") as returned by task_list or task_add.",
          "type": "string"
        },
        "status": {
          "description": "Target status. Standard values: \"In Progress\", \"To Do\", \"Deferred\", \"Done\", \"Archived\". Omit it to only set or clear the trigger.",
          "type": "string"
        },
        "trigger": {
          "description": "Revive condition. Defers the task; give it alone or with \"Deferred\". Never blank.",
          "type": "string"
        }
      },
      "required": [
        "harp_id"
      ],"#,
    ),
    // 12. task_set_status, task_tag and task_edit: a request's key, so a
    // retried request applies once (2026-10-27). task_add offers none until
    // fix O pins the harp an add mints by it; one sent is refused
    // (tasks-application-add-request-id-not-yet).
    (
        r#"        "status": {
          "description": "Target status."#,
        r#"        "request_id": {
          "description": "An id you choose for this request; resend the same one when retrying it, and a request already applied is not applied again.",
          "type": "string"
        },
        "status": {
          "description": "Target status."#,
    ),
    (
        r#"least one of add/remove is required; a tag named in both ends up removed.",
          "items": {
            "type": "string"
          },
          "type": [
            "null",
            "array"
          ]
        }
      },"#,
        r#"least one of add/remove is required; a tag named in both ends up removed.",
          "items": {
            "type": "string"
          },
          "type": [
            "null",
            "array"
          ]
        },
        "request_id": {
          "description": "An id you choose for this request; resend the same one when retrying it, and a request already applied is not applied again.",
          "type": "string"
        }
      },"#,
    ),
    (
        r#"        "text": {
          "description": "The full replacement text for the task."#,
        r#"        "request_id": {
          "description": "An id you choose for this request; resend the same one when retrying it, and a request already applied is not applied again.",
          "type": "string"
        },
        "text": {
          "description": "The full replacement text for the task."#,
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
