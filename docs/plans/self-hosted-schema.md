# Self-hosted tag schema: the rules travel with the data

Reviewed by the owner (2026-10-30) as red tests; implemented on
`red/self-hosted-schema` (see "Implementation notes" at the end).

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
An `Introduced` finding refuses the write (`tag-rules-tag-rules`,
naming tagma's code). `Existing` findings come back as
`Written::warnings()`, on stderr or in the JSON `warnings`.

**Seed.** `SEEDED_SCHEMA` is the old 16 rewritten: one tag per enum value,
`lo..hi` ranges, `tagma.keys-per-item:area=1`/`repo=1` (which replace
`ONE_KEY`), and `taskloom.priority_fn`/`decay_fn`; and its permits, for
every store is an allowlist from when it is made (the owner's ruling F6,
2026-11-02): one for each target the seed constrains, and the core
vocabulary every project copies (the factual flags, `human:`'s keys,
`relates:`, `touches:`, `sig:`, `queue:`, `landed:`, `platform:`'s keys,
and the parking flags `icebox` and `someday-maybe`, flat tags, the owner's
request of 2026-11-12; the loadout's tag axes say what each means).
A project's own `area:` and `repo:` keys it permits itself. A seed grown
later reaches no store already declared: such a store is read as it holds
its rules, so a person adds a new default with `manage schema permits add`. The seed is
written when a store is made, and on first open of a store with no schema
stream; a store holding no permit as a presence (one from before F6, open
or closed by `"*:*"=false`) has the seed's permits written on its first
open, its `=true` permits kept as presences, its other valued ones dropped.

**CLI.** `manage schema show` (same JSON shape as now), `manage schema
export`, `manage schema check` (exits non-zero on findings), `manage schema
declare --add/--remove` (a preview unless `--yes`), `manage migrate schema`.
`manage schema permits add X` adds
`tagma.permit:"X"`, a presence (`area:` becomes `"area:*"`), and nothing
else. `permits remove` is refused while any task carries a tag it admits, and the
error lists them. The loadout names each of these commands as person-only.

**Migration.** The `tag_schema`/`tag_permits` keys (home, work root, env,
`--config-set`) are refused with `tasks-config-key-moved`, which names
`taskloom manage migrate schema`. That command declares the work root's
keys in the store once, then removes them from the file, keeping its other
keys and comments.

**Deleted by the implementation.** `TagSchema`, `Range`, `SchemaError`,
`Why`, `FACETS`, `DEFAULT_TAG_SCHEMA`; `tags::{permitted, check_scalars}`
and `admit`'s value checks; `permit::Permit`; `lint::ONE_KEY`
and `Finding::ManyKeys`; `Configuration::{tag_schema, tag_permits}`,
`ConfigurationWriter`, `TagPermits`, `NamespacePermit`; tasks-config's
`permits`, plus `edit` once the import is retired.

## Implementation notes

- **Kept, not deleted:** `ports::Configuration::{tag_schema, tag_permits}`,
  `ConfigurationWriter::write_tag_permits`, `TagPermits` and
  `NamespacePermit`. The reviewed `schema.test.rs` (once `store_schema.test.rs`) implements them in
  its fixture (to show no use case reads them), and the import reads the
  moved keys through them; they go with the import. `Configuration` gained
  `moved()` (the `tasks-config-key-moved` refusal every use case but the
  import asks first) and `ConfigurationWriter` `retire_moved()`.
- **`tags::collapse` stays:** tagma applies its §8 collapse inside
  `validate` but exposes no call answering it, so taskloom reads
  `tagma.arity` for the one purpose of sending a retag already collapsed.
- **A retag is judged per attempt**, against the task as that attempt
  reads it and sent at that head, as the collapse is. An add is judged
  against the rules read before anything is bound (a refused add creates
  nothing) and again once its store is seeded.
- **Seeding** is sent at the head read (empty), so two writers seed once.
  A read never writes: a store with no schema stream reads as seeded; the
  first write (and `schema export`) records the seed.
- **No first permit** (F6): the seed permits every exact target (`arity`,
  `type`, `enum`, `range`) it constrains; a `keys-per-item` namespace is
  not opened by it, so a new store's `schema check` reports
  `rule-on-unpermitted` for `area` and `repo` until the project permits
  one of each. The import permits each target the file declared rules for,
  as the file admitted a declared target without a permit.
- **`schema export`** prints tagma syntax in every format, as `loadout`
  prints yaml.
- **Lint** reports one violation a broken rule (tagma's code, the tags,
  the args); a closed row's `not-permitted` and `too-many-keys` are left
  alone, as permits and one-area were.
- **Two values for one scalar target in one write** keep the last (tagma
  §8), where `check_scalars` refused them.
