# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

no-home = { $var } is not set, so Go's task logs cannot be found
unreadable = cannot read { $path }: { $ext_err_txt }
malformed = { $path }:{ $line }: malformed task event: { $ext_err_txt }
no-op = { $path }:{ $line }: record has no op field
unknown-op = { $path }:{ $line }: unrecognized op { $op }: the log was likely written by a newer taskloom
