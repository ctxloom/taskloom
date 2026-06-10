# tasks

Per-project task tracking for ctxloom sessions, as a standalone binary: a CLI
over an append-only task log plus an MCP server exposing the same operations
to agents. Extracted from ctxloom (which now consumes this module only for its
`run --seed-task` integration).

## Model

- Tasks are keyed by short harp IDs (`swift-amber-falcon`) and live in a
  per-project append-only JSONL log at `~/.ctxloom/tasks/<project-id>.jsonl`.
  Current state is the fold of the log's events; identity is never reused.
- The project-id is a stable, path-independent identity resolved through two
  self-healing carriers: the gitignored in-tree marker
  `<project>/.ctxloom/project-id` and the home registry
  `~/.ctxloom/projects/index.yaml`. Moves re-point; copies fork.
- A `Deferred` task must carry a trigger — the condition that should revive it.
- Statuses: `In Progress`, `To Do`, `Deferred`, `Done`, `Archived` (free-form
  values are accepted).

## CLI

```
tasks list [--status S]... [--term T] [--all] [--json]
tasks add <text> [--status S] [--trigger T]
tasks status <harp-id> <status> [--trigger T]
tasks edit <harp-id> <text>
tasks summary
tasks run [task-harp-id] [--no-start]   # launches `ctxloom run` on the task
tasks mcp                               # MCP server on stdio
```

Every command takes `--project <id>` to override project resolution.

## Session integration

`ctxloom run` exports `CTXLOOM_PROJECT_ID` and `CTXLOOM_SESSION_HARP` into the
session environment; both the CLI and the MCP tools read them so tasks are
filed under the right project and stamped with the originating session.
`CTXLOOM_ROOT` overrides working-directory resolution the same way it does for
ctxloom itself.

## MCP

`tasks mcp` serves `task_list`, `task_add`, `task_set_status`, and `task_edit`
over stdio. Wire it into an agent profile as a stdio MCP server.

## Build

```
just build   # bin/tasks
just test
```
