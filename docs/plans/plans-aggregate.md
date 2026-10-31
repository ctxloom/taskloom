---
sessions:
  - inept-sixth-stick
---

# Plans in taskloom (red, for review)

Rulings of 2026-10-30; tests in `crates/*/*/plan*`.

**Entity.** `plan`: one aggregate per plan, keyed by a minted harp id. It
holds title, status, path, sha-256 content hash, origin session, `governs`
(tasks), `supersedes` (plans) and `superseded_by`.

**Status** (FLAG): draft, active, superseded, done, abandoned. Allowed:
draft to active or abandoned; active to done or abandoned; done or
abandoned back to active. Nothing returns to draft. Only a supersede link
sets superseded, and removing it reinstates active. Add starts draft or
active.

**Events.** PlanAdded, StatusChanged{from,to}, Governed, Ungoverned,
Superseded, Unsuperseded, MarkedSuperseded{by} and Reinstated (on the
target), Rehashed. The aggregate refuses duplicate or absent links,
self-supersede, and an unchanged status or hash. The application checks
that a linked task or plan exists and refuses supersede cycles, reading
those streams first (FLAG: concurrent supersedes can race). A supersede is
two writes, one per stream. No order crosses streams.

**Files.** `plan add` writes `<work-root>/.taskloom/plans/<id>.plan.md`,
committed with the repo. `--path` registers a `.plan.md` elsewhere in the
tree, stored normalized and work-root-relative; paths outside the root are
refused. `plan check` reports `missing`, `changed` (hash), `unregistered`,
`missing task`, and `external` (advisory, exit 0). `check --accept <id>`
records the new hash (FLAG: every edit needs one).

**CLI** (`--json` everywhere): `plan add --title T [--path P] [--status S]
[--governs TASK]...`, `plan list [--status S]... [--governs TASK]`, `plan
show <id|path>`, `plan status <id> <S>`, `plan link|unlink <id> (--governs
TASK... | --supersedes PLAN)`, `plan check [--accept ID]`, `plan import
[--dry-run]`. List JSON keeps VS Code's `path` (absolute), `name`, `title`,
`session` (origin session) and `sessions` (frontmatter, else `[]`), and adds
`id`, `rel_path`, `status`, `content_hash` and the links. `plan show` off a
terminal without `--json` writes raw markdown (FLAG: an exception VS Code
needs).

**Migration.** `plan import` groups `ctxloom:plan` values by resolved file.
Each readable `.plan.md` becomes one active plan that governs its tasks;
its title comes from frontmatter, then the heading, then the name. A file
outside the root is registered by absolute path (FLAG: alternatively, copy
it in). The tags it migrates are removed. Dead paths and non-plan files are
reported and their tags kept, and the command exits 1 (FLAG). Afterwards
`ctxloom:plan` is retired: tag admission refuses it (FLAG). Reruns add
nothing.

**Deferred.** The proto and a `PlanAggregate` on angzarr-embedded, an async
kernel-backed service, the ports in `core/ports`, diagnostics and catalogs,
mounting the commands in the binary, a `plans:` line in `taskloom show`,
and Windows paths.
