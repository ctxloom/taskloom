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

**Seed.** The default is the GENERAL tag set (owner-ruled 2026-11-05),
small and project-neutral, written as one `SchemaRetagged` event when a
store is made and on first open of a store with no schema stream
(`GENERAL_SCHEMA`, also exported as `SEEDED_SCHEMA`; every store is an
allowlist from when it is made, the owner's ruling F6, 2026-11-02):

- constrained targets: `triage:level` (scalar 1..5), `kind` (scalar:
  defect, capability, chore), `effort` (scalar 0..5), `blocks-release`
  (scalar semver) and `verdict` (scalar: holds, obsolete, partial,
  phantom, unclear), each permitted; `tagma.keys-per-item:area=1`;
- `area:` OPEN (`tagma.permit:"area:*"`), and `relates:`, `touches:` and
  `queue:` open;
- CLOSED: `human:`'s six keys and the factual flags `triage:{security,
  data-loss, crashes, no-workaround, regression, silent-no-op, flaky}`;
- the parking flags `icebox` and `someday-maybe`, flat tags (the owner's
  request of 2026-11-12, kept in the default by the ruling of 2026-11-24),
  and `review:` open with its `review:illuminated` a scalar `YYYYMMDDHHMM`
  in UTC held to 202501010000..209912312359 (the illuminated code
  review's start);
- `taskloom.priority_fn` (level, age, effort: `age_days`, bounded, not a
  decay) and NO `decay_fn`.

Out of the default: `triage:exposed`, `blind-gate` and `audited`,
`landed:`, `repo:`, `platform:`, `sig:` and the ctxloom decay formula. A
fresh store therefore accepts `--tag area:cli` as its first write and
checks clean; `repo:` stays unpermitted.

The ctxloom project's set is the named TEMPLATE `ctxloom`
(`CTXLOOM_SCHEMA`): exactly the 67 tags every store was seeded with before
the general default (the seed as it stood on 2026-11-24: one tag per enum value, `lo..hi` ranges,
`tagma.keys-per-item:area=1`/`repo=1`, `taskloom.priority_fn`/`decay_fn`,
and the permits of each target it constrains and the core vocabulary:
the factual flags, `human:`'s keys, `relates:`, `touches:`, `sig:`,
`queue:`, `landed:` open, `platform:`'s keys, the parking flags and
`review:`). Its `area:` and
`repo:` keys the project permits itself, so a store holding it reports
`rule-on-unpermitted` for `area` and `repo` until it does.

A store that has a schema stream is read as it holds it
(`StoreRules::of`), so no existing store changes with the default. The
one place the seed is consulted for a store with history is the pre-F6
migration: a store holding no permit as a presence (one from before F6,
open or closed by `"*:*"=false`) has the PRE-F6 permits written on its
first open (pinned to `CTXLOOM_SCHEMA`'s, not the default of now), its
`=true` permits kept as presences, its other valued ones dropped. (A
modern store whose every permit was removed is read the same way: with
those permits.)

**CLI.** `manage schema show` (same JSON shape as now), `manage schema
export`, `manage schema check` (exits 49 on findings), `manage schema
declare --add/--remove` (a preview unless `--yes`), `manage migrate schema`.
`manage schema init [--template general|ctxloom] [--area <key>]... [--yes]`
declares a template on the store (a person's command; a preview unless
`--yes`): it adds only the tags the store lacks, so a second run changes
nothing, and `--area` (repeatable) CLOSES `area:` to exactly those keys,
replacing the template's open `area:*` by one `area:<key>` permit each.
It is refused (`tasks-application-schema-init-conflict`, exit 1) when it
would have to take something away (`--area` over an open `area:*`) or give
a target a second rule (another range, arity, type, `keys-per-item` or
formula than the store holds); and it never opens a closed `area:`.
On a store with no schema history it declares the template itself, not
the default first. Persisted format: the SAME `SchemaRetagged` event, the
whole tag set after, as a declare records; there is no new event.
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
  `type`, `enum`, `range`) it constrains. The general default also opens
  `area:`, so its `schema check` is clean; the `ctxloom` template does
  not open a `keys-per-item` namespace, so a store holding it reports
  `rule-on-unpermitted` for `area` and `repo` until the project permits
  one of each.
- **Closing `area:`** is `schema init --area`, once, before the first
  write; after it the person uses `permits remove area:` (refused while
  tasks carry an area) and `permits add area:<key>`. A key permit beside
  an open `area:*` is `shadowed-permit` in `schema check`.
- **The init interview**: the loadout's `init:` section carries the
  question `schema-areas` (an id and a prompt: all ctxloom's `InitQuestion`
  models) and a `setup_guidance` mapping its answer to `manage schema init
  --area`. The import permits each target the file declared rules for,
  as the file admitted a declared target without a permit.
- **`schema export`** prints tagma syntax in every format, as `loadout`
  prints yaml.
- **Lint** reports one violation a broken rule (tagma's code, the tags,
  the args); a closed row's `not-permitted` and `too-many-keys` are left
  alone, as permits and one-area were.
- **Two values for one scalar target in one write** keep the last (tagma
  §8), where `check_scalars` refused them.
