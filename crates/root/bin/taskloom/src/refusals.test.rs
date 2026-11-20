//! Every refusal a command line can reach, as the binary answers it: the
//! exit code, and the envelope's `code`, `args` and `message`, exactly as
//! they were before the refusals became typed messages with a business
//! status of their own. The status of a refusal is a proto option and may
//! change; what a person or the extension reads must not, so nothing here
//! names a status.

use serde_json::{json, Value};

use super::{one_document, Env};

/// One refusal: the arguments (`{harp}` is a task that exists), the exit
/// code, and the envelope.
struct Golden {
    args: &'static [&'static str],
    exit: i32,
    code: &'static str,
    envelope_args: Value,
    message: &'static str,
}

fn goldens() -> Vec<Golden> {
    let none = || json!({});
    vec![
        Golden {
            args: &["tasks", "add", " "],
            exit: 2,
            code: "tasks-text-required",
            envelope_args: none(),
            message: "a task needs text",
        },
        Golden {
            args: &["tasks", "add", "x", "--status", "Deferred"],
            exit: 2,
            code: "tasks-trigger-required",
            envelope_args: none(),
            message: "a Deferred task needs a trigger: the condition that should revive it",
        },
        Golden {
            args: &["tasks", "add", "x", "--status", "@open"],
            exit: 2,
            code: "tasks-status-reserved",
            envelope_args: json!({"status": "@open"}),
            message: "the status @open is not allowed: '@' is reserved for status classes",
        },
        Golden {
            args: &["tasks", "add", "x", "--status", "Done", "--trigger", "later"],
            exit: 2,
            code: "tasks-trigger-with-status",
            envelope_args: json!({"status": "Done", "defers": "Deferred"}),
            message: "a trigger defers the task to Deferred, so it cannot be given with the status Done: give the trigger alone, or the status alone",
        },
        Golden {
            args: &["tasks", "set", "{harp}", "--text", " "],
            exit: 2,
            code: "tasks-text-required",
            envelope_args: none(),
            message: "a task needs text",
        },
        Golden {
            args: &["tasks", "set", "{harp}", "--status", "@open"],
            exit: 2,
            code: "tasks-status-reserved",
            envelope_args: json!({"status": "@open"}),
            message: "the status @open is not allowed: '@' is reserved for status classes",
        },
        Golden {
            args: &["tasks", "set", "{harp}", "--status", "Done", "--trigger", "later"],
            exit: 2,
            code: "tasks-trigger-with-status",
            envelope_args: json!({"status": "Done", "defers": "Deferred"}),
            message: "a trigger defers the task to Deferred, so it cannot be given with the status Done: give the trigger alone, or the status alone",
        },
        Golden {
            args: &["tasks", "set", "{harp}", "--status", "Deferred"],
            exit: 2,
            code: "tasks-trigger-required",
            envelope_args: none(),
            message: "a Deferred task needs a trigger: the condition that should revive it",
        },
        Golden {
            args: &["tasks", "set", "no-such-task", "--status", "Done"],
            exit: 1,
            code: "tasks-no-task",
            envelope_args: none(),
            message: "there is no such task",
        },
        Golden {
            args: &["tasks", "tag", "remove", "no-such-task", "x"],
            exit: 1,
            code: "tasks-no-task",
            envelope_args: none(),
            message: "there is no such task",
        },
        Golden {
            args: &["manage", "schema", "declare", "--yes"],
            exit: 2,
            code: "tasks-schema-tag-required",
            envelope_args: none(),
            message: "a schema declaration needs at least one tag to add or remove",
        },
        Golden {
            args: &["manage", "schema", "declare", "--add", "area:cli", "--yes"],
            exit: 2,
            code: "tasks-schema-not-config",
            envelope_args: json!({"tag": "area:cli"}),
            message: "area:cli is not a rule: a schema declares only tagma.* and taskloom.* tags",
        },
    ]
}

#[test]
fn test_every_refusal_a_command_line_reaches_answers_its_code_args_message_and_exit_code() {
    let env = Env::new("refusals-harp");
    let harp = env.seed("Ship it")["harp_id"]
        .as_str()
        .expect("harp_id")
        .to_string();
    env.permit_every_tag();
    for golden in goldens() {
        let mut args: Vec<String> = golden
            .args
            .iter()
            .map(|arg| arg.replace("{harp}", &harp))
            .collect();
        args.extend(["--format".to_string(), "json".to_string()]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let ran = env.run(&args);
        assert_eq!(ran.code, golden.exit, "{args:?}: the exit code");
        assert_eq!(ran.stdout, "", "{args:?}: stdout is empty");
        let envelope = one_document(&ran.stderr);
        assert_eq!(
            envelope,
            json!({
                "code": golden.code,
                "args": golden.envelope_args,
                "message": golden.message,
            }),
            "{args:?}: the envelope"
        );
    }
}
