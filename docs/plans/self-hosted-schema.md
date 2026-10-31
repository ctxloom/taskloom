# Self-hosted tag schema: the rules travel with the data

RED for the owner's review (2026-10-30). Tests are the proposal;
new bodies are `todo!()`.

**Boundary.** tagma (33d0478) interprets every rule: `Rules::from_tags`,
`validate` (Finding + Origin), `check`, and the permit/enum/range/arity/
type/keys-per-item facets. taskloom owns storing the config tags, seeding,
the CLI, refusing on `Introduced`, the semver comparator it registers, the
`taskloom.*` formulas, and `admit`'s operator-word/reserved-namespace checks
(tagma's parser takes `and` as an ordinary key).

**Entity.** One schema stream per store: domain `schema`, fixed root
`compute_root("schema", "store")`, so no task list ever projects it.
`SchemaState { tags, declared }`.

**Command / event.** `Declare { add, remove }`, Retag-shaped (adds, then
removes); refused when empty (`tasks-schema-tag-required`) or when a tag is
outside `tagma.*`/`taskloom.*` (`tasks-schema-not-config`). It records
`SchemaRetagged { tags }`, the whole set after the change, and records
nothing when nothing changes. Malformed config is recorded and reported by
`check`. Ordering is per stream only.

**Read model.** `task_lists::schema::SchemaView`: projects
`SchemaRetagged`, exports it in tagma syntax (sorted), and hands its tags
to tagma.

**Write path.** Every add or tag is validated against the store's rules.
An `Introduced` finding refuses the write (`tasks-application-tag-rules`,
naming tagma's code). `Existing` findings come back as
`Written::warnings()`, on stderr or in the JSON `warnings`.

**Seed.** `SEEDED_SCHEMA` (33 tags) is the old 16 rewritten: one tag per
enum value, `lo..hi` ranges, `tagma.keys-per-item:area=1`/`repo=1` (which
replace `ONE_KEY`), and `taskloom.priority_fn`/`decay_fn`. It is written
when a store is made, and on first open of a store with no schema stream.
No permits, so the world stays open.

**CLI.** `schema` / `schema show` (same JSON shape as now), `schema
export`, `schema check` (exits non-zero on findings), `schema declare
--add/--remove`, `schema import-config`. `permit X` adds
`tagma.permit:"X"=true` (`area:` becomes `"area:*"`). The first permit also
adds `"*:*"=false` plus a permit for each target the schema declares rules
on. `revoke` is refused while any task carries a tag it admits, and the
error lists them. The loadout names each of these commands as person-only.

**Migration.** The `tag_schema`/`tag_permits` keys (home, work root, env,
`--config-set`) are refused with `tasks-config-key-moved`, which names
`taskloom schema import-config`. That command declares the work root's
keys in the store once, then removes them from the file, keeping its other
keys and comments.

**Deleted by the implementation.** `TagSchema`, `Range`, `SchemaError`,
`Why`, `FACETS`, `DEFAULT_TAG_SCHEMA`; `tags::{permitted, check_scalars}`
and `admit`'s value checks; `permit::Permit`; `lint::ONE_KEY`
and `Finding::ManyKeys`; `Configuration::{tag_schema, tag_permits}`,
`ConfigurationWriter`, `TagPermits`, `NamespacePermit`; tasks-config's
`permits`, plus `edit` once the import is retired.
