# taskloom — per-project task store and MCP server, extracted from ctxloom.
# Standalone module; tests run on the host (no devcontainer, no build tags).
TOP := `git rev-parse --show-toplevel`

# Get version from versionator (with fallback for CI without versionator).
# Standardized stamp across the ctxloom family:
#   v<major.minor.patch>-<short-sha>-<YYYYMMDDTHHMMSS commit datetime, utc>
# versionator emits the compact datetime (no separator); sed inserts the 'T'.
version := `if v=$(versionator output version -t "{{Prefix}}{{MajorMinorPatch}}-{{ShortHash}}-{{CommitDateCompact}}" --prefix 2>/dev/null); then echo "$v" | sed -E 's/([0-9]{8})([0-9]{6})$/\1T\2/'; else echo dev; fi`

# Show current version
show-version:
    @versionator output version

# Build the taskloom binary.
build:
    go build -ldflags "-X main.version={{version}}" -o {{TOP}}/bin/taskloom {{TOP}}/cmd/taskloom

# Install the taskloom binary to GOBIN (default ~/go/bin).
install:
    go install -ldflags "-X main.version={{version}}" {{TOP}}/cmd/taskloom

# Run the package tests under -race.
test *ARGS:
    go test -race {{ARGS}} {{TOP}}/...

# Vet all packages.
vet:
    go vet {{TOP}}/...

# Tidy module dependencies.
tidy:
    go mod tidy

# CI entrypoint: vet + race tests.
check: vet test
