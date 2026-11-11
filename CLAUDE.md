# Introduction
Note that this is a port of ctxloom into rust from golang, piece by piece, with architect review of each piece as its ported.

We're starting with the highest risk pieces first and the smallest complete units that can derisk early.

# Working rules

## Tests go in `*.test.rs` files beside the code they test

A source file `foo.rs` keeps its tests in `foo.test.rs` in the same
directory, included at the end of `foo.rs`:

```rust
#[cfg(test)]
#[path = "foo.test.rs"]
mod tests;
```

Not inline `mod tests { ... }`, and not a `tests/` directory. The tests stay
beside the code they test, but out of the source file, so reading the code
does not mean reading its tests too.

## Layout: a crate's directory is its role

Every crate lives at `crates/<ring>/<role>/<crate>`. The ring and role are
the architecture; there is no other table of crates.

- `core/wire`: generated prost types.
- `core/domain`: aggregates (decide, apply): the write side.
- `core/read-model`: projectors: the query side.
- `core/ports`: traits the core needs from outside.
- `core/application`: command and query handlers (a context's use
  cases), and `kernel-runner`, the one way every context runs a command
  through the kernel and catches a project's list up (`ProjectLists`).
- `adapters/driven`: port implementations (stores, clock, git).
- `adapters/driving`: the CLI (and later gRPC): translation only.
  taskloom serves no MCP: agents and people alike use the CLI.
- `root/lib`: the composition root's wiring as a library (`tasks-lib`):
  the use cases over the default adapters, which the binary, any later
  server and ctxloom build from, so no frontend wires an adapter.
- `root/bin`: binaries, the composition root.

This repository is taskloom: the `taskloom` binary and the crates it
composes. ctxloom depends on those crates by git (pinned by commit), so a
crate here is named for its domain or role, and the binaries under
`root/bin` compose the same core, application and adapters. Below
`root/bin`, a crate is named for its domain or role, never for a binary.

### A context's application crate (the template ctxloom-rs copies)

A context's application crate is its use cases, each a unit of its own
over only the ports it uses, and one `Service` composing them.
tasks-application is the example:

- **A unit per use case**, in a module of its own, beside its request and
  answer types: `add.rs` (`Adding`: the project, the kernel and the harp
  mint), `change.rs` (`Changing`: a status change, deferring and reviving
  among them, an edit, a retag), `read.rs` (`Reading`: no kernel),
  `prune.rs` (`Pruner`), `permit.rs` (`Permitting`), `watch.rs`
  (`Watching`: the identity, the change token and a read of the task
  streams from per-stream positions), `schema.rs`
  (the schema stream and its reads), and `import.rs` (`Importer`, a
  service of its own: only it holds the JSONL logs). A unit borrows its
  ports from the service for one call; its `impl` names only the bounds
  it needs.
- **Shared collaborators**, not a shared god type: `project.rs`
  (`Projects`: resolve, open, read the store's rules, refuse a moved
  key), `write.rs` (`Writes`: `Projects` and kernel-runner's `Runner`,
  with seeding, declaring and a commit answered with the task it left),
  `task.rs` (a task's stream), `lists.rs` (the task list caught up per
  project, through kernel-runner's `ProjectLists`), and pure rules
  (`rules.rs`, `tags.rs`, `query.rs`, `priority.rs`, `calendar.rs`).
- **`service.rs`**: the `Service`, which owns the ports once and
  implements the traits a driving adapter calls (`TaskService`,
  `PruneService`, ...) by lending each call's unit its share
  (`Parts`). lib.rs is the crate's map and its published names.
- **One crate per context**, not per use case: a diagnostic's code is
  prefixed with its crate's name and is public (the CLI's JSON carries
  it), so the use cases share the context's `AppError` and codes.

plans-application is not an aggregate's crate: plans are files, nothing
in the store (the owner's ruling, 2026-11-03). Its `front.rs` finds a
file's frontmatter between the `---` fences, reads it and edits it in
place through angzarr-yaml's editor (the body, comments, ordering and
unknown keys untouched; a file with none gets a fence block; the editor
reads each edit back); `plan.rs` is a plan as read (with `superseded`
derived across files, never written); `watch.rs` what differs between two
snapshots of the files; `service.rs` the `PlanService`, the use cases over
the `PlanFiles`, `Tasks` (a task exists; the store's tag rules), `Mint`
and `Configuration` (`plan_roots`) ports. A plan's tags are held to the
store's rules by `tag-rules`, the crate tasks share.

The domain crates keep their hand-written markup (`aggregate.rs` beside
the rules in `lib.rs`) rather than blackjack's generated adapter over a
pure domain crate (owner ruling Q8): angzarr-build's Rust adapter has no
natural-key root (`rooted_by`) and hands an applier no `PageContext`, and
the task aggregate needs both (its harp is its root; its instants are its
pages' `recorded_at`). The move waits on the generator.

`crates/core/clippy.toml` holds the core's purity rules (no I/O, clock, env,
threads or hash-ordered collections) and reaches every core crate, because
clippy reads the nearest `clippy.toml`. A crate's own `clippy.toml` would
replace it, so none has one. It repeats the root `clippy.toml`'s settings,
and `just lint` refuses to run if they disagree.

The libraries adapters build on are angzarr-platform's
(https://github.com/angzarr-io/angzarr-platform, pinned by commit, moved
there from this repository by decision 36): `os`, everything that differs
between operating systems; the `file-system` seam over it; and
`file-lock`'s advisory locks over both. They implement no port.

All file I/O goes through the `file-system` seam: an `Arc<dyn Fs>`, the
real `Os` in the binary, a temp-rooted `Rooted` in tests (`Rooted::temp`;
where a real process must see the files, an `Os` under a directory a
`Rooted` owns). SQLite and lock files take their path from it
(`Fs::real_path`, `Fs::open`), so a test's databases are sandboxed too.
The root `clippy.toml` disallows `std::fs`, `Path`'s filesystem methods
and `tempfile` everywhere, tests included; only the seam crate, in
angzarr-platform, calls them.

Everything that differs between operating systems is in the `os` crate:
`Flavor` (Unix | Windows), the logic (PATH and PATHEXT, environment
names, the home variable, path display and comparison keys, lock-file
records) as pure functions both flavors test on every host; and the `Os`
trait, the host's syscalls (modes, directory links, file identity,
directory sync, advisory locks, the stop signal), made once by
`os::host()` and reached through the seam (`Fs::os`). Nothing here names
an OS: no `cfg(unix|windows|target_os|target_family)`, `cfg!(`,
`std::os::`, `split_paths` or target-specific dependency, tests included
(a test that needs a unix tool asks `os::host().flavor()` at run time).
`just lint-os` checks it, with an allowlist (`scripts/os-cfg-allow.txt`),
and `just lint` runs it. `just check-windows` compiles the adapters that use
it, with it, for Windows, a quick check from Linux; CI (`.github/workflows/ci.yml`)
builds and tests the whole workspace natively on a Windows runner. So a
test names a path that must be absolute through `os::fixture` (`/tree`,
`C:\tree` on Windows; os's `test-support` feature, on through a
dev-dependency), never a unix literal, and builds an expected path
the way the code does, joined component by component.

## Architecture: hexagonal around CQRS/ES (checked in review)

Dependencies point inward; a review refuses a change that breaks these:

- The core depends on nothing outside it. A domain or read model links only
  angzarr-embedded (the in-app runtime: `TaskAggregate` is its aggregate,
  the task list its projection), angzarr-embedded-macros (the aggregate's
  markup), angzarr-diagnostic, prost, its wire crate (generated
  over angzarr-proto), and tagma's tagma-core (the tag grammar and query
  engine) and tagma-proto (its `Tag` message): no I/O, async runtime or
  clock in decide, apply or a projection. The ports link angzarr-ports,
  whose store traits they lend. One read model
  links more: `task-priority` evaluates the tag schema's priority formulas with
  expr-lang (the Rust port of the language Go's taskloom evaluates them
  in), which is computation and nothing else.
- A driven adapter may depend on a platform crate (angzarr-platform's;
  that is not one adapter depending on another).
- A driven adapter sees only ports (and wire types), never a use case. A
  driving adapter sees only the application layer, never a domain's
  internals. Adapters never depend on each other, with one exception:
  driving adapters share the presentation crates, angzarr core's
  `angzarr-presentation` (domain-free; moved there from this repository by
  decision 36) and a domain's own, such as
  `tasks-presentation` (the task JSON the CLI writes).
- A domain never depends on a read model (the write side reads no
  projection); a read model never depends on the application layer (a query
  issues no command).
- Contexts depend one way, downstream on upstream. Plans are downstream of
  the tasks (a plan's `governs:` names task ids; a task names no plan, and
  `tasks show` prints no plans line): plans-application depends on the
  tasks' application crate for its `Tasks` port (`ProjectTasks`: whether a
  task exists, and the store's rules) and on `tag-rules`, and no tasks
  crate names a plan. The task-to-plan lookup is `plans list --governs`,
  which reads the plan files.
- Entities are aggregates; anything listed is a projector's read model,
  never an aggregate. Nothing numbers across streams (ruling D7). A watch
  holds the store's change token to know when to look, and since the
  owner's ruling of 2026-11-03 (consolidation slice D, which reverses
  D7's cut of 2026-10-30) it reads the task streams from a cursor of one
  position per stream (`tasks-watch`'s `cursor.rs`; `tasks-application`'s
  `WatchService::heads` and `after`, over `EventStore::covers` and
  `read`, no port of its own): the use case keeps none, the driving
  adapter holds the positions and every line carries them. No order
  across streams is promised.

## Generated code is never committed

Wire and domain types are generated into `OUT_DIR` by `angzarr-build` from
`proto/`. A core crate's `build.rs` is one call (`angzarr_build::wire` or
`angzarr_build::domain`): angzarr-build reads the build environment itself,
so the core's clippy rules hold in build scripts too. Change the `.proto`,
never generated output. A tag is tagma's message, `tagma.v1.Tag`, imported
from tagma-proto's `PROTO_INCLUDE`; the generated domain layer gives it a
domain `Tag` that converts to `tagma_core::Tag` and spells itself
canonically.

## Storage is angzarr-store-sqlite

A project's database is angzarr-store-sqlite's `SqliteStore` (in the
angzarr-embedded repository), opened at
`~/.ctxloom/projects/<project-id>/state.db` by task-store-sqlite's
`SqliteStores`. ctxloom writes no SQL of its own. A store from before
angzarr-embedded (annals-store-sql's) is refused, naming its backup
(`state.db.pre-embedded`), never migrated in place (the owner's ruling
D2). Any other store older than the build is migrated when it is opened,
by a read as by a write (the owner's ruling, 2026-10-31): angzarr-store-sqlite
does it on every open, through angzarr-ports' `Migrations`, after backing
the store up beside itself through its `Backup`
(`state.db.pre-<applied level>-<UTC time>`); one newer than the build, or
one a migration failed or changed in, is refused untouched.

A store is closed, never dropped, wherever its file matters after (ruling
131): angzarr-store-sqlite's `SqliteStore::close` waits for the
connection's close, so the last close's WAL checkpoint and the log's
removal are over when it returns (a drop leaves them to sqlx's thread,
which an exiting process may never let run, leaving the log holding the
command's commits). `SqliteStores` closes a store it gives up for another
and, through tasks-lib's `Close` (Tasks, Import, Plans), the one it holds
when the binary's command is done; the check and the export close their
own. A read's connection is read-only, and SQLite leaves an empty log and
index beside the file after it. A test looks at the files the moment the
close returns, never polling for the log's removal.

A store an older generation of the build wrote is an archive (the owner's
ruling): its event pages are of an angzarr wire this build does not decode
(`EventPage.payload`, now `body`), so it is read only by the build that
wrote it, and this build never writes to a store it cannot read. Before any
open (so before a migration, a backup, or a `-wal`/`-shm` made beside it),
task-store-sqlite reads the file once on an `immutable` read-only
connection (SQLite makes no sidecar and takes no lock where no log holds
commits; the ledger is no mark, the migrations being the same for both
generations) and tries to decode its first event page as this build's
`EventPage`: every older page fails, since `payload` was a length-delimited
field 3 and `sequence` is a varint there now. Such a store is refused with
`task-store-sqlite-older-generation` (args `path`, `archive`), carried as
the cause of the envelope's `angzarr-ports-store-unavailable`, exit 1 like
every store the build cannot use, with not one byte, entry or mtime of the
store or its directory changed. The message says how to archive it: move
`state.db` (and `state.db-wal`, `state.db-shm`) aside, e.g. to
`state.db.archive-<date>`, and read it with the pre-port taskloom binary;
the next command creates a fresh store. `manage doctor`'s check refuses it
the same way. The one SQL this crate runs is that read; it writes none.

`taskloom manage store export` writes a project's database whole to stdout
as angzarr-store-transfer's JSONL (every row in proto3's JSON mapping, the
tasks' payloads expanded through `tasks_lib::DESCRIPTOR_SETS`), for carrying
it to a build of another proto generation; its closing note is on stderr
(`exported N records over M streams`) and it takes no `--format`.
`manage store fold-digest` writes one JSONL line per stream (the task
streams, then the schema stream, in the store's order: cover, head and, for
a domain that takes a snapshot, state) as angzarr-embedded's
`Kernel::fold_digest` folds it; two builds' digests of one store's crossing
must be the same bytes. Plans are files, so they have no stream and no line.
The export is `store-cli` over `project-application`'s `Projects::export`
and the `StoreExport` port, which task-store-sqlite implements: core's
`Export` port is the store's own (every row, as a list), and `StoreExport`
is the path-level command around it (the refusals of every open, then the
ledger, then the read-only open), so the two are not duplicates. Before it
opens anything the export refuses what every open refuses (an older
generation's store, one from before angzarr-embedded), and then a ledger
that is not exactly this build's migration set
(`angzarr-store-transfer-ledger-not-the-builds`); it migrates, backs up and
writes nothing. Run it on a copy (`VACUUM INTO`): a store with outbox
entries still to deliver is refused.

There is no unit of work: each handling step is one atomic write. The
kernel owns retries (angzarr's owner ruling, 2026-10-31): a command that
loses the race at its write is re-run on the new head, and a store that
could not say is tried again, under angzarr-embedded's `OnRetryable`
(`KERNEL_RETRY`, in taskloom's shape: `kernel_runner::backoff`), its waits
on taskloom's `Delay` and `Jitter` ports (`Ported`), so tests stay exact.
Every context sends its commands through kernel-runner's `Runner`, which
gathers the dead letters each call answers (angzarr-embedded's
`DeadLetter`) and hands back the domain's refusal typed, or why the
command was not decided (`Unrun`), for the context to report as its own
error. A command that depends on the task
as it is is sent at the head it read, so a write landing between is the
kernel's stale answer, and taskloom reads and shapes it again. Domain, projector and use-case tests run through
angzarr-embedded's kernel over angzarr-ports' `MemoryStore`, as the binary
runs over SQLite.

## Co-developing angzarr-build or tagma

angzarr-embedded, angzarr-build (the markup domain layer generator, with
angzarr-codegen; github.com/angzarr-io/angzarr-build) and tagma are git
dependencies pinned by full commit in Cargo.toml, so a fresh clone builds
with no sibling checkouts. To change angzarr-build or tagma and this
workspace together, `cp .cargo/local-deps.toml .cargo/config.toml`: a
gitignored `[patch]` onto the sibling checkouts (angzarr-build at
`../../angzarr/angzarr-build/main`, tagma at `../../kvtag`). CI, a fresh
clone and the pre-commit gates never read it. Restore Cargo.lock before
committing, and land the change by pushing the sibling and bumping its rev.

The angzarr pins are one set: angzarr core 415b29e (diagnostic, ports, yaml,
presentation, and store-transfer), angzarr-embedded 640d2d0,
angzarr-platform da6a3ee, angzarr-build 558cde5. Moving one moves all; the
lock holds one of each.

## Errors are localizable: a code, args, and an en-US message

Every error and rejection implements `angzarr_diagnostic::Diagnostic`.
No English crosses (angzarr decision 14.37): an error's `Display` is its
code and args (`angzarr_diagnostic::display`: ` name=value`, a
text value quoted where it would not parse back), a third party's text
(the OS's, a parser's, clap's, Fluent's) is the arg `ext_err_txt`, and
presentation shows a diagnostic no catalog knows as its code and args.
English is the catalogs' alone: a test that holds it to Go's renders it
from them (angzarr-presentation's `english_for!`, behind its
`test-support` feature, on through a dev-dependency). No code
spells its crate's name (the owner's ruling, 2026-11-02: names by
reflection): a code is written `code!("suffix")` (angzarr-diagnostic's
macro, which prefixes `CARGO_PKG_NAME` and a dash), so a renamed crate's
codes follow it. A new variant needs a stable code, an en-US message in
the crate's `locales/en-US/messages.ftl` under the code's suffix alone,
using exactly its args, and a sample in the crate's `diagnostic_samples()`.
The crate's `build.rs` calls `angzarr_diagnostic::build::catalogs()` (a
build-dependency on angzarr-diagnostic with its `build` feature), which
writes the catalog to `OUT_DIR` with every id under the package name. The
crate's catalog tests check messages and samples both ways, and that every
code is under its name (`angzarr_diagnostic::catalog::check`). A test asks
which error it has by its typed variant (`matches!`), or by the code of a
typed error of another crate's (`TaskRejection::NoTask.code()`), never by
a code spelled out.

`diagnostic_samples()` is test-only API, so it is no part of a binary:
`#[cfg(any(test, feature = "test-support"))]` on the function and its
re-export, where `test-support` is the crate's own feature (declared in
its Cargo.toml, enabling nothing else). The crate's tests see it through
`cfg(test)`; another crate's tests (tasks-lib's and the binary's
every-diagnostic catalog tests) list the crate again under
`[dev-dependencies]` with `features = ["test-support"]`, which no build of
a binary turns on. Any other API only tests use goes behind the same
gate: `#[cfg(test)]` when only its own crate's tests use it, the
crate's `test-support` feature when another crate's do (os's `fixture`,
`Mode::PRIVATE_DIR`, `Flavor::program_file`, `Flavor::startup_vars`,
`own_group` and `ctrl_break`).

Each crate with diagnostics exports its embedded en-US catalog as `EN_US`:
`(env!("CARGO_PKG_NAME"), include_str!(concat!(env!("OUT_DIR"),
"/en-US.ftl")))`, its name and the generated catalog. The composition root assembles the binary's
catalog from every crate's `EN_US` (angzarr-presentation's `Catalog`), and the
driving adapters render with it: a diagnostic's catalog message in text,
its `{code, args, message}` envelope in JSON. A new core or driven
crate's `EN_US` (or a platform crate's) joins tasks-lib's `catalog()` (beside
angzarr-embedded's, the angzarr-ports errors it forwards and
angzarr-store-sqlite's), and its samples tasks-lib's and the binary's
every-diagnostic catalog tests; a driving adapter's joins the frontends
the binary hands that `catalog()` (`catalog()` in its `main.rs`).

## Use `just`

`just build`, `just test` (cargo-nextest, required: a missing nextest fails
the gate), `just lint` (clippy, warnings denied) and `just lint-deps`
(cargo-deny) are the checks. Every gate is terse: a pass prints one `ok
<gate> (<scope>) <duration>` line; a failure prints `FAIL <gate>: <summary>
(full log: target/logs/<gate>.log)` and only the relevant extract (failing
tests' output, short clippy messages, the files rustfmt would change, deny's
errors, the mutation verdict and survivors). Read the log for the rest;
`VERBOSE=1 just <gate>` shows the full output. `just fmt` and `just
fmt-check` cover this workspace's crates only, never angzarr's crates
or tagma, whether pinned or patched onto local checkouts.

## This repository's tasks are in the Rust taskloom

The Rust taskloom is the global taskloom (the owner's switch,
2026-10-31): `just install` puts it in `~/.local/share/taskloom-rs/bin`,
which must come first on PATH, as `~/.zshrc` puts it; `.envrc` (direnv)
puts it first here too. It serves no MCP. Run `taskloom` and not
`~/go/bin/taskloom`, which writes a log the Rust store never reads.

## The git-hook gates are in gates/; the justfile gives their parameters

The gates and their recipes are `gates/gates.just` and
`gates/lefthook.yml`: the justfile imports the one and lefthook.yml extends
the other. The justfile keeps taskloom's parameters for them
(`mutation_thresholds`, `workspace_wide`, `staged_tree`, `feature_rules`,
`features_prepare`) and its own `build` and `lint`; their tool configs are
`deny.toml`, `.gitleaks.toml` and `.cargo/mutants.toml`. `just
hooks-install` installs the hooks, once per clone (every worktree shares
them), and again after a change to the gates names a new hook
(`gates/MIGRATING.md`). They came from the shared gates repository that
annals and ctxloom-rs still link; a change here does not reach them.

lefthook runs the gates one at a time, stopping at the first failure, and
shows each gate's pass line and any failure's extract. Gates 1 to 5 run at
pre-commit, and pre-push runs the push's tests (`just push-check`).
Mutation (6) runs in no git hook: it runs while tests are written and
judged, at the end of an unattended run, and in the scheduled CI full pass.
A failing gate is a
finding: fix the cause and commit again; never `--no-verify`, and never
weaken a gate to get through.

1. **fmt** formats the staged `*.rs` files in the index (`just
   fmt-staged`) rather than refusing an unformatted commit. A fully staged
   file's working copy gets the same content; a partially staged file's
   working copy is left exactly as it was, and named (`partially staged:
   <file> is committed formatted; its working copy is left as it was`).
   `just fmt-check` is the check for CI and manual runs.
2. **secrets**: gitleaks over the staged changes (`.gitleaks.toml`). A fake
   secret in a test ends its line with `gitleaks:allow`.
3. **clippy**: `just lint`, including the core's purity rules
   (`crates/core/clippy.toml`).
4. **deps**: `just lint-deps`, cargo-deny (`deny.toml`).
5. **tests**: `just test-staged` tests the changed crates and their
   dependents in the staged tree (HEAD plus the staged diff, a sibling
   worktree of the checkout); a staged `workspace_wide` file tests the whole
   workspace.
6. **mutants** (no hook; by hand and in scheduled CI): `just mutants-push`
   mutation-tests what each ref given on stdin, as git's pre-push refs,
   adds (`cargo mutants --in-diff` over the diff from the
   remote's tip of that ref, or for a new branch from its merge base with
   the default branch, to the commit pushed), holding new logic to its
   tier: every
   hand-written tier (domain, read models, ports, application and adapters)
   90% or more; the composition root is listed but not held (the justfile's
   `mutation_thresholds` has every tier); the generated wire code is
   excluded (`.cargo/mutants.toml`). Each mutant runs only its own crate's
   tests (`test_workspace = false`), under `cargo test`: a dependent's
   tests, the binary's acceptance tests included, kill nothing, so a crate
   kills its own mutants. A mutant judged equivalent or untestable gets
   `#[cfg_attr(test, mutants::skip)]` on its function, with `mutants` a
   dev-dependency (`mutants.workspace = true`, so nothing new is linked)
   and a one-line comment above it saying why; never an exclude in
   `.cargo/mutants.toml` or a lowered threshold. cargo mutants runs in a
   session of its own (`setsid`), and whatever it leaves running (a
   process a test spawned whose stop a mutant broke) is killed and named
   in the gate's log; the verdict does not change. The last run's mutants.out (each
   mutant's log, outcomes.json) is kept in target/logs/mutants-push/. Red
   tests fail it on every branch (no mutant can be judged against failing
   tests), as does a run that found mutants and tested fewer, and a run
   that found mutants and judged none of them at all (every one unviable):
   a mutation run that judged nothing has shown nothing. A tier all
   unviable beside one that judged passes, its row saying "none judged"
   (ctxloom rulings 120, 124: the whole run's rule). The verdict is
   `_mutants-verdict` in `gates/gates.just`, inline jq over cargo-mutants'
   `outcomes.json` (`outcomes[]`, `total_mutants`) and its exit code, no
   log read; its self-test is `scripts/mutants-verdict.selftest.sh` (`just
   selftest`). No feature rule reaches a mutation run: every mutation
   recipe refuses a non-empty `feature_rules`, and a crate whose tests need
   a feature of its own turns it on through a dev-dependency on itself
   (ctxloom rulings 99/105).
   `just mutants-staged` runs the same judgement over the staged diff, by
   hand, before committing, and `just mutants-all` over the whole workspace
   at HEAD (the full pass; weekly in CI, `.github/workflows/mutation.yml`).
