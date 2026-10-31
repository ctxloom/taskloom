task-store-sqlite-create-dir = cannot create the store's directory { $path }: { $ext_err_txt }
task-store-sqlite-unplaced = cannot find where the store { $path } is on disk: { $ext_err_txt }
task-store-sqlite-no-runtime = cannot start a runtime to check the store: { $ext_err_txt }
task-store-sqlite-predates-embedded = the store { $path } predates angzarr-embedded: move it to { $backup } to start afresh
task-store-sqlite-backup-unsafe = the store { $path } needs migrating, but its write-ahead log { $wal } is not empty, so a copy of the file alone might not back it up: it is neither copied nor migrated
task-store-sqlite-backup-failed = cannot back the store { $path } up to { $backup } before migrating it: { $ext_err_txt }
