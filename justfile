# tasks — per-project task store and MCP server, extracted from ctxloom.
# Standalone module; tests run on the host (no devcontainer, no build tags).
TOP := `git rev-parse --show-toplevel`

# Get version from versionator (with fallback for CI without versionator)
# Format: v0.0.1-abc1234.20240115103045 (uncommitted) or v0.0.1-abc1234 (clean)
# Requires versionator >= v0.2.0 (DateTimeDirty + `output version` subcommand).
version := `versionator output version -t "{{Prefix}}{{MajorMinorPatch}}{{PreReleaseWithDash}}" --prefix --prerelease="{{ShortHash}}{{DateTimeDirty}}" 2>/dev/null || echo "dev"`

# Show current version
show-version:
    @versionator output version

# Build the tasks binary.
build:
    go build -ldflags "-X main.version={{version}}" -o {{TOP}}/bin/tasks {{TOP}}/cmd/tasks

# Install the tasks binary to GOBIN (default ~/go/bin).
install:
    go install -ldflags "-X main.version={{version}}" {{TOP}}/cmd/tasks

# Run the package tests under -race.
test *ARGS:
    go test -race {{ARGS}} {{TOP}}/...

# Vet all packages.
vet:
    go vet {{TOP}}/...

# Tidy module dependencies.
tidy:
    go mod tidy
