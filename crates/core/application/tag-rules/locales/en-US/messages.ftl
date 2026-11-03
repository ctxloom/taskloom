# Messages for the crate's refusals. Each id is a diagnostic's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

tag-blank = a tag was given blank: give one, or leave it out
tag-unparseable = the tag { $tag } is not a tag in tagma's grammar: { $ext_err_txt }
tag-operator-word = the tag { $tag } is a query operator (and, or, not, in any case), so no query could match it
tag-reserved-namespace = the tag { $tag } is in the namespace { $namespace }: tagma's and taskloom's namespaces are reserved for the store's tag rules
tag-rules = the store's tag rules refuse this write: { $findings } (see `taskloom manage schema show`)
tag-rule-warning = warning: { $tag } breaks the store's rule { $rule } ({ $args }), as it did before this write
tag-retired = { $tag } is retired: { $target } tags are plans now (taskloom plans add, taskloom plans link <plan> --governs <task>)
