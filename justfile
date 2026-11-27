# The git-hook gates and their recipes (fmt, fmt-check, fmt-staged, test,
# test-staged, mutants-push, mutants-staged, mutants-all, lint-deps,
# secrets-scan-staged and the `_run` machinery and extractors) are
# gates/gates.just, imported below; this file gives them taskloom's
# parameters and keeps its own recipes (`build`, `lint`).
# cargo-deny's duplicate-version warnings stay in target/logs/lint-deps.log,
# not silenced in deny.toml.

TOP := `git rev-parse --show-toplevel`

import 'gates/gates.just'

# Cross-project dependency automation (dev::local, repin, land, pins-check): the
# shared dev module of rust-support's tools/dev, linked as .dev (gitignored;
# `tools/dev/install.sh` in a rust-support checkout). Optional: absent in a clone
# or CI without the sibling.
mod? dev '.dev'

# The staged tree is a sibling of the checkout, not under target/: cargo
# reads .cargo/config.toml from every parent directory, so under the checkout
# it would pick up a local-deps override (a generated dev-module config) and gate the
# sibling checkouts instead of the commit's pinned dependencies.
staged_tree := "../.{name}.staged"

# The two flavours the product is built and tested in (and installed in): `plain` (the
# default: no prune verb and no Purge API compiled in) and `purge` (the cargo
# feature of that name, on every crate that has it). `TASKLOOM_FLAVOUR=purge
# just <build|lint|test|...>` runs a gate in the purge flavour; CI runs every
# gate in both.
flavour := env("TASKLOOM_FLAVOUR", "plain")
flavour_features := if flavour == "purge" { "--features purge" } else if flavour == "plain" { "" } else { error("TASKLOOM_FLAVOUR must be plain or purge, not " + flavour) }

# The flavour's feature, turned on for the tests (and refused for a mutation
# run by gates.just: a mutation run is of the plain flavour).
feature_rules := if flavour == "purge" { "taskloom=purge" } else { "" }
features_prepare := ""

# Mutation kill-rate thresholds, percent, by crate path: the one place they
# are stated. A mutant is judged by the longest prefix its file matches; one
# matching none fails the gate, so a new tier gets a threshold before it gets
# code. core/wire is not here: .cargo/mutants.toml excludes it (generated).
# root/bin at 0 reports its survivors and refuses nothing: the composition root
# holds no logic, and the binary's wiring is shown by running it, not by unit
# tests. root/lib, the wiring the binary and its servers share, is a library
# with tests of its own, and held as the adapters are.
mutation_thresholds := "crates/core/domain=90 crates/core/read-model=90 crates/core/ports=90 crates/core/application=90 crates/adapters=90 crates/root/lib=90 crates/root/bin=0"

# Staged paths outside any crate that can change what every crate builds or
# tests (bash globs, `*` crossing `/`): one of these staged tests the whole
# workspace.
workspace_wide := "Cargo.toml Cargo.lock rust-toolchain.toml proto/* .cargo/config.toml .cargo/config"

# Mutation-test the purge flavour: `tasks prune` and every Purge call, the code
# that deletes task history, which no other mutation recipe reaches (a mutation
# run is of the plain flavour: gates.just refuses a feature rule, and that stays).
# Its own recipe by the owner's ruling (2026-11-27): not a step of `check`, CI or
# the close-out. `cargo mutants --features purge --in-diff` over BASE..HEAD,
# limited to the files `scripts/purge-files.sh` names (the non-test files with
# cfg(feature = "purge") and the modules they gate), so only crates that have the
# feature are built with it (the hollow pass the refusal guards against needs a
# crate without it). `extra` goes to cargo-mutants: `just mutants-purge BASE
# --list` lists the mutants and judges nothing. Judged by `_mutants-verdict` and
# `mutation_thresholds`, as every other recipe; its mutants.out is kept in
# target/logs/mutants-purge/. Mutates a copy of the tree (no --in-place), so
# commit first: it refuses a tree with uncommitted Rust changes.
[positional-arguments]
[no-exit-message]
mutants-purge base *extra:
    #!/usr/bin/env bash
    set -euo pipefail
    base=$1
    shift
    root='{{TOP}}'
    cd "$root"
    just --justfile "$root/justfile" _require-mutants mutants-purge
    git rev-parse --verify --quiet "$base^{commit}" >/dev/null \
        || { echo "FAIL mutants-purge: no commit $base" >&2; exit 1; }
    if ! git diff --quiet HEAD -- '*.rs'; then
        echo "FAIL mutants-purge: uncommitted Rust changes; the run mutates the tree and judges BASE..HEAD" >&2
        exit 1
    fi
    mapfile -t files < <(scripts/purge-files.sh)
    work=$(mktemp -d)
    session="$work/session"
    reap() {
        local sid
        sid=$(cat "$session" 2>/dev/null) || return 0
        [ -n "$sid" ] && pkill -KILL -s "$sid" 2>/dev/null || true
    }
    trap 'reap; rm -rf "$work"' EXIT
    trap 'exit 130' INT TERM HUP
    git diff --no-color --no-ext-diff --no-renames --src-prefix=a/ --dst-prefix=b/ "$base..HEAD" -- "${files[@]}" > "$work/purge.patch"
    if [ ! -s "$work/purge.patch" ]; then
        echo "mutants-purge: no change to the purge-gated sources in $base..HEAD; nothing to mutate."
        exit 0
    fi
    out=${GATE_LOG_DIR:-$root/target/logs}/mutants-purge
    rm -rf "$out"
    mkdir -p "$out"
    rc=0
    setsid --wait bash -c 'echo $$ > "$1"; shift; exec "$@"' mutants-session "$session" \
        cargo mutants --dir "$root" --workspace --features purge --in-diff "$work/purge.patch" \
        --output "$out" --colors never "$@" 9>&- &
    wait "$!" || rc=$?
    reap
    if [ "${1:-}" = "--list" ] || [ "${1:-}" = "--list-files" ]; then
        exit "$rc"
    fi
    just --justfile "$root/justfile" _mutants-verdict mutants-purge "$out" "$rc"

# Build every crate. Needs protoc on PATH and access to github.com (angzarr-proto).
[no-exit-message]
build:
    #!/usr/bin/env bash
    set -euo pipefail
    terse=(-q --message-format=short)
    [ "${VERBOSE:-}" = 1 ] && terse=()
    exec just --justfile {{TOP}}/justfile _run build workspace cargo \
        cargo build "${terse[@]}" --manifest-path {{TOP}}/Cargo.toml --workspace {{flavour_features}}

# Clippy, warnings denied, including the core's purity rules
# (crates/core/clippy.toml), after `lint-os`: lefthook's `clippy` step. Clippy reads only the
# nearest clippy.toml, so the core's repeats the root's nesting threshold
# and every method the root's disallows (the file I/O the file-system seam
# holds); refuse to lint if they differ.
[no-exit-message]
lint:
    #!/usr/bin/env bash
    set -euo pipefail
    root=$(grep '^excessive-nesting-threshold' {{TOP}}/clippy.toml)
    core=$(grep '^excessive-nesting-threshold' {{TOP}}/crates/core/clippy.toml)
    if [ "$root" != "$core" ]; then
        echo "FAIL lint: clippy.toml ($root) and crates/core/clippy.toml ($core) disagree on nesting" >&2
        exit 1
    fi
    paths() { grep -o 'path = "[^"]*"' "$1" | sort -u; }
    missing=$(comm -23 <(paths {{TOP}}/clippy.toml) <(paths {{TOP}}/crates/core/clippy.toml) | tr '\n' ' ')
    if [ -n "$missing" ]; then
        echo "FAIL lint: crates/core/clippy.toml does not disallow what clippy.toml does: $missing" >&2
        exit 1
    fi
    {{TOP}}/scripts/os-cfg-check.sh
    terse=(-q --message-format=short)
    [ "${VERBOSE:-}" = 1 ] && terse=()
    exec just --justfile {{TOP}}/justfile _run lint workspace cargo \
        cargo clippy "${terse[@]}" --manifest-path {{TOP}}/Cargo.toml --workspace --all-targets {{flavour_features}} -- -D warnings

# Nothing in this workspace names an operating system (decision 4): that is
# angzarr's os crate's alone: no cfg naming one, `cfg!(`, `std::os::`, `split_paths` or
# target-specific dependency, tests included, but the allowlist's
# (scripts/os-cfg-allow.txt). `just lint` runs it first.
[no-exit-message]
lint-os:
    @{{TOP}}/scripts/os-cfg-check.sh

# The driven adapters that use the platform crates (angzarr's os,
# and the seams over it), with those crates' Windows implementors, compiled
# for a Windows target but not run: the quick check a Linux host can make of them before CI's Windows
# runner builds and tests the whole workspace natively (decision 5; see
# .github/workflows/ci.yml). Needs the x86_64-pc-windows-gnu target (`rustup target
# add`); no linker, since nothing is linked. The rest of the workspace is
# not checked: its C dependencies (psm, stacker) need a mingw C compiler.
[no-exit-message]
check-windows:
    #!/usr/bin/env bash
    set -euo pipefail
    terse=(-q --message-format=short)
    [ "${VERBOSE:-}" = 1 ] && terse=()
    exec just --justfile {{TOP}}/justfile _run check-windows platform cargo \
        cargo check "${terse[@]}" --manifest-path {{TOP}}/Cargo.toml --target x86_64-pc-windows-gnu \
        -p tasks-config -p project-registry --all-targets

# Where `just install` puts the Rust taskloom, the global taskloom since the
# owner's switch (2026-10-31): a cargo root of its own, the binary in
# <root>/bin, which the shell profile (~/.zshrc) puts FIRST on PATH, ahead
# of any other taskloom (cargo's default, ~/.cargo/bin, and the Go one's).
# TASKLOOM_RS_ROOT overrides it.
install_root := env("TASKLOOM_RS_ROOT", home_directory() / ".local/share/taskloom-rs")

# Build and install the taskloom binary into `install_root` (plan step 9), in
# the flavour `TASKLOOM_FLAVOUR` names (the default has no prune verb).
[no-exit-message]
install:
    cargo install --locked --path {{TOP}}/crates/root/bin/taskloom --root {{install_root}} {{flavour_features}}
    @echo "ok   install ({{install_root}}/bin/taskloom; it must come first on PATH, as ~/.zshrc puts it)"

# Wire lefthook's git hooks into the repository's .git/hooks. Worktrees share
# .git/hooks, so this is once per clone, and again whenever lefthook.yml or
# gates/lefthook.yml names a hook not installed yet. Needs lefthook (1.13.6) and
# gitleaks (v8), both by `go install`; cargo-nextest, cargo-deny and
# cargo-mutants by `cargo install <name> --locked`; jq, flock, setsid, ps
# and pkill.
hooks-install:
    lefthook install

# The gates' own self-tests: a throwaway worktree let go alone
# (gates/drop-worktree.sh, never a repository-wide prune), and the mutation
# verdict (`_mutants-verdict`) over fixture cargo-mutants outputs, and the
# refusal of a feature rule.
[no-exit-message]
selftest:
    @{{TOP}}/scripts/drop-worktree.selftest.sh
    @{{TOP}}/scripts/mutants-verdict.selftest.sh

# The tests of the named crates only, by cargo-nextest (the gates' `_tests`):
# the quick loop on one crate. `just test` runs every crate's.
[no-exit-message]
test-pkg +packages:
    @just --justfile '{{TOP}}/justfile' _require-nextest test-pkg
    @just --justfile '{{TOP}}/justfile' _run test-pkg "{{packages}}" tests just --justfile '{{TOP}}/justfile' _tests {{packages}}
