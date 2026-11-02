# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

already-added = this plan already exists
no-such-plan = there is no such plan
empty-title = a plan needs a title
initial-status = a plan starts draft or active, not { $status }
status-unchanged = the plan is already { $status }
transition = a plan cannot go from { $from } to { $to }
superseded-by-supersede-only = a plan is superseded only by another plan's supersede link (`taskloom plan link <plan> --supersedes <this one>`)
already-governs = the plan already governs task { $task }
not-governed = the plan does not govern task { $task }
supersedes-itself = a plan cannot supersede itself
already-supersedes = the plan already supersedes plan { $plan }
not-superseded = the plan does not supersede plan { $plan }
hash-unchanged = the plan's text has not changed
bad-hash = { $hash } is not a sha-256: 64 lowercase hex digits
