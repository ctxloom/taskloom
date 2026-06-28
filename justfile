# taskloom — per-project task store and MCP server, extracted from ctxloom.
# Standalone module. Plain-go targets run on the host; the tool-dependent
# targets (lint, mutation) run inside the pre-baked devcontainer so the pinned
# toolchain is used and go.mod stays free of a linter tool tree.
TOP := `git rev-parse --show-toplevel`

# Get version from versionator (with fallback for CI without versionator).
# Standardized stamp across the ctxloom family:
#   v<major.minor.patch>-<short-sha>-<YYYYMMDDTHHMMSS commit datetime, utc>
# versionator emits the compact datetime (no separator); sed inserts the 'T'.
version := `if v=$(versionator output version -t "{{Prefix}}{{MajorMinorPatch}}-{{ShortHash}}-{{CommitDateCompact}}" --prefix 2>/dev/null); then echo "$v" | sed -E 's/([0-9]{8})([0-9]{6})$/\1T\2/'; else echo dev; fi`
ldflags := "-X main.version=" + version

# Container runtime (docker or podman) and the devcontainer image tag.
container_cmd := env_var_or_default("CONTAINER_CMD", "docker")
devcontainer_image := "taskloom-devcontainer"

# List available recipes.
default:
    @just --list

# Show current version.
show-version:
    @versionator output version

# Set the release version (the only supported way to bump). Releases are
# merge-triggered: bump here, commit VERSION, merge — CI tags it immutably.
# Example: just set-version 0.7.0
set-version version:
    versionator set {{version}}

# Build the taskloom binary (version-stamped).
build:
    go build -ldflags "{{ldflags}}" -o {{TOP}}/bin/taskloom {{TOP}}/cmd/taskloom

# Build a stripped, CGO-free static binary (release shape, version-stamped).
build-static:
    CGO_ENABLED=0 go build -ldflags "-s -w {{ldflags}}" -o {{TOP}}/bin/taskloom {{TOP}}/cmd/taskloom

# Install the taskloom binary to GOBIN (default ~/go/bin).
install:
    go install -ldflags "{{ldflags}}" {{TOP}}/cmd/taskloom

# Run the package tests under -race.
test *ARGS:
    go test -race {{ARGS}} {{TOP}}/...

# Vet all packages.
vet:
    go vet {{TOP}}/...

# Report any unformatted files (CI-friendly; non-zero on drift).
fmt-check:
    @test -z "$(gofmt -l .)" || { echo "unformatted:"; gofmt -l .; exit 1; }

# Format the tree in place.
fmt:
    gofmt -w .

# golangci-lint (v2; config in .golangci.yml) — runs in the devcontainer.
lint: dev-image
    just _run lint

# Mutation testing (gremlins; config in .gremlins.yaml) — runs in the devcontainer.
test-mutation *ARGS: dev-image
    just _run test-mutation {{ARGS}}

# Tidy module dependencies.
tidy:
    go mod tidy

# Remove build artifacts.
clean:
    rm -rf {{TOP}}/bin

# CI entrypoint: format check, vet, lint, race tests.
check: fmt-check vet lint test

# ===== devcontainer delegation (shared family pattern) =====

# Build the devcontainer image from .devcontainer/Dockerfile.
dev-image:
    {{container_cmd}} build -t {{devcontainer_image}}:latest -f .devcontainer/Dockerfile .

# Internal helper: run a justfile.container target inside the devcontainer.
# Short-circuits to a direct invocation when already in CI/devcontainer.
_run +ARGS:
    #!/usr/bin/env bash
    if [ -n "$DEVCONTAINER" ] || [ -n "$CI" ] || [ -n "$GITHUB_ACTIONS" ]; then
        just -f justfile.container {{ARGS}}
    else
        user_flag=(--user "$(id -u):$(id -g)")
        if {{container_cmd}} info 2>/dev/null | grep -q "rootless"; then user_flag=(); fi
        GOWORK=off go mod download
        cache_mount=()
        if [ -d "$HOME/go/pkg/mod" ]; then cache_mount=(-v "$HOME/go/pkg/mod:/tmp/gomodcache:ro"); fi
        {{container_cmd}} run --rm \
            "${user_flag[@]}" \
            "${cache_mount[@]}" \
            -e HOME=/tmp \
            -e GOMODCACHE=/tmp/gomodcache \
            -e GOCACHE=/tmp/.gocache \
            -e GOWORK=off \
            -v "$(pwd):/workspace" \
            -v "$(pwd)/justfile.container:/workspace/justfile:ro" \
            -w /workspace \
            {{devcontainer_image}}:latest \
            just {{ARGS}}
    fi
