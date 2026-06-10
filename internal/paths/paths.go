// Package paths provides the home-rooted and in-tree path conventions the
// task store shares with ctxloom. Data layout is unchanged by the extraction:
// task logs, the project registry, and the project marker all live where
// ctxloom put them, under ~/.ctxloom and <projectDir>/.ctxloom.
package paths

import (
	"os"
	"path/filepath"
)

const (
	// AppDirName is the name of the ctxloom directory; the task store shares
	// it rather than minting a parallel dot-dir.
	AppDirName = ".ctxloom"

	// ProjectsDir is the home-rooted subdirectory holding the project-identity
	// registry that maps a stable project-id to its current path.
	ProjectsDir = "projects"

	// IndexFileName is the name of the registry index file.
	IndexFileName = "index.yaml"

	// TasksDir is the home-rooted subdirectory holding per-project append-only
	// task logs, one <project-id>.jsonl per project.
	TasksDir = "tasks"

	// ProjectMarkerFileName is the in-tree marker carrying a project's stable
	// project-id. It lives at <projectDir>/.ctxloom/project-id and is gitignored:
	// private working-state identity must never ride a distributable tree.
	ProjectMarkerFileName = "project-id"

	// TasksLogExt is the suffix for a per-project task log file.
	TasksLogExt = ".jsonl"
)

// HomeProjectsDir returns ~/.ctxloom/projects — the home-rooted directory
// holding the project-identity registry.
func HomeProjectsDir() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, AppDirName, ProjectsDir), nil
}

// ProjectRegistryPath returns ~/.ctxloom/projects/index.yaml — the registry
// mapping each stable project-id to its current path.
func ProjectRegistryPath() (string, error) {
	root, err := HomeProjectsDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(root, IndexFileName), nil
}

// HomeTasksDir returns ~/.ctxloom/tasks — the home-rooted directory holding
// per-project append-only task logs.
func HomeTasksDir() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, AppDirName, TasksDir), nil
}

// TasksLogPath returns ~/.ctxloom/tasks/<project-id>.jsonl — the append-only
// task log for a project.
func TasksLogPath(projectID string) (string, error) {
	root, err := HomeTasksDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(root, projectID+TasksLogExt), nil
}

// ProjectMarkerPath returns <projectDir>/.ctxloom/project-id — the in-tree
// marker carrying the project's stable project-id.
func ProjectMarkerPath(projectDir string) string {
	return filepath.Join(projectDir, AppDirName, ProjectMarkerFileName)
}
