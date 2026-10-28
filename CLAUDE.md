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
- `core/application`: command and query handlers.
- `adapters/driven`: port implementations (stores, clock, git).
- `adapters/platform`: libraries adapters build on (`os`, everything that
  differs between operating systems; the `file-system` seam over it; and
  `file-lock`'s advisory locks over both); they implement no port.
- `adapters/driving`: CLI, MCP, gRPC: translation only.
- `root/lib`: the composition root's wiring as a library (`tasks-lib`):
  the use cases over the default adapters, which the binaries, their
  servers and ctxloom build from, so no frontend wires an adapter.
- `root/bin`: binaries, the composition root.

This repository is taskloom: the `taskloom` binary and the crates it
composes. ctxloom depends on those crates by git (pinned by commit), so a
crate here is named for its domain or role, and the binaries under
`root/bin` compose the same core, application and adapters. Below
`root/bin`, a crate is named for its domain or role, never for a binary.

`crates/core/clippy.toml` holds the core's purity rules (no I/O, clock, env,
threads or hash-ordered collections) and reaches every core crate, because
clippy reads the nearest `clippy.toml`. A crate's own `clippy.toml` would
replace it, so none has one. It repeats the root `clippy.toml`'s settings,
and `just lint` refuses to run if they disagree.

All file I/O goes through the `file-system` seam: an `Arc<dyn Fs>`, the
real `Os` in the binary, a temp-rooted `Rooted` in tests (`Rooted::temp`;
where a real process must see the files, an `Os` under a directory a
`Rooted` owns). SQLite and lock files take their path from it
(`Fs::real_path`, `Fs::open`), so a test's databases are sandboxed too.
The root `clippy.toml` disallows `std::fs`, `Path`'s filesystem methods
and `tempfile` everywhere else, tests included; only the seam crate
allows them.

Everything that differs between operating systems is in the `os` crate:
`Flavor` (Unix | Windows), the logic (PATH and PATHEXT, environment
names, the home variable, path display and comparison keys, lock-file
records) as pure functions both flavors test on every host; and the `Os`
trait, the host's syscalls (modes, directory links, file identity,
directory sync, advisory locks, the stop signal), made once by
`os::host()` and reached through the seam (`Fs::os`). Nothing else names
an OS: no `cfg(unix|windows|target_os|target_family)`, `cfg!(`,
`std::os::`, `split_paths` or target-specific dependency, tests included
(a test that needs a unix tool asks `os::host().flavor()` at run time).
`just lint-os` checks it, with an allowlist (`scripts/os-cfg-allow.txt`),
and `just lint` runs it. `just check-windows` compiles the crates that use
it for Windows, a quick check from Linux; CI (`.github/workflows/ci.yml`)
builds and tests the whole workspace natively on a Windows runner. So a
test names a path that must be absolute through `os::fixture` (`/tree`,
`C:\tree` on Windows), never a unix literal, and builds an expected path
the way the code does, joined component by component.

## Architecture: hexagonal around CQRS/ES (checked in review)

Dependencies point inward; a review refuses a change that breaks these:

- The core depends on nothing outside it. A domain or read model links only
  annals, annals-diagnostic, prost, its wire crate (generated over
  angzarr-proto), and tagma's tagma-core (the tag grammar and query engine)
  and tagma-proto (its `Tag` message): no I/O, async runtime or clock in
  decide, apply or a projection. One read model links more:
  `task-priority` evaluates the tag schema's priority formulas with
  expr-lang (the Rust port of the language Go's taskloom evaluates them
  in), which is computation and nothing else.
- A platform crate depends on no other workspace crate but another
  platform crate (`file-lock` on `file-system` and `os`); a driven adapter
  may depend on one (that is not one adapter depending on another).
- A driven adapter sees only ports (and wire types), never a use case. A
  driving adapter sees only the application layer, never a domain's
  internals. Adapters never depend on each other, with one exception:
  driving adapters share the presentation crates, `presentation`
  (domain-free; its tests keep it so) and a domain's own, such as
  `tasks-presentation` (the task JSON the CLI and MCP server both write).
- A domain never depends on a read model (the write side reads no
  projection); a read model never depends on the application layer (a query
  issues no command).
- Entities are aggregates; anything listed is a projector's read model,
  never an aggregate. A UI watches one stream per watch; a list's cursor is
  its projector's own sequence.

## Generated code is never committed

Wire and domain types are generated into `OUT_DIR` by `angzarr-build` from
`proto/`. A core crate's `build.rs` is one call (`angzarr_build::wire` or
`angzarr_build::domain`): angzarr-build reads the build environment itself,
so the core's clippy rules hold in build scripts too. Change the `.proto`,
never generated output. A tag is tagma's message, `tagma.v1.Tag`, imported
from tagma-proto's `PROTO_INCLUDE`; the generated domain layer gives it a
domain `Tag` that converts to `tagma_core::Tag` and spells itself
canonically.

## Storage is annals-store-sql, in annals for now

The SQL store is `annals-store-sql` (feature `sqlite`), in the annals
repository: backend-neutral statements, with each backend's execution and
migrations behind a feature. It lives in annals until angzarr core's older
SQL storage is brought onto it, and then moves to angzarr core. ctxloom
writes no SQL of its own; a project's database is that crate's SQLite store,
opened at `~/.ctxloom/projects/<project-id>/state.db`.

Any store is held to annals' store contract (`annals::store_contract!` or
`annals::async_store_contract!`), the checks `InMemoryStore` passes. Domain
and projector tests run through annals' kernel over `InMemoryStore`, as the
binary runs over SQLite.

## Co-developing annals, angzarr-cli or tagma

annals, angzarr-build (angzarr-cli's `rust-cli` branch) and tagma are git
dependencies pinned by full commit in Cargo.toml, so a fresh clone builds
with no sibling checkouts. To change one of them and this workspace
together, `cp .cargo/local-deps.toml .cargo/config.toml`: a gitignored
`[patch]` onto the sibling checkouts (annals at `../../annals`, angzarr-cli
at `../../angzarr/angzarr-cli/main`, tagma at `../../kvtag`). CI, a fresh
clone and the pre-commit gates never read it. Restore Cargo.lock before
committing, and land the change by pushing the sibling and bumping its rev.

## Errors are localizable: a code, args, and an en-US message

Every error and rejection implements `annals_diagnostic::Diagnostic` with an
English `Display` fallback. A new variant needs a stable code (prefixed with
the crate's name), an en-US message in `locales/en-US/<crate>.ftl` using
exactly its args, and a sample in the crate's `diagnostic_samples()`. The
crate's catalog test checks messages and samples both ways.

Each crate with diagnostics exports its embedded en-US catalog as `EN_US`
(its name and its `.ftl`). The composition root assembles the binary's
catalog from every crate's `EN_US` (presentation's `Catalog`), and the
driving adapters render with it: a diagnostic's catalog message in text,
its `{code, args, message}` envelope in JSON. A new core, driven or
platform crate's `EN_US` joins tasks-lib's `catalog()` (beside annals'
and annals-store-sql's), and its samples tasks-lib's and the binary's
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
fmt-check` cover this workspace's crates only, never annals, angzarr-build
or tagma, whether pinned or patched onto local checkouts.

## This repository's tasks are in the Rust taskloom

`just install` puts it in `~/.local/share/taskloom-rs/bin`, on no PATH;
`.envrc` (direnv, `direnv allow` once per checkout) puts that first, so a
session started here runs it as its MCP server and CLI. Run `taskloom` and
not `~/go/bin/taskloom`, which writes a log the Rust store never reads.

## The pre-commit gates are in gates/; the justfile gives their parameters

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
shows each gate's pass line and any failure's extract. A failing gate is a
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
6. **mutants**: `just mutants-staged` mutation-tests the staged diff
   (`cargo mutants --in-diff`), holding new logic to its tier: every
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
   mutant's log, outcomes.json) is kept in target/logs/mutants-staged/.
