//! Tag permits, over fake ports and angzarr-ports' in-memory store: what a
//! write admits under them, how a permit and a revoke change the store's
//! rules, and what lint says of a stored tag they do not permit.
//!
//! Rewritten for the self-hosted schema (docs/plans/self-hosted-schema.md):
//! a permit is a `tagma.permit` tag on the store's schema stream, judged by
//! tagma, and no config file is read or written. The rulings cited are
//! tag-permits.plan.md's (session inept-sixth-stick) where they still hold;
//! what tagma now decides (the refusal's code, an existing tag warned of
//! rather than ignored, one area a task as `tagma.keys-per-item`) is said
//! where it changed. Every store is an allowlist from when it is made (the
//! owner's ruling F6, 2026-11-02), seeded with the permits of its rules'
//! targets and the core vocabulary: there is no open world, no first
//! permit, and a permit is a presence.

use angzarr_diagnostic::{code, Diagnostic, Report};

use crate::fakes::{ready, FakeClock, FakeDelay, FakeIdentity, FakeNames, FakeStores};
use crate::{
    AppError, Declaration, NewTask, PermitChange, PermitService, Permitted, SchemaService, Scope,
    Service, StatusChange, TagChange, TaskService, Written,
};

/// The configuration's refusal of keys moved into the store, as the fake
/// answers it: its code names no crate, since the use case passes it on.
const KEY_MOVED: &str = "a-key-moved";

/// A configuration no use case here may read the moved keys of, or write:
/// the permits are the store's.
#[derive(Default)]
struct FakeConfig {
    written: usize,
}

impl ports::Configuration for FakeConfig {
    fn tag_schema(&mut self) -> Result<Vec<String>, Report> {
        panic!("only the import reads the moved keys")
    }
}

impl ports::ConfigurationWriter for FakeConfig {
    fn write_tag_permits(&mut self, _: &ports::TagPermits) -> Result<(), Report> {
        self.written += 1;
        Ok(())
    }
}

type Permits = Service<FakeIdentity, FakeStores, FakeNames, FakeClock, FakeDelay, FakeConfig>;

const HARPS: [&str; 6] = [
    "swift-amber",
    "calm-otter",
    "brisk-oak",
    "lone-elm",
    "tall-oak",
    "dim-fern",
];

/// A service with `permits` permitted in order (each made on the store
/// the first one binds and makes): none, the seeded permits alone.
fn under(permits: &[&str]) -> Permits {
    let mut service = Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        FakeNames::of(&HARPS),
        FakeClock::default(),
        FakeDelay::default(),
    )
    .with_configuration(FakeConfig::default());
    for spelled in permits {
        permit(&mut service, spelled).unwrap();
    }
    service
}

/// The permits most tests run under: `area:cli` and `area:mcp`, `relates:`
/// open, and the flat `urgent`.
const USUAL: [&str; 4] = ["area:cli", "area:mcp", "relates:", "urgent"];

/// The factual flags (ruling 14): bare keys of `triage:`, scored by
/// nothing, seeded key by key (the core every project copies).
const FLAGS: [&str; 7] = [
    "triage:security",
    "triage:data-loss",
    "triage:crashes",
    "triage:no-workaround",
    "triage:regression",
    "triage:silent-no-op",
    "triage:flaky",
];

/// The exact targets the seeded schema constrains, sorted: the seed
/// permits each.
const SEEDED_TARGETS: [&str; 8] = [
    "triage:audited",
    "triage:blind-gate",
    "triage:blocks-release",
    "triage:effort",
    "triage:exposed",
    "triage:kind",
    "triage:level",
    "triage:verdict",
];

/// `harp_id` moved to `status` (Deferred given a trigger).
fn moved(service: &mut Permits, harp_id: &str, status: &str) {
    ready(service.set_status(StatusChange {
        harp_id: harp_id.to_string(),
        status: Some(status.to_string()),
        trigger: (status == "Deferred").then(|| "when it matters".to_string()),
        ..StatusChange::default()
    }))
    .unwrap();
}

/// Lint's violations, by harp id, each its English.
fn lint_findings(service: &mut Permits) -> Vec<(String, String)> {
    use crate::english::English;
    let report = ready(service.lint(Scope::default())).unwrap();
    report
        .violations
        .into_iter()
        .map(|v| (v.harp_id, v.finding.english()))
        .collect()
}

fn tagged(tags: &[&str]) -> NewTask {
    NewTask {
        text: "Ship it".to_string(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        ..NewTask::default()
    }
}

fn add(service: &mut Permits, tags: &[&str]) -> Result<Written, AppError> {
    ready(service.add(tagged(tags)))
}

fn permit(service: &mut Permits, permit: &str) -> Result<Permitted, AppError> {
    ready(service.permit(PermitChange {
        permit: permit.to_string(),
    }))
}

fn revoke(service: &mut Permits, permit: &str) -> Result<Permitted, AppError> {
    ready(service.revoke(PermitChange {
        permit: permit.to_string(),
    }))
}

fn export(service: &mut Permits) -> Vec<String> {
    ready(service.schema_export(None)).unwrap()
}

/// `spelled`, as tagma spells it canonically.
fn canonical(spelled: &str) -> String {
    tagma_core::Tag::parse(spelled).unwrap().to_string()
}

fn spelled(written: &Written) -> Vec<String> {
    written.task.tags.iter().map(ToString::to_string).collect()
}

/// A task written straight through the kernel, as a store from before
/// its permits holds it: its tags were never judged by them.
fn old_task(service: &Permits, harp: &str, tags: &[&str]) {
    let add = tasks::AddTask {
        harp_id: harp.to_string(),
        text: "Old".to_string(),
        tags: tags
            .iter()
            .map(|t| tagma_core::Tag::parse(t).unwrap().into())
            .collect(),
        status: None,
        trigger: None,
        origin_session: None,
        defers: "Deferred".to_string(),
        initial: "To Do".to_string(),
    };
    crate::fakes::run(service.stores.store.inner(), harp, add.into());
}

/// Whether `error` is tagma's `not-permitted` of `tag`.
fn not_permitted(error: &AppError, tag: &str) -> bool {
    let shown = error.to_string();
    error.code() == code!("tag-rules") && shown.contains("not-permitted") && shown.contains(tag)
}

// ------------------------------------------------------------ admission

// Ruling 2: a tag not permitted is refused, naming the tag, by tagma's
// `not-permitted` (was tasks-application-tag-not-permitted, which named
// the permit to run: tagma's finding names no command), and nothing is
// written.
#[test]
fn test_an_add_with_a_tag_not_permitted_is_refused_naming_it() {
    for tag in ["area:host-hardening", "lang:foo=bar", "wibble"] {
        let mut service = under(&USUAL);
        let error = add(&mut service, &["urgent", tag]).unwrap_err();
        assert!(not_permitted(&error, tag), "{tag}: {error}");
        assert!(matches!(error, AppError::TagRefused(_)));
        let listed = ready(service.list(crate::ListQuery::default())).unwrap();
        assert_eq!(listed.tasks().count(), 0, "nothing written");
    }
}

#[test]
fn test_a_tag_change_adding_a_tag_not_permitted_is_refused_and_changes_nothing() {
    let mut service = under(&USUAL);
    add(&mut service, &["area:cli"]).unwrap();
    let change = TagChange {
        harp_id: "swift-amber".to_string(),
        add: vec!["urgent".to_string(), "area:host-hardening".to_string()],
        ..TagChange::default()
    };
    let error = ready(service.tag(change)).unwrap_err();
    assert!(not_permitted(&error, "area:host-hardening"), "{error}");
    let shown = ready(service.show(crate::ShowQuery {
        harp_ids: vec!["swift-amber".to_string()],
        ..crate::ShowQuery::default()
    }))
    .unwrap();
    let tags: Vec<String> = shown.rows[0]
        .task
        .tags
        .iter()
        .map(|t| t.to_string())
        .collect();
    assert_eq!(tags, ["area:cli"], "urgent was not added either");
}

#[test]
fn test_what_the_permits_permit_is_admitted() {
    let mut service = under(&USUAL);
    let written = add(&mut service, &["area:cli", "relates:calm-otter", "urgent"]).unwrap();
    assert_eq!(
        spelled(&written),
        ["area:cli", "relates:calm-otter", "urgent"]
    );
}

// Ruling 6: a permit is of a target, never a value, so a permitted key
// carries any value the rules admit.
#[test]
fn test_a_permitted_key_may_carry_a_value() {
    let mut service = under(&USUAL);
    let written = add(&mut service, &["area:cli=x"]).unwrap();
    assert_eq!(spelled(&written), ["area:cli=x"]);
}

// Ruling 14, as the seed now has it: every target the store's rules
// constrain (the seeded triage:* among them) is permitted from when the
// store is made, as a KEY: its namespace is not opened by it.
// triage:was-release is declared nowhere, so it is refused.
#[test]
fn test_the_rules_constrained_targets_are_permitted_from_the_start() {
    let mut service = under(&[]);
    let written = add(
        &mut service,
        &["triage:kind=defect", "triage:level=2", "triage:exposed=cli"],
    )
    .unwrap();
    assert_eq!(written.task.tags.len(), 3);
    let error = add(&mut service, &["triage:was-release=1.0.0"]).unwrap_err();
    assert!(not_permitted(&error, "triage:was-release"), "{error}");
}

// Ruling 5: triage:blocks-release=<semver> is the one release form, and
// permits do not break it: admitted, still held to its declared type
// (tagma's type-mismatch, through taskloom's semver comparator), and any
// other spelling refused as not permitted.
#[test]
fn test_release_has_one_form_and_permits_do_not_break_it() {
    let mut service = under(&USUAL);
    let written = add(&mut service, &["triage:blocks-release=1.2.0"]).unwrap();
    assert_eq!(spelled(&written), ["triage:blocks-release=1.2.0"]);
    let error = add(&mut service, &["triage:blocks-release=next"]).unwrap_err();
    assert!(error.to_string().contains("type-mismatch"), "{error}");
    for other in ["release:1.2.0", "blocks-release", "release=1.2.0"] {
        let error = add(&mut service, &[other]).unwrap_err();
        assert!(not_permitted(&error, other), "{other}: {error}");
    }
}

// F6: there is no opt-in. A store is an allowlist from when it is made, so
// a new store refuses what its seeded permits do not admit.
#[test]
fn test_a_new_store_refuses_a_tag_its_seeded_permits_do_not_admit() {
    let mut service = under(&[]);
    for tag in ["area:host-hardening", "wibble"] {
        let error = add(&mut service, &[tag]).unwrap_err();
        assert!(not_permitted(&error, tag), "{tag}: {error}");
    }
}

// A tag held from before the permits never refuses a write: it is an
// existing finding, warned of (it was ignored before tagma judged the
// whole set), and kept beside one added; removed, it is no longer there.
#[test]
fn test_a_tag_held_from_before_the_permits_is_warned_of_never_refused() {
    let mut service = under(&USUAL);
    old_task(&service, "swift-amber", &["wibble", "area:host-hardening"]);
    let change = TagChange {
        harp_id: "swift-amber".to_string(),
        add: vec!["urgent".to_string()],
        remove: vec!["wibble".to_string()],
        ..TagChange::default()
    };
    let written = ready(service.tag(change)).unwrap();
    assert_eq!(spelled(&written), ["area:host-hardening", "urgent"]);
    let warned: Vec<String> = written
        .warnings()
        .iter()
        .map(|f| format!("{} {}", f.code.as_str(), f.tag))
        .collect();
    assert_eq!(warned, ["not-permitted area:host-hardening"]);
}

// Ruling 14: the factual flags stay bare keys of `triage:`, seeded key by
// key, and the targets the rules constrain (kind, level, blind-gate, ...)
// are permitted beside them, each still held to its rules; a key neither
// permits is refused.
#[test]
fn test_seeded_triage_flags_coexist_with_the_rules_targets() {
    let mut service = under(&[]);
    for flag in FLAGS {
        assert!(
            !permit(&mut service, flag).unwrap().changed,
            "{flag} seeded"
        );
    }
    let written = add(
        &mut service,
        &[
            "triage:security",
            "triage:silent-no-op",
            "triage:kind=defect",
            "triage:level=2",
            "triage:blind-gate=mutation",
        ],
    )
    .unwrap();
    assert_eq!(written.task.tags.len(), 5, "{:?}", spelled(&written));
    for tag in [
        "triage:fact=security",
        "triage:was-release=1.0.0",
        "triage:flake",
    ] {
        let error = add(&mut service, &[tag]).unwrap_err();
        assert!(not_permitted(&error, tag), "{tag}: {error}");
    }
    let error = add(&mut service, &["triage:kind=bug"]).unwrap_err();
    assert!(error.to_string().contains("not-in-enum"), "{error}");
}

// Ruling 14: triage:blind-gate is constrained by the seeded schema, so a
// new store admits it, more than one gate at once (it is repeatable), and
// refuses a gate not in its enum.
#[test]
fn test_blind_gate_is_permitted_by_the_seed_and_held_to_its_enum() {
    let mut service = under(&[]);
    let written = add(
        &mut service,
        &["triage:blind-gate=mutation", "triage:blind-gate=ci"],
    )
    .unwrap();
    assert_eq!(
        spelled(&written),
        ["triage:blind-gate=ci", "triage:blind-gate=mutation"]
    );
    let error = add(&mut service, &["triage:blind-gate=vibes"]).unwrap_err();
    assert!(error.to_string().contains("not-in-enum"), "{error}");
}

// Ruling 13, as F6 seeds it: the core vocabulary is copied into every
// store when it is made (seeded, so a project may revoke it), and a
// project's own (its areas, its sibling repos, a flat tag) it permits
// itself.
#[test]
fn test_the_core_vocabulary_is_seeded_and_a_projects_own_is_not() {
    let mut service = under(&[]);
    let core = [
        "human:decision",
        "relates:calm-otter",
        "touches:justfile",
        "sig:os.Mode",
        "queue:20261027",
        r#"landed:"tag-permits""#,
        "platform:linux",
        "triage:security",
    ];
    let written = add(&mut service, &core).unwrap();
    assert_eq!(written.task.tags.len(), core.len());
    for tag in [
        "area:cli",
        "repo:annals",
        "urgent",
        "human:whim",
        "platform:beos",
    ] {
        let error = add(&mut service, &[tag]).unwrap_err();
        assert!(not_permitted(&error, tag), "{tag}: {error}");
    }
}

// ------------------------------------------------------------ the list

// Ruling 9: what is permitted is readable, by a person and an agent alike
// (`taskloom permits`): the store's patterns, and every target its rules
// constrain, sorted, its own declarations among them.
#[test]
fn test_the_permits_are_listed_with_the_targets_the_rules_constrain() {
    let mut service = under(&USUAL);
    ready(service.declare(Declaration {
        add: vec![r#"tagma.enum:"area:x"=a"#.to_string()],
        remove: vec![],
    }))
    .unwrap();
    let listed = ready(service.permits()).unwrap();
    let mut targets = vec!["area:x".to_string()];
    targets.extend(SEEDED_TARGETS.iter().map(|t| t.to_string()));
    assert_eq!(listed.schema_targets, targets);
    for pattern in ["area:cli", "area:mcp", "relates:*", "urgent", "triage:kind"] {
        assert!(
            listed.patterns.contains(&pattern.to_string()),
            "{pattern} in {:?}",
            listed.patterns
        );
    }
    assert!(
        !listed.patterns.contains(&"*:*".to_string()),
        "an allowlist, never every tag"
    );
}

// F6, as the list says it: a new store's permits are the seed's.
#[test]
fn test_a_new_store_lists_its_seeded_permits() {
    let mut service = under(&[]);
    let listed = ready(service.permits()).unwrap();
    let mut seeded: Vec<String> = crate::SEEDED_SCHEMA
        .iter()
        .filter_map(|spelled| tagma_core::Tag::parse(spelled).ok())
        .filter(|tag| tag.namespace.as_deref() == Some("tagma.permit"))
        .map(|tag| tag.key)
        .collect();
    seeded.sort();
    assert_eq!(listed.patterns, seeded);
    assert_eq!(listed.schema_targets, SEEDED_TARGETS);
}

// ------------------------------------------------------------ permit

// Ruling 3, as the store has it: a permit is declared on the schema
// stream, as the tagma pattern it spells, and no config file is written.
#[test]
fn test_a_permit_grants_a_namespace_a_key_or_a_flat_tag() {
    for (spelled, pattern) in [
        ("area:", r#"tagma.permit:"area:*""#),
        ("area:cli", r#"tagma.permit:"area:cli""#),
        ("urgent", "tagma.permit:urgent"),
    ] {
        let mut service = under(&[]);
        let permitted = permit(&mut service, spelled).unwrap();
        assert_eq!(
            permitted,
            Permitted {
                permit: spelled.to_string(),
                changed: true
            }
        );
        assert!(
            export(&mut service).contains(&canonical(pattern)),
            "{spelled}"
        );
        assert_eq!(service.configuration.written, 0, "{spelled}");
    }
}

// F6: no permit is the first. A permit declares its own pattern and
// nothing else (no `"*:*"=false`, no target kept), and a tag no permit
// admits is still refused.
#[test]
fn test_a_permit_declares_its_pattern_alone() {
    let mut service = under(&[]);
    add(&mut service, &[]).unwrap();
    let before = export(&mut service);
    permit(&mut service, "area:cli").unwrap();
    let after = export(&mut service);
    let added: Vec<&String> = after.iter().filter(|t| !before.contains(t)).collect();
    assert_eq!(added, [&canonical(r#"tagma.permit:"area:cli""#)]);
    assert!(before.iter().all(|t| after.contains(t)));
    let error = add(&mut service, &["wibble"]).unwrap_err();
    assert!(not_permitted(&error, "wibble"), "{error}");
}

// A permit is spelled as the tag is, canonically: whitespace trimmed, as
// a tag given to a write is; a namespace's as the namespace and a colon,
// a key quoted as tagma quotes one, a flat tag bare.
#[test]
fn test_each_kind_of_permit_is_answered_in_its_canonical_spelling() {
    for (given, canonical) in [
        (" area: ", "area:"),
        ("  area:cli ", "area:cli"),
        ("urgent", "urgent"),
        (r#"sig:"a b""#, r#"sig:"a b""#),
    ] {
        let mut service = under(&[]);
        assert_eq!(
            permit(&mut service, given).unwrap().permit,
            canonical,
            "{given}"
        );
    }
}

// Idempotent: a permit already declared is answered unchanged, and the
// stream records nothing.
#[test]
fn test_a_permit_of_what_is_already_permitted_changes_nothing() {
    for spelled in USUAL {
        let mut service = under(&USUAL);
        let before = export(&mut service);
        let permitted = permit(&mut service, spelled).unwrap();
        assert!(!permitted.changed, "{spelled}");
        assert_eq!(export(&mut service), before, "{spelled}");
    }
}

#[test]
fn test_a_permit_not_spelled_as_one_is_refused_and_nothing_declared() {
    for spelled in [
        "area:cli=x",
        "urgent=1",
        "tagma.enum:",
        "tagma:x",
        "taskloom:x",
        "and",
        "a b",
        ":",
        "a:b:",
        "area:cli:",
    ] {
        let mut service = under(&USUAL);
        let before = export(&mut service);
        let error = permit(&mut service, spelled).unwrap_err();
        assert_eq!(error.code(), code!("permit-malformed"), "{spelled}");
        assert_eq!(export(&mut service), before, "{spelled}");
    }
    let mut service = under(&USUAL);
    let error = permit(&mut service, "  ").unwrap_err();
    assert_eq!(error, AppError::Blank { field: "permit" });
    let error = revoke(&mut service, "").unwrap_err();
    assert_eq!(error, AppError::Blank { field: "permit" });
}

// ------------------------------------------------------------ revoke

#[test]
fn test_a_revoke_no_task_needs_removes_the_permit() {
    for (spelled, pattern) in [
        ("area:mcp", r#"tagma.permit:"area:mcp""#),
        ("relates:", r#"tagma.permit:"relates:*""#),
        ("urgent", "tagma.permit:urgent"),
    ] {
        let mut service = under(&USUAL);
        let revoked = revoke(&mut service, spelled).unwrap();
        assert!(revoked.changed, "{spelled}");
        assert!(
            !export(&mut service).contains(&canonical(pattern)),
            "{spelled}"
        );
    }
}

// Ruling 4: revoking what stored tasks still carry is refused, listing
// them by harp id (sorted), and nothing is declared. A key's tasks are
// those carrying it with any value; a namespace's, any key of it.
#[test]
fn test_a_revoke_of_what_tasks_still_carry_is_refused_listing_them() {
    let mut service = under(&["area:", "area:cli", "urgent"]);
    add(&mut service, &["area:cli"]).unwrap();
    add(&mut service, &["area:cli=x", "urgent"]).unwrap();
    add(&mut service, &["area:mcp"]).unwrap();
    let before = export(&mut service);
    for (spelled, carriers) in [
        ("area:cli", &["calm-otter", "swift-amber"][..]),
        ("area:", &["brisk-oak", "calm-otter", "swift-amber"]),
        ("urgent", &["calm-otter"]),
    ] {
        let error = revoke(&mut service, spelled).unwrap_err();
        let expected: Vec<String> = carriers.iter().map(|c| c.to_string()).collect();
        assert_eq!(
            error,
            AppError::PermitInUse {
                permit: spelled.to_string(),
                harp_ids: expected
            }
        );
        assert_eq!(export(&mut service), before, "{spelled}");
    }
}

// Ruling 6 (revoke refused while in use): a task carries a tag whatever
// its status, so a finished one still carrying it blocks the revoke too.
#[test]
fn test_a_finished_task_still_carrying_a_permit_blocks_its_revoke() {
    let mut service = under(&USUAL);
    add(&mut service, &["area:mcp"]).unwrap();
    moved(&mut service, "swift-amber", "Done");
    let error = revoke(&mut service, "area:mcp").unwrap_err();
    assert!(matches!(error, AppError::PermitInUse { .. }), "{error:?}");
}

// A revoke of what is not among the permits is refused: a pattern never
// declared (a key under a namespace permitted whole is no pattern of its
// own: revoke the namespace's), and anything a new store's seed does not
// permit.
#[test]
fn test_a_revoke_of_what_is_not_among_the_permits_is_refused() {
    for spelled in ["lang:", "area:config", "relates:calm-otter", "wibble"] {
        let mut service = under(&USUAL);
        let error = revoke(&mut service, spelled).unwrap_err();
        assert_eq!(
            error,
            AppError::PermitNotDeclared {
                permit: spelled.to_string()
            }
        );
    }
    let mut service = under(&[]);
    let error = revoke(&mut service, "area:cli").unwrap_err();
    assert!(
        matches!(error, AppError::PermitNotDeclared { .. }),
        "{error:?}"
    );
}

// Where there is no project yet, nothing is declared, so there is nothing
// to revoke.
#[test]
fn test_a_revoke_where_there_is_no_project_yet_is_not_declared() {
    let mut service = Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        FakeNames::of(&HARPS),
        FakeClock::default(),
        FakeDelay::default(),
    )
    .with_configuration(FakeConfig::default());
    let error = revoke(&mut service, "area:cli").unwrap_err();
    assert!(
        matches!(error, AppError::PermitNotDeclared { .. }),
        "{error:?}"
    );
    assert!(service.identity.bound.is_none(), "no project bound");
}

// A flat tag's revoke is blocked by a task carrying that flat tag only:
// not by a namespaced tag of the same key, nor by another flat tag.
#[test]
fn test_a_flat_tags_revoke_is_blocked_only_by_that_flat_tag() {
    let mut service = under(&["area:urgent", "urgent", "blocked"]);
    add(&mut service, &["area:urgent"]).unwrap();
    add(&mut service, &["blocked"]).unwrap();
    assert!(revoke(&mut service, "urgent").unwrap().changed);
    let error = revoke(&mut service, "blocked").unwrap_err();
    assert_eq!(
        error,
        AppError::PermitInUse {
            permit: "blocked".to_string(),
            harp_ids: vec!["calm-otter".to_string()],
        }
    );
}

// ------------------------------------------------------------ lint

// Lint sees what admission let stand: a stored tag no permit admits
// (written before the permits). Ruling 15: only on the rows still in
// play; closed rows are the next test's. A key the permits refuse still
// counts toward the one area a task takes (tagma counts every tag toward
// its namespace's keys, §15), so a task carrying one refused and one
// permitted area gets both findings.
#[test]
fn test_lint_reports_a_stored_tag_the_permits_do_not_permit() {
    let mut service = under(&USUAL);
    old_task(
        &service,
        "swift-amber",
        &["area:host-hardening", "area:cli", "urgent"],
    );
    old_task(&service, "calm-otter", &["wibble"]);
    moved(&mut service, "calm-otter", "Deferred");
    assert_eq!(
        lint_findings(&mut service),
        [
            ("calm-otter".to_string(), "wibble: not-permitted (-)".to_string()),
            (
                "swift-amber".to_string(),
                "area:host-hardening: not-permitted (-)".to_string()
            ),
            (
                "swift-amber".to_string(),
                "area:cli, area:host-hardening: too-many-keys (namespace=area, limit=1, keys=cli, keys=host-hardening)".to_string()
            ),
        ]
    );
}

// Ruling 15: a closed row's tags are its history, so lint does not report
// what it carries no permit admits, Done or Archived. The open row beside
// them shows the lint did look.
#[test]
fn test_lint_leaves_a_closed_rows_permits_alone() {
    let mut service = under(&USUAL);
    old_task(&service, "swift-amber", &["area:host-hardening"]);
    old_task(&service, "calm-otter", &["wibble"]);
    old_task(&service, "brisk-oak", &["wibble"]);
    moved(&mut service, "swift-amber", "Done");
    moved(&mut service, "calm-otter", "Archived");
    let found = lint_findings(&mut service);
    let subjects: Vec<&str> = found.iter().map(|(harp, _)| harp.as_str()).collect();
    assert_eq!(subjects, ["brisk-oak"], "{found:?}");
}

// Review (2026-10-27): one area and one repo a task holds in every store;
// it is the seeded `tagma.keys-per-item`, so a write is refused a second
// one and lint reports a task that carries two from before the rule.
#[test]
fn test_one_area_and_one_repo_a_task_hold_in_every_store() {
    let mut service = under(&["area:cli", "area:mcp", "repo:a", "repo:b"]);
    let error = add(&mut service, &["area:cli", "area:mcp"]).unwrap_err();
    assert!(error.to_string().contains("too-many-keys"), "{error}");
    ready(service.declare(Declaration {
        add: vec![],
        remove: vec!["tagma.keys-per-item:repo=1".to_string()],
    }))
    .unwrap();
    add(&mut service, &["repo:a", "repo:b"]).unwrap();
    add(&mut service, &["repo:a", "repo:b"]).unwrap();
    moved(&mut service, "calm-otter", "Done");
    ready(service.declare(Declaration {
        add: vec!["tagma.keys-per-item:repo=1".to_string()],
        remove: vec![],
    }))
    .unwrap();
    assert_eq!(
        lint_findings(&mut service),
        [(
            "swift-amber".to_string(),
            "repo:a, repo:b: too-many-keys (namespace=repo, limit=1, keys=a, keys=b)".to_string()
        )]
    );
}

// ------------------------------------------------------- import-config

/// The work root's config file as the import reads it: its moved keys,
/// and whether they were taken out of it.
#[derive(Default)]
struct OldConfig {
    schema: Vec<String>,
    permits: Option<ports::TagPermits>,
    retired: usize,
}

impl ports::Configuration for OldConfig {
    fn moved(&mut self) -> Result<(), Report> {
        match self.schema.is_empty() && self.permits.is_none() {
            true => Ok(()),
            false => Err(Report::new(KEY_MOVED)),
        }
    }

    fn tag_schema(&mut self) -> Result<Vec<String>, Report> {
        Ok(self.schema.clone())
    }

    fn tag_permits(&mut self) -> Result<Option<ports::TagPermits>, Report> {
        Ok(self.permits.clone())
    }
}

impl ports::ConfigurationWriter for OldConfig {
    fn write_tag_permits(&mut self, _: &ports::TagPermits) -> Result<(), Report> {
        panic!("the import writes no permits to a file")
    }

    fn retire_moved(&mut self) -> Result<bool, Report> {
        let held = !(self.schema.is_empty() && self.permits.is_none());
        self.schema.clear();
        self.permits = None;
        self.retired += 1;
        Ok(held)
    }
}

type Importing = Service<FakeIdentity, FakeStores, FakeNames, FakeClock, FakeDelay, OldConfig>;

fn importing(config: OldConfig) -> Importing {
    Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        FakeNames::of(&HARPS),
        FakeClock::default(),
        FakeDelay::default(),
    )
    .with_configuration(config)
}

// The old declarations become tagma's facets: an enum of `a,b` one tag a
// value, a range of `lo,hi` as `lo..hi`, the formulas taskloom.*, any
// other as written (for `schema check` to judge); then the keys are taken
// out of the file, and every use case reads again.
#[test]
fn test_the_import_moves_the_old_declarations_into_tagmas_facets() {
    let mut service = importing(OldConfig {
        schema: [
            r#"tagma.enum:"area:x"="a, b""#,
            r#"tagma.range:"triage:level"="1,9""#,
            r#"tagma.priority_fn:"triage:kind"="1""#,
            r#"tagma.decay_fn:"triage:kind"="2""#,
            r#"tagma.arity:"area:x"=scalar"#,
        ]
        .map(String::from)
        .into(),
        ..OldConfig::default()
    });
    let error = ready(service.list(crate::ListQuery::default())).unwrap_err();
    assert_eq!(error.code(), KEY_MOVED, "refused before the import");
    let imported = ready(service.import_config()).unwrap();
    assert!(imported.changed);
    for tag in [
        r#"tagma.enum:"area:x"=a"#,
        r#"tagma.enum:"area:x"=b"#,
        r#"tagma.range:"triage:level"=1..9"#,
        r#"taskloom.priority_fn:"triage:kind"="1""#,
        r#"taskloom.decay_fn:"triage:kind"="2""#,
        r#"tagma.arity:"area:x"=scalar"#,
    ] {
        assert!(
            imported.tags.contains(&canonical(tag)),
            "{tag}: {:?}",
            imported.tags
        );
    }
    let seeded: Vec<String> = crate::SEEDED_SCHEMA.iter().map(|s| canonical(s)).collect();
    let permits: Vec<&String> = imported
        .tags
        .iter()
        .filter(|t| t.starts_with("tagma.permit") && !seeded.contains(t))
        .collect();
    assert_eq!(
        permits,
        [&canonical(r#"tagma.permit:"area:x""#)],
        "beside the seed's, the one target the file declared rules for"
    );
    assert_eq!(service.configuration.retired, 1);
    assert!(ready(service.list(crate::ListQuery::default())).is_ok());
    let again = ready(service.import_config()).unwrap();
    assert!(!again.changed, "nothing left to import");
    assert_eq!(again.tags, imported.tags);
}

// The old permits become tagma.permit patterns, each a presence (F6: no
// world to close), and each target the file declared rules for is
// permitted, as the file admitted a declared target without one.
#[test]
fn test_the_import_moves_the_old_permits_and_permits_the_declared_targets() {
    let mut permits = ports::TagPermits::default();
    permits.namespaces.insert(
        "area".to_string(),
        ports::NamespacePermit::Closed(["cli".to_string()].into()),
    );
    permits
        .namespaces
        .insert("relates".to_string(), ports::NamespacePermit::Open);
    permits.flat.insert("urgent".to_string());
    let mut service = importing(OldConfig {
        schema: vec![r#"tagma.enum:"lang:x"="en""#.to_string()],
        permits: Some(permits),
        ..OldConfig::default()
    });
    let imported = ready(service.import_config()).unwrap();
    assert!(
        !imported
            .tags
            .iter()
            .any(|t| t.contains("=true") || t.contains("=false")),
        "no permit carries a value: {:?}",
        imported.tags
    );
    for tag in [
        r#"tagma.permit:"area:cli""#,
        r#"tagma.permit:"relates:*""#,
        "tagma.permit:urgent",
        r#"tagma.permit:"lang:x""#,
        r#"tagma.permit:"triage:kind""#,
    ] {
        assert!(
            imported.tags.contains(&canonical(tag)),
            "{tag}: {:?}",
            imported.tags
        );
    }
    assert!(add_to(&mut service, &["lang:x=en", "area:cli", "urgent"]).is_ok());
    let error = add_to(&mut service, &["area:mcp"]).unwrap_err();
    assert!(not_permitted(&error, "area:mcp"), "{error}");
}

// A declaration that is no tag at all is refused, and nothing is declared
// or taken out of the file.
#[test]
fn test_the_import_refuses_a_declaration_that_is_no_tag() {
    let mut service = importing(OldConfig {
        schema: vec!["a b".to_string()],
        ..OldConfig::default()
    });
    let error = ready(service.import_config()).unwrap_err();
    assert_eq!(error.code(), code!("tag-unparseable"));
    assert_eq!(service.configuration.retired, 0);
}

fn add_to(service: &mut Importing, tags: &[&str]) -> Result<Written, AppError> {
    ready(service.add(tagged(tags)))
}
