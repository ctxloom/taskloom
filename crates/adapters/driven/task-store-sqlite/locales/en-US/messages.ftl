# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

create-dir = cannot create the store's directory { $path }: { $ext_err_txt }
unplaced = cannot find where the store { $path } is on disk: { $ext_err_txt }
no-runtime = cannot start a runtime to check the store: { $ext_err_txt }
predates-embedded = the store { $path } predates angzarr-embedded: move it to { $backup } to start afresh
no-output = cannot write the store's export: { $ext_err_txt }
