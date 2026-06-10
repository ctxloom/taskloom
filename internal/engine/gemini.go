package engine

import (
	"os"
	"path/filepath"
)

// Gemini registers MCP servers for the Gemini CLI: `.gemini/settings.json`
// under the project for project scope, under the home dir for user scope
// (same file shape, "mcpServers" table).
type Gemini struct{}

func (Gemini) Name() string { return "gemini" }

func (g Gemini) Present(dir string, global bool) bool {
	p, err := g.ConfigPath(dir, global)
	if err != nil {
		return false
	}
	return fileExists(filepath.Dir(p))
}

func (Gemini) ConfigPath(dir string, global bool) (string, error) {
	root := dir
	if global {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}
		root = home
	}
	return filepath.Join(root, ".gemini", "settings.json"), nil
}

func (Gemini) Install(config []byte, entry ServerEntry) ([]byte, error) {
	return jsonInstall(config, entry)
}

func (Gemini) Uninstall(config []byte, name string) ([]byte, error) {
	return jsonUninstall(config, name)
}

func (Gemini) Installed(config []byte, name string) (bool, error) {
	return jsonInstalled(config, name)
}
