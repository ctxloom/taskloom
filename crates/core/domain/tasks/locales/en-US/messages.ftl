# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

harp-id-required = a task needs a harp id
task-exists = task { $harp_id } already exists
no-task = there is no such task
removed = task { $harp_id } was removed
text-required = a task needs text
status-required = a status change needs a status, a trigger, or the trigger cleared
status-reserved = the status { $status } is not allowed: '@' is reserved for status classes
trigger-required = a Deferred task needs a trigger: the condition that should revive it
tag-required = a retag needs at least one tag
blank = the { $field } given is blank: give one, or leave it out
trigger-with-status = a trigger defers the task to { $defers }, so it cannot be given with the status { $status }: give the trigger alone, or the status alone
unresolved = the command carries no { $field } status: the application resolves it from the configured statuses
schema-tag-required = a schema declaration needs at least one tag to add or remove
schema-not-config = { $tag } is not a rule: a schema declares only tagma.* and taskloom.* tags
