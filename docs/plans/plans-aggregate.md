---
sessions:
  - inept-sixth-stick
---

# Plans in taskloom

Rulings of 2026-10-30, and the owner's review of the red tests the same
night (rulings a, b and c below). The code is in `crates/*/*/plan*` and
`proto/ctxloom/plans/v1/plans.proto`.

**Entity.** `plan`: one aggregate per plan (`PlanAggregate`, on
angzarr-embedded), keyed by a minted harp id, its stream in the project's
store beside the task streams. It holds title, status, path, sha-256
content hash, origin session, `governs` (tasks), `supersedes` (plans) and
`superseded_by`.

**Status.** draft, active, superseded, done, abandoned. Allowed: draft to
active or abandoned; active to done or abandoned; done or abandoned back to
active. Nothing returns to draft. Only a supersede link sets superseded,
and removing it reinstates active. Add starts draft or active; an imported
plan starts active.

**Events.** PlanAdded, PlanStatusChanged{from,to}, Governed, Ungoverned,
Superseded, Unsuperseded, MarkedSuperseded{by,from} and Reinstated (on the
target), Rehashed. The aggregate refuses duplicate or absent links,
self-supersede, an unchanged status or hash, and an add outside the stream
its harp derives. The application checks that a linked task or plan exists,
refuses supersede cycles and a target another plan supersedes, reading
those streams first, and decides each command at the head it read. A
supersede is two writes, one per stream; a link or unlink left half done is
finished by running it again. No order crosses streams. Still open:
concurrent supersedes of one target can both pass the check; the second
target write is refused and its first half is left for an unlink.

**Files.** `plan add` writes `<work-root>/.taskloom/plans/<id>.plan.md`,
committed with the repo. `--path` registers a `.plan.md` elsewhere in the
tree, stored normalized and work-root-relative; paths outside the root are
refused. The hash tracks edits (ruling a): `plan list`, `show` and `check`
read the current file and record `Rehashed` when it changed. `plan check`
reports only a recorded file that is `missing` and a `.taskloom/plans`
file no plan records (`unregistered`); either exits 1. There is no
`--accept`.

**CLI** (the shared `--format`, `--json`, `--project`): `plan add --title T
[--path P] [--status S] [--governs TASK]...`, `plan list [--status S]...
[--governs TASK]`, `plan show <id|path>`, `plan status <id> <S>`, `plan
link|unlink <id> (--governs TASK... | --supersedes PLAN)`, `plan check`,
`plan import [--dry-run]`. A status that is none, or a link naming
nothing, exits 2. List JSON keeps VS Code's `path` (absolute), `name`,
`title`, `session` (origin session) and `sessions` (frontmatter, else
`[]`), and adds `id`, `rel_path`, `status`, `content_hash` and the links.
`plan show` follows the CLI's convention (ruling c): JSON off a terminal
(the plan and its `content`); its text is the markdown, raw, which the VS
Code extension asks for with `--format text`. `taskloom show <task>` names
the plans governing the task on a `plans:` line (text only).

**Migration.** `plan import` groups `ctxloom:plan` values by resolved file.
Each readable `.plan.md` becomes one active plan that governs its tasks;
its title comes from frontmatter, then the heading, then the name. A file
outside the root is copied into `.taskloom/plans/<id>.plan.md` and the copy
registered, the original left as it was (ruling b). The tags it migrates
are removed. Dead paths and non-plan files are reported and their tags
kept, and the command exits 1. `ctxloom:plan` is retired: tag admission
refuses it (`plans::tag_is_retired`). Reruns add nothing.

**Not done.** Windows paths (a plan path is `/`-separated and the work
root is compared as text); a `plans` field in `taskloom show`'s JSON (its
schema is pinned); a `plan` reference in the loadout.
