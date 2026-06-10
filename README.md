# taskloom

Per-project task tracking for AI coding agents, as a standalone binary: a CLI
over an append-only task log plus an MCP server exposing the same operations
to agents. Works on its own with any MCP-capable agent; pairs naturally with
[ctxloom](https://github.com/ctxloom/ctxloom) for session-aware provenance.

## Install

```bash
# macOS
brew install ctxloom/tap/taskloom

# Go
go install github.com/ctxloom/taskloom/cmd/taskloom@latest

# Or download a release archive (linux/darwin/windows, amd64/arm64):
# https://github.com/ctxloom/taskloom/releases
```

The [ctxloom install script](https://github.com/ctxloom/ctxloom) installs
taskloom alongside ctxloom by default.

## Register with your agent

```bash
taskloom manage install            # every backend present, user-level
taskloom manage install --engine claude-code
taskloom manage install --project  # this project's config instead
taskloom manage status             # where am I registered?
taskloom manage uninstall
```

Supported backends: Claude Code (`.mcp.json` / `~/.claude.json`), Gemini CLI
(`.gemini/settings.json`), Codex (`.codex/config.toml`). Registration merges
one `taskloom` server entry and preserves everything else in the file.

ctxloom users don't need this: ctxloom's embedded taskloom bundle registers
the server automatically when the binary is on PATH.

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
taskloom list [--status S]... [--term T] [--all] [--json]
taskloom add <text> [--status S] [--trigger T]
taskloom status <harp-id> <status> [--trigger T]
taskloom edit <harp-id> <text>
taskloom summary
taskloom run [task-harp-id] [--no-start]   # launches `ctxloom run` on the task
taskloom mcp                               # MCP server on stdio
taskloom manage install|uninstall|status   # backend MCP registration
```

Every command takes `--project <id>` to override project resolution.

## MCP

`taskloom mcp` serves `task_list`, `task_add`, `task_set_status`, and
`task_edit` over stdio. The project and session resolve per call from the
environment or working directory, so one user-level registration serves every
project.

## ctxloom integration (optional)

`ctxloom run` exports `CTXLOOM_PROJECT_ID` and `CTXLOOM_SESSION_HARP` into the
session environment; both the CLI and the MCP tools read them so tasks are
filed under the right project and stamped with the originating session.
`CTXLOOM_ROOT` overrides working-directory resolution the same way it does for
ctxloom itself.

`taskloom run` is the one ctxloom-coupled command: it shells out to
`ctxloom run` to spin a task into its own agent session. Everything else works
without ctxloom installed.

## Build

```
just build   # bin/taskloom
just check   # vet + race tests
```
