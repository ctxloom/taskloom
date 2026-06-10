package engine

import (
	"os"
	"path/filepath"
)

// ClaudeCode registers MCP servers for Claude Code: project scope is the
// repo-root `.mcp.json`; user scope is `~/.claude.json` (both hold a
// top-level "mcpServers" table).
type ClaudeCode struct{}

func (ClaudeCode) Name() string { return "claude-code" }

func (c ClaudeCode) Present(dir string, global bool) bool {
	if global {
		home, err := os.UserHomeDir()
		if err != nil {
			return false
		}
		return fileExists(filepath.Join(home, ".claude.json")) || fileExists(filepath.Join(home, ".claude"))
	}
	return fileExists(filepath.Join(dir, ".mcp.json")) || fileExists(filepath.Join(dir, ".claude"))
}

func (ClaudeCode) ConfigPath(dir string, global bool) (string, error) {
	if global {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}
		return filepath.Join(home, ".claude.json"), nil
	}
	return filepath.Join(dir, ".mcp.json"), nil
}

func (ClaudeCode) Install(config []byte, entry ServerEntry) ([]byte, error) {
	return jsonInstall(config, entry)
}

func (ClaudeCode) Uninstall(config []byte, name string) ([]byte, error) {
	return jsonUninstall(config, name)
}

func (ClaudeCode) Installed(config []byte, name string) (bool, error) {
	return jsonInstalled(config, name)
}
