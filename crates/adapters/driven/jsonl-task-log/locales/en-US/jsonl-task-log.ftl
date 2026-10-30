jsonl-task-log-no-home = { $var } is not set, so Go's task logs cannot be found
jsonl-task-log-unreadable = cannot read { $path }: { $ext_err_txt }
jsonl-task-log-malformed = { $path }:{ $line }: malformed task event: { $ext_err_txt }
jsonl-task-log-no-op = { $path }:{ $line }: record has no op field
jsonl-task-log-unknown-op = { $path }:{ $line }: unrecognized op { $op }: the log was likely written by a newer taskloom
