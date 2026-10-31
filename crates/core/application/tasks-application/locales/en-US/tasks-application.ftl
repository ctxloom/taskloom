tasks-application-gave-up = gave up after { $attempts } attempts: { $error }
tasks-application-harps-exhausted = no free task id after { $attempts } tries
tasks-application-vanished = task { $harp_id } was written but cannot be read back
tasks-application-no-project = there is no project here yet, so no task to change
tasks-application-no-jsonl-log = Go taskloom has no task log for project { $project }
tasks-application-unknown-status-class = { $status } is no status class (the classes are { $classes }): '@' is reserved for them, so no status starts with it
tasks-application-unknown-sort = { $sort } is no sort: the only one is priority
tasks-application-blank = the { $field } given is blank: give one, or leave it out
tasks-application-trigger-and-clear = a trigger was given and cleared at once: give a trigger, or clear the one held, not both
tasks-application-add-request-id-collides = every one of the { $probes } task ids request id { $request_id } derives holds another request's task: give this add a new request id
tasks-application-priority-no-priority-fn = --sort priority is meaningless here: this project's tag_schema declares no priority_fn, so every task's raw score is 0 and the ranking reflects nothing
tasks-application-priority-all-tied = --sort priority is meaningless here: every active task ties at the same raw priority score (only { $scored } of { $live } active tasks carry a tag any priority_fn/decay_fn formula actually reads) — the ranking reflects nothing
tasks-application-priority-inert = priority_fn/decay_fn reads { $target }, which none of the { $live } active tasks carries — that term is inert and moves no task's rank
tasks-application-priority-thin = { $uncovered } of { $live } active tasks carry no { $target } — they rank as if it were absent, on a formula that reads it
tasks-application-priority-in-project = project { $project }: { $warning }
tasks-application-not-found = no task holds { $count ->
    [one] this harp id
   *[other] these harp ids
}: { $harp_ids } (see `taskloom list`)
tasks-application-ambiguous = { $count ->
    [one] a harp id is
   *[other] harp ids are
} held by more than one project ({ $held }): pass --project <id> instead of --global to choose one
tasks-application-permit-malformed = { $permit } is not a permit: give a namespace (area:), a namespace's key (area:cli) or a flat tag (urgent), with no value
tasks-application-permit-not-declared = { $permit } is not among this store's tag permits, so there is nothing to revoke
tasks-application-permit-in-use = { $permit } cannot be revoked: { $count ->
    [one] this task still carries it
   *[other] these tasks still carry it
}: { $harp_ids } (retag them first)
tasks-application-import-displaced = another task holds this harp in Go's log: run Go's `taskloom repair`, then import again
tasks-application-import-harp-taken = a task not imported from this record already holds this harp
tasks-application-import-task-not-imported = its task was not imported
tasks-application-tag-blank = a tag was given blank: give one, or leave it out
tasks-application-tag-unparseable = the tag { $tag } is not a tag in tagma's grammar: { $ext_err_txt }
tasks-application-tag-operator-word = the tag { $tag } is a query operator (and, or, not, in any case), so no query could match it
tasks-application-tag-reserved-namespace = the tag { $tag } is in the namespace { $namespace }: tagma's and taskloom's namespaces are reserved for the store's tag rules
tasks-application-tag-rules = the store's tag rules refuse this write: { $findings } (see `taskloom schema show`)
tasks-application-tag-rule-warning = warning: { $tag } breaks the store's rule { $rule } ({ $args }), as it did before this write
tasks-application-dead-letter = { $line }
tasks-application-tag-query = the tag query { $query } is malformed ({ $postfix }): { $grammar }
tasks-application-lint-rule = { $tags }: { $rule } ({ $args })
tasks-application-lint-formula-enum = { $facet } on { $target } references the placeholder { $placeholder }, but { $value } is not one of { $reference }'s declared enum values: { $members }
tasks-application-prune-no-status = a prune needs at least one status to select by: an empty status set is never read as every status
tasks-application-prune-age-required = a prune needs an age: an offset like 30d, or a date like 2026-01-01
tasks-application-prune-age-not-positive = the age { $age } is not a positive age
tasks-application-prune-age-unreadable = the age { $age } is neither an offset (30d, 12w, 720h) nor a date (2026-01-01)
tasks-application-tag-retired = { $tag } is retired: { $target } tags are plans now (taskloom plan add, taskloom plan link <plan> --governs <task>)
