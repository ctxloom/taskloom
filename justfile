# tasks — per-project task store and MCP server, extracted from ctxloom.
# Standalone module; tests run on the host (no devcontainer, no build tags).
TOP := `git rev-parse --show-toplevel`

# Build the tasks binary.
build:
    go build -o {{TOP}}/bin/tasks {{TOP}}/cmd/tasks

# Run the package tests under -race.
test *ARGS:
    go test -race {{ARGS}} {{TOP}}/...

# Vet all packages.
vet:
    go vet {{TOP}}/...

# Tidy module dependencies.
tidy:
    go mod tidy
