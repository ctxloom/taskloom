package main

import (
	"github.com/spf13/cobra"
)

// version is stamped at build time via
// -ldflags "-X main.version=<v>"; "dev" for a bare go build.
var version = "dev"

var versionCmd = &cobra.Command{
	Use:   "version",
	Short: "Print the taskloom version",
	Run: func(cmd *cobra.Command, _ []string) {
		cmd.Println(version)
	},
}

func init() {
	rootCmd.Version = version
	rootCmd.AddCommand(versionCmd)
}
