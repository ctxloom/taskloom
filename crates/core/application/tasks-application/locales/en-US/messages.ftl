# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

gave-up = gave up after { $attempts } attempts: { $error }
harps-exhausted = no free task id after { $attempts } tries
vanished = task { $harp_id } was written but cannot be read back
no-project = there is no project here yet, so no task to change
no-jsonl-log = Go taskloom has no task log for project { $project }
unknown-status-class = { $status } is no status class (the classes are { $classes }): '@' is reserved for them, so no status starts with it
unknown-sort = { $sort } is no sort: the only one is priority
blank = the { $field } given is blank: give one, or leave it out
trigger-and-clear = a trigger was given and cleared at once: give a trigger, or clear the one held, not both
add-request-id-collides = every one of the { $probes } task ids request id { $request_id } derives holds another request's task: give this add a new request id
priority-no-priority-fn = --sort priority is meaningless here: this project's tag_schema declares no priority_fn, so every task's raw score is 0 and the ranking reflects nothing
priority-all-tied = --sort priority is meaningless here: every active task ties at the same raw priority score (only { $scored } of { $live } active tasks carry a tag any priority_fn/decay_fn formula actually reads) — the ranking reflects nothing
priority-inert = priority_fn/decay_fn reads { $target }, which none of the { $live } active tasks carries — that term is inert and moves no task's rank
priority-thin = { $uncovered } of { $live } active tasks carry no { $target } — they rank as if it were absent, on a formula that reads it
priority-in-project = project { $project }: { $warning }
not-found = no task holds { $count ->
    [one] this harp id
   *[other] these harp ids
}: { $harp_ids } (see `taskloom tasks list`)
ambiguous = { $count ->
    [one] a harp id is
   *[other] harp ids are
} held by more than one project ({ $held }): pass --project <id> to choose one
permit-malformed = { $permit } is not a permit: give a namespace (area:), a namespace's key (area:cli) or a flat tag (urgent), with no value
permit-not-declared = { $permit } is not among this store's tag permits, so there is nothing to revoke
permit-in-use = { $permit } cannot be revoked: { $count ->
    [one] this task still carries it
   *[other] these tasks still carry it
}: { $harp_ids } (retag them first)
import-displaced = another task holds this harp in Go's log: run Go's `taskloom repair`, then import again
import-harp-taken = a task not imported from this record already holds this harp
import-task-not-imported = its task was not imported
tag-blank = a tag was given blank: give one, or leave it out
tag-unparseable = the tag { $tag } is not a tag in tagma's grammar: { $ext_err_txt }
tag-operator-word = the tag { $tag } is a query operator (and, or, not, in any case), so no query could match it
tag-reserved-namespace = the tag { $tag } is in the namespace { $namespace }: tagma's and taskloom's namespaces are reserved for the store's tag rules
tag-rules = the store's tag rules refuse this write: { $findings } (see `taskloom manage schema show`)
tag-rule-warning = warning: { $tag } breaks the store's rule { $rule } ({ $args }), as it did before this write
tag-query = the tag query { $query } is malformed ({ $postfix }): { $grammar }
lint-rule = { $tags }: { $rule } ({ $args })
lint-formula-enum = { $facet } on { $target } references the placeholder { $placeholder }, but { $value } is not one of { $reference }'s declared enum values: { $members }
prune-no-status = a prune needs at least one status to select by: an empty status set is never read as every status
prune-age-required = a prune needs an age: an offset like 30d, or a date like 2026-01-01
prune-age-not-positive = the age { $age } is not a positive age
prune-age-unreadable = the age { $age } is neither an offset (30d, 12w, 720h) nor a date (2026-01-01)
tag-retired = { $tag } is retired: { $target } tags are plans now (taskloom plans add, taskloom plans link <plan> --governs <task>)
