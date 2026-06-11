package main

import (
	"fmt"
	"io"

	"github.com/spf13/cobra"
)

// version is stamped at build time from versionator via
// -ldflags "-X main.version=<v>" (see justfile); "dev" for a bare go build.
var version = "dev"

// versionInfo is the machine-readable shape of `taskloom version --format
// json`. ctxloom probes it at boot to report companion versions, so the
// field set is a cross-binary contract (ltk and ctxloom emit the same shape).
type versionInfo struct {
	Name    string `json:"name"`
	Version string `json:"version"`
}

var versionFormat string

var versionCmd = &cobra.Command{
	Use:   "version",
	Short: "Print the taskloom version",
	RunE: func(cmd *cobra.Command, _ []string) error {
		return printVersion(cmd.OutOrStdout(), versionFormat)
	},
}

func printVersion(w io.Writer, format string) error {
	switch format {
	case "text":
		_, err := fmt.Fprintln(w, version)
		return err
	case "json":
		return writeJSON(w, versionInfo{Name: "taskloom", Version: version})
	default:
		return fmt.Errorf("unknown format %q (supported: text, json)", format)
	}
}

func init() {
	rootCmd.Version = version
	versionCmd.Flags().StringVar(&versionFormat, "format", "text", "Output format: text or json ({name, version})")
	rootCmd.AddCommand(versionCmd)
}
