# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

output = cannot write the output: { $ext_err_txt }
no-link = plans { $command } needs --governs <task> (one or more) or --supersedes <plan>
hook-payload = stamp-plan: the hook payload is not JSON: { $ext_err_txt }
hook-no-file = stamp-plan: the hook payload names no file (expected tool_input.file_path or tool_input.notebook_path)
hook-failed = stamp-plan: { $error }
watch-failed = cannot watch the plan files: { $error }
