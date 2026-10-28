tasks-config-unreadable = cannot read { $path }: { $detail }
tasks-config-unwritable = cannot write { $path }: { $detail }
tasks-config-malformed = { $path } is not taskloom's config: { $detail }
tasks-config-newer = { $path } is schema_version { $version }, newer than the { $current } this taskloom reads
tasks-config-unknown-key = { $source } sets { $key }, which is no config key taskloom knows (known: { $known })
tasks-config-not-an-override = --config-set { $entry }: expected <path>=<value>
tasks-config-unterminated = { $source }: { $value } has an unterminated quote
tasks-config-blank-item = { $source }: { $value } has a blank item: give one, or leave it out
tasks-config-permits-outside-project = { $path } sets tag_permits, which only a project's own .taskloom/config.yaml sets
tasks-config-file-only = { $source } sets { $key }, which only the project's .taskloom/config.yaml sets (taskloom permit and revoke change it)
tasks-config-no-work-root = there is no work root here, so no project config to write tag permits to
tasks-config-edit-refused = { $path } was left as it was: editing its tag_permits would not have read back as meant ({ $detail }); this is a bug in taskloom
