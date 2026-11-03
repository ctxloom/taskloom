# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

unreadable = cannot read { $path }: { $ext_err_txt }
unwritable = cannot write { $path }: { $ext_err_txt }
not-a-permit = { $path }: tag_permits: namespaces: { $namespace } is { $value }, which is neither open nor a list of keys
newer = { $path } is schema_version { $version }, newer than the { $current } this taskloom reads
unknown-key = { $source } sets { $key }, which is no config key taskloom knows (known: { $known })
not-an-override = --config-set { $entry }: expected <path>=<value>
blank-item = { $source }: { $value } has a blank item: give one, or leave it out
key-moved = { $source } sets { $key }, which moved into the project's store: run `taskloom manage migrate schema` in the work root to move a project's .taskloom/config.yaml into its store, and delete it from anywhere else
no-work-root = there is no work root here, so no project config to write tag permits to
edit-refused = { $path } was left as it was: editing it would not have read back as meant ({ $ext_err_txt }); this is a bug in taskloom
