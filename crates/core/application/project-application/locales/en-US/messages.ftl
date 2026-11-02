# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

unknown-project = no project { $id } is registered or has a database: nothing to repoint to
in-use = project { $id } is still the project of { $path }, which exists: pass --share if the two trees are meant to share it
not-orphan = { $id } is not an orphaned database: a live registered path, or this directory, still points at it, or it does not exist
