# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

create-dir = cannot create the store's directory { $path }: { $ext_err_txt }
unplaced = cannot find where the store { $path } is on disk: { $ext_err_txt }
no-runtime = cannot start a runtime to check the store: { $ext_err_txt }
predates-embedded = the store { $path } predates angzarr-embedded: move it to { $backup } to start afresh
older-generation = the store { $path } was written by an older build of taskloom, in a form this build cannot read, so it is left untouched as a read-only archive: move state.db (and state.db-wal and state.db-shm, if they exist) aside, e.g. to { $archive }, and read it with the pre-port taskloom binary; this build then starts a fresh store
no-output = cannot write the store's export: { $ext_err_txt }
restrict = cannot make the store's { $path } private to its owner, so it is not opened: { $ext_err_txt }
