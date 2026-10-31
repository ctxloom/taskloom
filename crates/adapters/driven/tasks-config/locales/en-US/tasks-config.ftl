tasks-config-unreadable = cannot read { $path }: { $ext_err_txt }
tasks-config-unwritable = cannot write { $path }: { $ext_err_txt }
tasks-config-malformed = { $path } is not taskloom's config: { $ext_err_txt }
tasks-config-newer = { $path } is schema_version { $version }, newer than the { $current } this taskloom reads
tasks-config-unknown-key = { $source } sets { $key }, which is no config key taskloom knows (known: { $known })
tasks-config-not-an-override = --config-set { $entry }: expected <path>=<value>
tasks-config-blank-item = { $source }: { $value } has a blank item: give one, or leave it out
tasks-config-key-moved = { $source } sets { $key }, which moved into the project's store: run `taskloom schema import-config` in the work root to move a project's .taskloom/config.yaml into its store, and delete it from anywhere else
tasks-config-no-work-root = there is no work root here, so no project config to write tag permits to
tasks-config-edit-refused = { $path } was left as it was: editing it would not have read back as meant ({ $ext_err_txt }); this is a bug in taskloom
