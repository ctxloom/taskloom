# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

output = cannot write the output: { $ext_err_txt }
import-incomplete = not everything was imported: { $stopped } projects stopped, { $lost } records not imported
usage = { $ext_err_txt }
project = project { $root } ({ $id })
unrooted-project = project { $id }
no-project = no project here yet: nothing to list
loadout-format = the loadout is printed only as yaml, not { $format }
every-project = no project here: { $directory } is in no git repository, has no CTXLOOM_ROOT and no project of its own, so this reads every project; pass --project <id>, or work in a project, to read one
hidden = { $hidden } more matching tasks hidden by the default view ({ $completed } completed, { $deferred } deferred): add --all to include them
omitted = { $omitted } more tasks omitted by --limit: raise or drop it to see them (summary's counts are unaffected)
lint-violations = { $count } tag-standard violations found
lint-checked-nothing = the check found no target: the tag schema declares no enum or range, so a clean sweep proves nothing
schema-findings = tagma's check of the store's tag rules found { $count } (listed above)
nothing-to-prune = no project here yet: nothing to prune
report-only = removed nothing: this was a report, not a removal; to apply exactly this plan, run it again with --yes: { $command }
removed = removed: use { $instead }
plan-import-removed = removed: plans are files; there is nothing to import
preview = changed nothing: this was a preview; to apply exactly this, run it again with --yes: { $command }
