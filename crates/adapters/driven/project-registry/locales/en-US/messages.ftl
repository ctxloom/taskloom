# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

no-home = no home directory: set { $var }
io = { $path }: { $ext_err_txt }
invalid-id = the project id { $id } from { $origin } is not a valid id: { $reason }
exhausted = no free project id after { $attempts } tries
no-cwd = the working directory cannot be read: { $ext_err_txt }
relative-path = { $path } is not an absolute path: nothing is written relative to the working directory
lock-timeout = the project registry's lock { $path } is still held after { $waited_ms }ms: another ctxloom or taskloom process holds it; try again once it exits
lock-replaced = the project registry's lock { $path } was replaced { $attempts } times while being taken: try again
changed = the project registry or a marker changed while the change was planned: nothing was changed; run it again
not-a-database = { $path } is not a project's database directory: not removed
stale-worktree = { $root } is a linked worktree whose git directory { $git_dir } is gone: run `git worktree repair` from its primary checkout, or remove the worktree
lock-failed = the project registry's lock cannot be taken: { $cause }
invalid-marker-path = the marker location { $path } is not usable: { $reason }
marker-conflict = { $path } names the project { $id } but { $other_path } names { $other_id }: remove the one that is wrong, and nothing is chosen between them
