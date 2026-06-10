package engine

import (
	"bytes"
	"encoding/json"
	"fmt"
)

// jsonInstall merges entry into a JSON config document under "mcpServers",
// preserving every other key (Claude Code and Gemini share this shape). A nil
// or empty config yields a fresh document.
func jsonInstall(config []byte, entry ServerEntry) ([]byte, error) {
	doc, err := jsonDoc(config)
	if err != nil {
		return nil, err
	}
	servers, ok := doc["mcpServers"].(map[string]any)
	if !ok {
		servers = map[string]any{}
		doc["mcpServers"] = servers
	}
	server := map[string]any{"command": entry.Command}
	if len(entry.Args) > 0 {
		args := make([]any, len(entry.Args))
		for i, a := range entry.Args {
			args[i] = a
		}
		server["args"] = args
	}
	servers[entry.Name] = server
	return jsonRender(doc)
}

// jsonUninstall removes the named server, preserving everything else.
func jsonUninstall(config []byte, name string) ([]byte, error) {
	doc, err := jsonDoc(config)
	if err != nil {
		return nil, err
	}
	if servers, ok := doc["mcpServers"].(map[string]any); ok {
		delete(servers, name)
	}
	return jsonRender(doc)
}

// jsonInstalled reports whether the named server exists in the document.
func jsonInstalled(config []byte, name string) (bool, error) {
	doc, err := jsonDoc(config)
	if err != nil {
		return false, err
	}
	servers, ok := doc["mcpServers"].(map[string]any)
	if !ok {
		return false, nil
	}
	_, present := servers[name]
	return present, nil
}

func jsonDoc(config []byte) (map[string]any, error) {
	if len(bytes.TrimSpace(config)) == 0 {
		return map[string]any{}, nil
	}
	var doc map[string]any
	if err := json.Unmarshal(config, &doc); err != nil {
		return nil, fmt.Errorf("parse config: %w", err)
	}
	return doc, nil
}

func jsonRender(doc map[string]any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetIndent("", "  ")
	if err := enc.Encode(doc); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}
