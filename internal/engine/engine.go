// Package engine adapts the agent backends' MCP-server config formats
// (Claude Code, Gemini CLI, Codex) to one Install/Uninstall surface, so
// `taskloom manage` can register the `taskloom mcp` server without ctxloom.
// Engine-specific details — config paths, the on-disk format — live entirely
// in the engine's own file; adding an engine never touches the manage command.
package engine

import (
	"fmt"
	"os"
	"strings"
)

// fileExists reports whether the path exists (file or directory).
func fileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}

// ServerEntry is the engine-neutral description of one MCP server
// registration: the key under the backend's server table plus the command line
// that serves it.
type ServerEntry struct {
	Name    string
	Command string
	Args    []string
}

// TaskloomEntry is the registration `taskloom manage` installs.
func TaskloomEntry() ServerEntry {
	return ServerEntry{Name: "taskloom", Command: "taskloom", Args: []string{"mcp"}}
}

// Engine is everything specific to one agent backend's MCP configuration.
type Engine interface {
	// Name is the engine identifier (e.g. "claude-code").
	Name() string
	// Present reports whether this backend appears to be in use for the given
	// scope: its config file or well-known directory exists. Auto-registration
	// only touches backends that are present; an explicit --engine overrides.
	Present(dir string, global bool) bool
	// ConfigPath returns the backend's MCP config file for the given scope:
	// global (user-level, under the home dir) or project (under dir).
	ConfigPath(dir string, global bool) (string, error)
	// Install merges entry into the config bytes (empty in → fresh config).
	// Idempotent; foreign keys and servers are preserved byte-for-byte where
	// the format allows.
	Install(config []byte, entry ServerEntry) ([]byte, error)
	// Uninstall removes the named server from the config bytes. Removing a
	// server that is not present is a no-op, not an error.
	Uninstall(config []byte, name string) ([]byte, error)
	// Installed reports whether the named server is present in the config.
	Installed(config []byte, name string) (bool, error)
}

// engines is the registry of known engines.
func engines() []Engine {
	return []Engine{ClaudeCode{}, Gemini{}, Codex{}}
}

// All returns every known engine, for "register wherever present" flows.
func All() []Engine {
	return engines()
}

// engineAliases maps accepted alternate spellings to canonical engine names.
var engineAliases = map[string]string{
	"claudecode": "claude-code",
	"claude":     "claude-code",
}

// Get returns the engine for a name: the canonical name or a declared alias,
// case-insensitively. No prefix matching — a typo must error rather than
// silently pick an engine.
func Get(name string) (Engine, error) {
	want := strings.ToLower(name)
	if canonical, ok := engineAliases[want]; ok {
		want = canonical
	}
	for _, e := range engines() {
		if e.Name() == want {
			return e, nil
		}
	}
	return nil, fmt.Errorf("unknown engine %q", name)
}
