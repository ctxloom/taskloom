#!/usr/bin/env bash
# Print the purge-gated source files, one per line, relative to the repo root:
# the non-test .rs files carrying `cfg(feature = "purge")`, plus each module
# (`mod NAME;` / `pub mod NAME;`) such a cfg line gates, which resolves to
# NAME.rs or NAME/mod.rs beside the file that declares it. `just
# mutants-purge` limits a mutation run to these.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

git grep -lF 'cfg(feature = "purge")' -- '*.rs' \
  | grep -Ev '(\.test\.rs$|(^|/)tests/)' | sort -u | while read -r f; do
    echo "$f"
    dir=$(dirname "$f")
    # A cfg line directly followed by a `mod` declaration gates that module.
    awk '/^[[:space:]]*#\[cfg\(feature = "purge"\)\]/ {g=1; next}
         g && /^[[:space:]]*(pub(\([^)]*\))? +)?mod +[A-Za-z0-9_]+;/ {
           s=$0; sub(/^.*mod +/, "", s); sub(/;.*/, "", s); print s }
         {g=0}' "$f" | while read -r m; do
      for c in "$dir/$m.rs" "$dir/$m/mod.rs"; do
        [ -f "$c" ] && echo "$c" && break
      done
    done
  done | sort -u
