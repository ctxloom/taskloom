package engine

import (
	"bytes"
	"fmt"
	"os"
	"path/filepath"

	toml "github.com/pelletier/go-toml/v2"
)

// Codex registers MCP servers for Codex: `.codex/config.toml` under the
// project for project scope, under the home dir for user scope. Servers live
// in the `[mcp_servers.<name>]` table.
//
// The merge round-trips through a TOML document model, so unknown tables and
// keys survive; TOML comments do not (the file is machine-managed in
// practice — ctxloom regenerates it wholesale).
type Codex struct{}

func (Codex) Name() string { return "codex" }

func (c Codex) Present(dir string, global bool) bool {
	p, err := c.ConfigPath(dir, global)
	if err != nil {
		return false
	}
	return fileExists(filepath.Dir(p))
}

func (Codex) ConfigPath(dir string, global bool) (string, error) {
	root := dir
	if global {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}
		root = home
	}
	return filepath.Join(root, ".codex", "config.toml"), nil
}

func (Codex) Install(config []byte, entry ServerEntry) ([]byte, error) {
	doc, err := tomlDoc(config)
	if err != nil {
		return nil, err
	}
	servers, ok := doc["mcp_servers"].(map[string]any)
	if !ok {
		servers = map[string]any{}
		doc["mcp_servers"] = servers
	}
	server := map[string]any{"command": entry.Command}
	if len(entry.Args) > 0 {
		server["args"] = entry.Args
	}
	servers[entry.Name] = server
	return toml.Marshal(doc)
}

func (Codex) Uninstall(config []byte, name string) ([]byte, error) {
	doc, err := tomlDoc(config)
	if err != nil {
		return nil, err
	}
	if servers, ok := doc["mcp_servers"].(map[string]any); ok {
		delete(servers, name)
	}
	return toml.Marshal(doc)
}

func (Codex) Installed(config []byte, name string) (bool, error) {
	doc, err := tomlDoc(config)
	if err != nil {
		return false, err
	}
	servers, ok := doc["mcp_servers"].(map[string]any)
	if !ok {
		return false, nil
	}
	_, present := servers[name]
	return present, nil
}

func tomlDoc(config []byte) (map[string]any, error) {
	doc := map[string]any{}
	if len(bytes.TrimSpace(config)) == 0 {
		return doc, nil
	}
	if err := toml.Unmarshal(config, &doc); err != nil {
		return nil, fmt.Errorf("parse config: %w", err)
	}
	return doc, nil
}
