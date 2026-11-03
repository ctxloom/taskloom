# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

no-project = no project here yet; add a task first (taskloom tasks add "<task text>")
output = cannot write the change stream: { $ext_err_txt }
cursor-malformed = --since is not a cursor: pass the cursor of a watch line, unchanged
cursor-version = the cursor is version { $found }; this taskloom reads version { $supported }
cursor-foreign = the cursor belongs to project { $cursor_project }, not to { $project }
unknown-project = no project { $project } has a store here
