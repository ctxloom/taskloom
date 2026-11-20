package main

// Laid into Go taskloom's cmd/taskloom with `go test -overlay` by
// scripts/go-companion-fixtures.sh, so the Go checkout is not written. It
// dumps the companion text Rust taskloom keeps parity with, straight from
// the Go source: the loadout document. (Rust taskloom serves no MCP, so
// Go's MCP surface is not dumped.)

import (
	"os"
	"path/filepath"
	"testing"
)

func TestDumpCompanion(t *testing.T) {
	out := os.Getenv("GO_COMPANION_OUT")
	if out == "" {
		t.Skip("GO_COMPANION_OUT not set")
	}
	if err := os.WriteFile(filepath.Join(out, "loadout.yaml"), loadoutYAML, 0o644); err != nil {
		t.Fatal(err)
	}
}
