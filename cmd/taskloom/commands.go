package main

import (
	"sort"
	"strings"

	"github.com/spf13/cobra"

	"github.com/ctxloom/shared/iox"
	"github.com/ctxloom/taskloom/operations"
)

var (
	tasksListStatuses []string
	tasksListTerm     string
	tasksListJSON     bool
	tasksListAll      bool
)

var listCmd = &cobra.Command{
	Use:   "list",
	Short: "List tasks, optionally filtered by status or term",
	RunE: func(cmd *cobra.Command, args []string) error {
		res, err := operations.ListTasks(taskContext(), tasksListStatuses, tasksListTerm, tasksListAll, false)
		if err != nil {
			return err
		}
		warnTask(res.Warning)
		if tasksListJSON {
			return writeJSON(cmd.OutOrStdout(), res.Tasks)
		}
		// Name the resolved store: in multi-root workspaces (several .ctxloom
		// trees under one repo), which project a listing came from is the
		// first thing a confused reader needs to know.
		w := iox.NewErrWriter(cmd.OutOrStdout())
		if res.ProjectDir != "" {
			w.Printf("Project: %s (%s)\n\n", res.ProjectDir, res.ProjectID)
		} else {
			w.Printf("Project: %s\n\n", res.ProjectID)
		}
		if err := w.Err(); err != nil {
			return err
		}
		return renderTaskTable(cmd.OutOrStdout(), res.Tasks)
	},
}

var (
	tasksAddStatus  string
	tasksAddTrigger string
)

var addCmd = &cobra.Command{
	Use:   "add <text>",
	Short: "Add a new task",
	Args:  cobra.MinimumNArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		text := strings.Join(args, " ")
		res, err := operations.AddTask(taskContext(), text, tasksAddStatus, tasksAddTrigger)
		if err != nil {
			return err
		}
		warnTask(res.Warning)
		noteTaskProject(res.ProjectID, res.ProjectDir)
		task := res.Task
		w := iox.NewErrWriter(cmd.OutOrStdout())
		w.Printf("%s\t%s\t%s\n", task.HarpID, task.Status, task.Text)
		return w.Err()
	},
}

var tasksStatusTrigger string

var statusCmd = &cobra.Command{
	Use:   "status <harp-id> <status>",
	Short: "Change the status of a task",
	Long: `Change the status of a task.

Use "Deferred" with --trigger to park a task on a named revive condition; the
task then hides from the default list until the trigger fires. A task already
carrying a trigger keeps it when re-deferred, so --trigger is optional then.`,
	Args: cobra.ExactArgs(2),
	RunE: func(cmd *cobra.Command, args []string) error {
		res, err := operations.SetTaskStatus(taskContext(), args[0], args[1], tasksStatusTrigger)
		if err != nil {
			return err
		}
		warnTask(res.Warning)
		noteTaskProject(res.ProjectID, res.ProjectDir)
		task := res.Task
		w := iox.NewErrWriter(cmd.OutOrStdout())
		w.Printf("%s\t%s\t%s\n", task.HarpID, task.Status, task.Text)
		return w.Err()
	},
}

var editCmd = &cobra.Command{
	Use:   "edit <harp-id> <text>",
	Short: "Replace a task's text in place (full new text)",
	Long: `Replace a task's text, keyed by its harp ID.

The entire text is replaced with what you pass (not patched); the task's
status and any Deferred trigger are left unchanged.`,
	Args: cobra.MinimumNArgs(2),
	RunE: func(cmd *cobra.Command, args []string) error {
		text := strings.Join(args[1:], " ")
		res, err := operations.EditTask(taskContext(), args[0], text)
		if err != nil {
			return err
		}
		warnTask(res.Warning)
		noteTaskProject(res.ProjectID, res.ProjectDir)
		task := res.Task
		w := iox.NewErrWriter(cmd.OutOrStdout())
		w.Printf("%s\t%s\t%s\n", task.HarpID, task.Status, task.Text)
		return w.Err()
	},
}

var summaryCmd = &cobra.Command{
	Use:   "summary",
	Short: "Show per-status counts and active in-progress tasks",
	RunE: func(cmd *cobra.Command, args []string) error {
		res, err := operations.ListTasks(taskContext(), nil, "", false, true)
		if err != nil {
			return err
		}
		warnTask(res.Warning)
		sum := res.Summary
		// Stable order so output is diffable.
		keys := make([]string, 0, len(sum.Counts))
		for k := range sum.Counts {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		w := iox.NewErrWriter(cmd.OutOrStdout())
		for _, k := range keys {
			w.Printf("%s\t%d\n", k, sum.Counts[k])
		}
		if len(sum.InProgress) > 0 {
			w.Printf("\nIn-progress: %s\n", strings.Join(sum.InProgress, ", "))
		}
		return w.Err()
	},
}

func init() {
	listCmd.Flags().StringSliceVar(&tasksListStatuses, "status", nil, "filter by status (repeatable)")
	listCmd.Flags().StringVar(&tasksListTerm, "term", "", "filter by case-insensitive substring of task text")
	listCmd.Flags().BoolVar(&tasksListJSON, "json", false, "emit JSON instead of a table (for jq)")
	listCmd.Flags().BoolVar(&tasksListAll, "all", false, "include completed (Done/Archived) tasks, hidden by default")

	addCmd.Flags().StringVar(&tasksAddStatus, "status", "", "initial status (default: \"To Do\")")
	addCmd.Flags().StringVar(&tasksAddTrigger, "trigger", "", "revive condition for a Deferred task (required when --status Deferred)")

	statusCmd.Flags().StringVar(&tasksStatusTrigger, "trigger", "", "revive condition when setting status to Deferred")

	rootCmd.AddCommand(listCmd, addCmd, statusCmd, editCmd, summaryCmd)
}
