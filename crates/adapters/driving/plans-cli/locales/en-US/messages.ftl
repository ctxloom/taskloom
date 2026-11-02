# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

output = cannot write the output: { $ext_err_txt }
unknown-status = { $status } is not a plan status: draft, active, superseded, done or abandoned
no-link = plan { $command } needs --governs <task> (one or more) or --supersedes <plan>
file-missing = plan { $plan }'s file is missing: { $path }
