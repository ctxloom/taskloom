# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

unknown-status = { $status } is not a plan status: draft, active, done or abandoned
hand-superseded = a plan cannot be set to superseded: it is superseded by another plan's `supersedes:` (see `taskloom plans link --supersedes`)
no-change = plans set needs --title or --status
blank-title = a plan's title cannot be blank
not-a-plan-path = { $path } is not a plan file: its name must end .plan.md
supersede-self = plan { $plan } cannot supersede itself
supersede-cycle = plan { $plan } cannot supersede { $target }: { $target } already supersedes it, directly or through other plans
tag-query = { $query }: not a tag query: { $postfix } (queries are postfix, tags first and the operator after, as in human:decision/area:cli/and)
no-such-plan = there is no plan { $plan } in the plan roots (see `taskloom plans list`)
no-such-task = there is no task { $task } to govern (see `taskloom tasks list --all`)
ambiguous = several plan files have the id { $id }: name the file by its path ({ $paths })
no-id = the plan file { $path } has no id yet: `taskloom plans set { $path } --title ...` gives it one
unreadable = cannot read the plan file { $path }
frontmatter = { $path }: its frontmatter cannot be read or edited, so the file is left as it is: { $error }
file-exists = { $path } already exists
harps-exhausted = no free plan id after { $attempts } tries
finding-unreadable = { $path }: the file cannot be read
finding-frontmatter = { $path }: the frontmatter cannot be read: { $error }
finding-no-id = { $path }: the file has no id (taskloom gives it one the first time it writes the file)
finding-duplicate-id = { $path }: the id { $id } is also in { $others }
finding-tag = { $path }: the tag { $tag } is not admitted: { $error }
finding-governs = { $path }: governs { $task }, which is no task of this project
finding-supersedes = { $path }: supersedes { $plan }, which is no plan in the plan roots
finding-supersedes-self = { $path }: supersedes itself ({ $plan })
finding-status = { $path }: { $status } is not a plan status: draft, active, done or abandoned
finding-hand-superseded = { $path }: status: superseded is written by hand, but it is derived from another plan's supersedes: take it out
front-unclosed = the frontmatter's opening --- has no closing ---
front-yaml = the frontmatter is not YAML taskloom reads: { $ext_err_txt }
front-not-a-mapping = the frontmatter is not a mapping of keys
front-shape = the frontmatter's { $key } holds a value of the wrong shape
front-edit = the frontmatter's { $key } could not be edited: { $ext_err_txt }
