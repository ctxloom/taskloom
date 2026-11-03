# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

outside-work-root = { $path } is outside the work root: a plan's text is committed with the work tree
not-a-plan-file = { $path } is not a plan file: its name must end .plan.md
empty-path = the path given is empty
no-such-plan = there is no plan { $plan } (see `taskloom plans list`)
no-such-task = there is no task { $task } to govern (see `taskloom tasks list --all`)
supersede-cycle = plan { $plan } cannot supersede { $target }: { $target } already supersedes it, directly or through other plans
already-superseded = plan { $plan } is already superseded by { $by }: unlink that first
file-missing = there is no file at { $path }
file-exists = { $path } already exists: register it with --path, or move it aside
already-registered = { $path } is already plan { $plan }'s text
harps-exhausted = no free plan id after { $attempts } tries
gave-up = gave up after { $attempts } attempts: { $error }
