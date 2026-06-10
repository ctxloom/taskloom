# tasks — per-project task store and MCP server, extracted from ctxloom.
# Standalone module; tests run on the host (no devcontainer, no build tags).
TOP := `git rev-parse --show-toplevel`

# Version stamp: latest tag (or short hash before any tag exists), -dirty when
# the tree has uncommitted changes.
version := `git describe --tags --always --dirty 2>/dev/null || echo dev`

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
