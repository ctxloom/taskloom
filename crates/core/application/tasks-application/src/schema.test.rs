//! RED (red/self-hosted-schema, for the owner's review): the store's own
//! tag rules, over fake ports and angzarr-ports' in-memory store
//! (docs/plans/self-hosted-schema.md).
//!
//! The boundary these pin: the rules are tagma config tags on the store's
//! schema stream, and tagma alone interprets them (`Rules::validate`,
//! `Rules::check`). A rule taskloom has no code for (keys-per-item, range
//! intersection, introduced/existing) works through taskloom because
//! tagma's does; no file layer is read for them.

use coded_diagnostic::{code, Diagnostic, Report};
use exit_code::ExitClass;
use ports::TagPermits;

use crate::fakes::{ready, FakeClock, FakeDelay, FakeIdentity, FakeNames, FakeStores};
use crate::rules::StoreRulesExt;
use crate::{
    AppError, Declaration, ListQuery, NewTask, Origin, PermitChange, PermitService, SchemaInit,
    SchemaService, Scope, Service, TagChange, TaskService, Template, Written, CTXLOOM_SCHEMA,
    GENERAL_SCHEMA, SEEDED_SCHEMA,
};

/// Configuration that would narrow every rule, were it read: it records
/// every time it is asked, and every permit written to it.
#[derive(Default)]
struct Files {
    asked: usize,
    written: usize,
}

impl ports::Configuration for Files {
    fn tag_schema(&mut self) -> Result<Vec<String>, Report> {
        self.asked += 1;
        Ok(vec![
            r#"tagma.range:"triage:level"="1,2""#.to_string(),
            r#"tagma.enum:"area:x"="a,b""#.to_string(),
        ])
    }

    fn tag_permits(&mut self) -> Result<Option<TagPermits>, Report> {
        self.asked += 1;
        Ok(Some(TagPermits::default()))
    }
}

impl ports::ConfigurationWriter for Files {
    fn write_tag_permits(&mut self, _: &TagPermits) -> Result<(), Report> {
        self.written += 1;
        Ok(())
    }
}

type Store = Service<FakeIdentity, FakeStores, FakeNames, FakeClock, FakeDelay, Files>;

const HARPS: [&str; 4] = ["swift-amber", "calm-otter", "brisk-oak", "lone-elm"];

/// A service over `identity` and `stores`.
fn over(identity: FakeIdentity, stores: FakeStores) -> Store {
    Service::new(
        identity,
        stores,
        FakeNames::of(&HARPS),
        FakeClock::default(),
        FakeDelay::default(),
    )
    .with_configuration(Files::default())
}

/// A service whose store permits every tag, so a rule other than the
/// permits is judged alone.
fn service() -> Store {
    over(FakeIdentity::bound(), FakeStores::permitting_every_tag())
}

/// A service whose store is made by its first write, as every store is.
fn fresh() -> Store {
    over(FakeIdentity::default(), FakeStores::default())
}

/// The seed as it was before every store was an allowlist (F6), the
/// ctxloom template's rules alone, no permit.
fn seed_without_permits() -> Vec<&'static str> {
    CTXLOOM_SCHEMA
        .iter()
        .copied()
        .filter(|spelled| !spelled.starts_with("tagma.permit:"))
        .collect()
}

/// What `service`'s store's schema stream holds, as it holds it: no use
/// case reads it unread.
fn held(service: &Store) -> Vec<String> {
    let registry = crate::registry();
    let (schema, _) = ready(
        angzarr_embedded::Kernel::new(&registry).load::<tasks::schema::SchemaAggregate, _>(
            service.stores.store.inner(),
            &crate::schema::schema_stream(),
        ),
    )
    .unwrap();
    crate::rules::StoreRules::held(schema.state().state()).export()
}

/// `spelled`, each canonically, sorted.
fn sorted(spelled: &[&str]) -> Vec<String> {
    let mut all: Vec<String> = spelled.iter().map(|s| canonical(s)).collect();
    all.sort();
    all
}

fn add(service: &mut Store, tags: &[&str]) -> Result<Written, AppError> {
    ready(service.add(NewTask {
        text: "Ship it".to_string(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        ..NewTask::default()
    }))
}

fn tag(
    service: &mut Store,
    harp: &str,
    add: &[&str],
    remove: &[&str],
) -> Result<Written, AppError> {
    let strings = |s: &[&str]| s.iter().map(|t| t.to_string()).collect();
    ready(service.tag(TagChange {
        harp_id: harp.to_string(),
        add: strings(add),
        remove: strings(remove),
        ..TagChange::default()
    }))
}

fn declare(service: &mut Store, add: &[&str], remove: &[&str]) {
    let strings = |s: &[&str]| s.iter().map(|t| t.to_string()).collect();
    ready(service.declare(Declaration {
        add: strings(add),
        remove: strings(remove),
        ..Default::default()
    }))
    .unwrap();
}

fn export(service: &mut Store) -> Vec<String> {
    ready(service.schema_export(None)).unwrap()
}

/// `spelled`, as tagma spells it canonically.
fn canonical(spelled: &str) -> String {
    tagma_core::Tag::parse(spelled).unwrap().to_string()
}

/// The refusal's code and its display (the key, then its args).
fn refusal(error: &AppError) -> (String, String) {
    (error.code().to_string(), error.to_string())
}

/// An old store: a task written straight through the kernel, as a
/// taskloom from before the schema stream wrote it, with no schema stream.
fn old_task(service: &Store, harp: &str, tags: &[&str]) {
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

// ------------------------------------------------------- seeding

// A dry run of a declaration answers the tags after it and whether they
// would change, and declares nothing; a bad declaration is refused as ever.
#[test]
fn test_a_dry_run_declaration_answers_the_tags_after_and_declares_nothing() {
    let mut service = service();
    let before = export(&mut service);
    let rule = r#"tagma.permit:"zzdry:key""#;
    let would = ready(service.declare(Declaration {
        add: vec![rule.to_string()],
        dry_run: true,
        ..Default::default()
    }))
    .unwrap();
    assert!(would.changed);
    assert!(would.tags.contains(&canonical(rule)));
    assert_eq!(export(&mut service), before, "nothing was declared");
    let none = ready(service.declare(Declaration {
        dry_run: true,
        ..Default::default()
    }));
    assert!(none.is_err(), "a declaration of nothing is refused");
}

#[test]
fn test_the_seeded_schema_is_tagma_syntax_in_its_canonical_spelling() {
    for spelled in SEEDED_SCHEMA {
        assert_eq!(canonical(spelled), spelled);
    }
}

#[test]
fn test_a_new_store_is_seeded_with_the_built_in_rules_when_it_is_made() {
    let mut service = fresh();
    add(&mut service, &[]).unwrap();
    let mut seeded: Vec<String> = SEEDED_SCHEMA.iter().map(|s| s.to_string()).collect();
    seeded.sort();
    assert_eq!(export(&mut service), seeded);
}

#[test]
fn test_a_store_made_before_the_schema_stream_is_seeded_on_first_open() {
    let mut service = fresh();
    // The store made, and an old task in it, with no schema stream.
    add(&mut service, &[]).unwrap();
    old_task(&service, "calm-otter", &["area:cli"]);
    let mut migrated = Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        FakeNames::of(&HARPS),
        FakeClock::default(),
        FakeDelay::default(),
    )
    .with_configuration(Files::default());
    old_task(&migrated, "brisk-oak", &["area:cli"]);
    migrated.stores.absent = false;
    let exported = export(&mut migrated);
    assert!(exported.contains(&canonical("tagma.keys-per-item:area=1")));
    assert_eq!(exported.len(), SEEDED_SCHEMA.len());
    // Seeded once: a declare after it is not undone by a second open.
    declare(&mut migrated, &[], &["tagma.keys-per-item:area=1"]);
    assert_eq!(export(&mut migrated).len(), SEEDED_SCHEMA.len() - 1);
}

// F6: every store is an allowlist from when it is made, so the seed
// permits every target it constrains (or a seeded rule could never be
// met): by the target's own pattern, or by its namespace's when the seed
// opens that namespace whole (`review:*`, for `review:illuminated`, the
// owner's request of 2026-11-12), and tagma's check agrees (the one-key
// limits on `area` and `repo` are on targets a project permits itself).
#[test]
fn test_every_target_the_seed_constrains_is_permitted_by_it() {
    let seed = crate::rules::StoreRules::default();
    let permits = seed.permits();
    let targets = seed.targets();
    for target in &targets {
        let namespace = target
            .split_once(':')
            .map(|(namespace, _)| format!("{namespace}:*"));
        assert!(
            permits.contains(target) || namespace.is_some_and(|open| permits.contains(&open)),
            "{target} in {permits:?}"
        );
    }
    let unpermitted: Vec<String> = seed
        .check()
        .into_iter()
        .filter(|finding| finding.code == tagma_core::Code::RuleOnUnpermitted)
        .filter(|finding| targets.contains(&finding.tag.key))
        .map(|finding| finding.tag.to_string())
        .collect();
    assert!(unpermitted.is_empty(), "{unpermitted:?}");
}

// A store closed before permits were presences (F6): its `=true` permits
// are kept as presences, its `"*:*"=false` dropped (every store is closed
// now), and the seed's permits declared beside them, on its first open,
// by a read as by a write; a read alone writes nothing.
#[test]
fn test_a_store_closed_before_permits_were_presences_is_seeded_as_an_allowlist() {
    let mut service = over(FakeIdentity::bound(), FakeStores::default());
    let mut old = seed_without_permits();
    old.extend([
        r#"tagma.permit:"*:*"=false"#,
        r#"tagma.permit:"area:cli"=true"#,
        r#"tagma.permit:"lang:*"=true"#,
    ]);
    crate::fakes::declared(service.stores.store.inner(), &old);
    old_task(&service, "calm-otter", &["area:cli", "wibble"]);
    let before = held(&service);
    let listed = ready(service.list(ListQuery::default())).unwrap();
    assert_eq!(listed.tasks().count(), 1);
    assert_eq!(held(&service), before, "a read writes nothing");
    // Pinned to the permits every store was seeded with then (the ctxloom
    // template's), not the default of now.
    let mut expected: Vec<&str> = CTXLOOM_SCHEMA.to_vec();
    expected.extend([r#"tagma.permit:"area:cli""#, r#"tagma.permit:"lang:*""#]);
    assert_eq!(export(&mut service), sorted(&expected));
    assert_eq!(held(&service), sorted(&expected), "seeded on its open");
    assert!(
        !held(&service).contains(&canonical(r#"tagma.permit:"area:*""#)),
        "the general default's permits are not added to an old store"
    );
    assert!(add(&mut service, &["area:cli", "lang:en"]).is_ok());
    let error = add(&mut service, &["wibble"]).unwrap_err();
    assert!(error.to_string().contains("not-permitted"), "{error}");
    let kept = tag(&mut service, "calm-otter", &["lang:fr"], &[]).unwrap();
    let warned: Vec<String> = kept.warnings().iter().map(|f| f.tag.to_string()).collect();
    assert_eq!(warned, ["wibble"], "an old tag is warned of, never refused");
}

// A store open before every store was an allowlist (F6: one declaring no
// permit) is seeded with the seed's permits, as a store made now is.
#[test]
fn test_a_store_open_before_every_store_was_an_allowlist_is_seeded_with_its_permits() {
    let mut service = over(FakeIdentity::bound(), FakeStores::default());
    crate::fakes::declared(service.stores.store.inner(), &seed_without_permits());
    old_task(&service, "calm-otter", &["wibble"]);
    assert!(add(&mut service, &["triage:kind=defect", "relates:calm-otter"]).is_ok());
    assert_eq!(
        held(&service),
        sorted(&CTXLOOM_SCHEMA),
        "seeded on its open"
    );
    assert!(
        !held(&service).contains(&canonical(r#"tagma.permit:"area:*""#)),
        "pinned to the old permits, not the default of now"
    );
    let error = add(&mut service, &["wibble"]).unwrap_err();
    assert!(error.to_string().contains("not-permitted"), "{error}");
}

// Permits once presences are the store's own: a seeded one revoked is not
// seeded again, and a declared rule is kept.
#[test]
fn test_a_store_holding_its_permits_as_presences_is_read_as_it_holds_them() {
    let mut service = fresh();
    add(&mut service, &[]).unwrap();
    ready(service.revoke(PermitChange {
        permit: "queue:".to_string(),
        ..Default::default()
    }))
    .unwrap();
    let revoked = export(&mut service);
    assert!(!revoked.contains(&canonical(r#"tagma.permit:"queue:*""#)));
    assert_eq!(held(&service), revoked);
    assert_eq!(export(&mut service), revoked, "not seeded again");
}

// A store left with no permit (each one revoked or removed) is read with
// the permits stores were seeded with before F6 (the ctxloom template's:
// a store holding no presence is read as an old one), as a store with no
// schema stream is read with the seed: there is no store permitting
// nothing.
#[test]
fn test_a_store_left_with_no_permit_is_read_with_the_seeds_permits() {
    let mut service = fresh();
    add(&mut service, &[]).unwrap();
    let permits: Vec<&str> = SEEDED_SCHEMA
        .iter()
        .copied()
        .filter(|spelled| spelled.starts_with("tagma.permit:"))
        .collect();
    declare(&mut service, &[], &permits);
    let mut expected: Vec<&str> = SEEDED_SCHEMA
        .iter()
        .copied()
        .filter(|spelled| !spelled.starts_with("tagma.permit:"))
        .collect();
    expected.extend(
        CTXLOOM_SCHEMA
            .iter()
            .filter(|spelled| spelled.starts_with("tagma.permit:")),
    );
    assert_eq!(export(&mut service), sorted(&expected));
}

#[test]
fn test_the_schema_stream_is_never_listed_as_a_task() {
    let mut service = service();
    add(&mut service, &[]).unwrap();
    declare(&mut service, &["tagma.keys-per-item:lang=1"], &[]);
    let listing = ready(service.list(ListQuery::default())).unwrap();
    let harps: Vec<String> = listing.tasks().map(|t| t.harp_id.clone()).collect();
    assert_eq!(harps, ["swift-amber"]);
}

// ------------------------------------------------- the write path

#[test]
fn test_a_write_introducing_a_tagma_finding_is_refused_naming_its_code() {
    let mut service = service();
    add(&mut service, &[]).unwrap();
    declare(&mut service, &[r#"tagma.enum:"area:x"=a"#], &[]);
    let error = add(&mut service, &["area:x=b"]).unwrap_err();
    let (code, shown) = refusal(&error);
    assert_eq!(code, crate::fakes::tag_code("tag-rules"));
    assert!(shown.contains("not-in-enum"), "{shown}");
    assert!(shown.contains("area:x=b"), "{shown}");
    let error = tag(&mut service, "swift-amber", &["area:x=b"], &[]).unwrap_err();
    assert_eq!(error.code(), crate::fakes::tag_code("tag-rules"));
    assert!(add(&mut service, &["area:x=a"]).is_ok());
}

// keys-per-item and range intersection have no taskloom code at all: they
// hold because tagma's validate holds them.
#[test]
fn test_a_rule_taskloom_has_no_code_for_is_enforced_through_tagma() {
    let mut service = service();
    add(&mut service, &[]).unwrap();
    declare(
        &mut service,
        &[
            "tagma.keys-per-item:lang=1",
            r#"tagma.range:"x:n"=1..10"#,
            r#"tagma.range:"x:n"=5..20"#,
        ],
        &[],
    );
    let error = add(&mut service, &["lang:en", "lang:fr"]).unwrap_err();
    assert!(error.to_string().contains("too-many-keys"), "{error}");
    let error = add(&mut service, &["x:n=3"]).unwrap_err();
    assert!(error.to_string().contains("out-of-range"), "{error}");
    assert!(add(&mut service, &["x:n=7"]).is_ok(), "in both ranges");
}

#[test]
fn test_an_existing_finding_is_a_warning_and_never_refuses_the_write() {
    let mut service = service();
    add(&mut service, &["lang:en", "lang:fr"]).unwrap();
    declare(&mut service, &["tagma.keys-per-item:lang=1"], &[]);
    let written = tag(&mut service, "swift-amber", &["x"], &[]).unwrap();
    let warned: Vec<(String, Origin)> = written
        .warnings()
        .iter()
        .map(|f| (format!("{} {}", f.code.as_str(), f.tag), f.origin))
        .collect();
    assert_eq!(
        warned,
        [
            ("too-many-keys lang:en".to_string(), Origin::Existing),
            ("too-many-keys lang:fr".to_string(), Origin::Existing),
        ]
    );
}

#[test]
fn test_a_write_that_fixes_an_existing_finding_is_not_warned() {
    let mut service = service();
    add(&mut service, &["lang:en", "lang:fr"]).unwrap();
    declare(&mut service, &["tagma.keys-per-item:lang=1"], &[]);
    let written = tag(&mut service, "swift-amber", &[], &["lang:fr"]).unwrap();
    assert!(written.warnings().is_empty());
}

#[test]
fn test_a_write_adding_a_second_key_to_an_already_over_namespace_is_refused() {
    let mut service = service();
    add(&mut service, &["lang:en", "lang:fr"]).unwrap();
    declare(&mut service, &["tagma.keys-per-item:lang=1"], &[]);
    let error = tag(&mut service, "swift-amber", &["lang:de"], &[]).unwrap_err();
    assert_eq!(error.code(), crate::fakes::tag_code("tag-rules"));
}

// ONE_KEY is gone: one area and one repo a task is the seeded
// `tagma.keys-per-item`, reported by validate, and data, so a store can
// drop it.
#[test]
fn test_one_area_a_task_is_the_seeded_keys_per_item_rule() {
    let mut service = service();
    let error = add(&mut service, &["area:cli", "area:core"]).unwrap_err();
    assert!(error.to_string().contains("too-many-keys"), "{error}");
    let error = add(&mut service, &["repo:a", "repo:b"]).unwrap_err();
    assert!(error.to_string().contains("too-many-keys"), "{error}");
    add(&mut service, &[]).unwrap();
    declare(&mut service, &[], &["tagma.keys-per-item:area=1"]);
    assert!(add(&mut service, &["area:cli", "area:core"]).is_ok());
}

#[test]
fn test_lint_reports_an_existing_two_areas_through_tagmas_finding() {
    let mut service = service();
    add(&mut service, &[]).unwrap();
    old_task(&service, "calm-otter", &["area:cli", "area:core"]);
    let report = ready(service.lint(Scope::default())).unwrap();
    let found: Vec<(&str, &str)> = report
        .violations
        .iter()
        .map(|v| (v.harp_id.as_str(), v.finding.code()))
        .collect();
    assert_eq!(found, [("calm-otter", code!("lint-rule"))]);
    assert!(report.violations[0]
        .finding
        .to_string()
        .contains("too-many-keys"));
}

// ------------------------------------------ the file layers are gone

#[test]
fn test_no_file_layer_is_consulted_for_tag_rules() {
    let mut service = service();
    // Files would narrow triage:level to 1-2, close area:x to a and b,
    // and declare an empty allowlist: none of it is read.
    add(&mut service, &["triage:level=4", "area:x=c", "lang:en"]).unwrap();
    declare(&mut service, &["tagma.keys-per-item:lang=1"], &[]);
    ready(service.permit(PermitChange {
        permit: "lang:".to_string(),
        ..Default::default()
    }))
    .unwrap();
    assert_eq!(service.configuration.asked, 0, "no file layer read");
    assert_eq!(service.configuration.written, 0, "no file written");
}

// --------------------------------------------- the taskloom namespace

#[test]
fn test_a_task_may_carry_no_tag_in_the_taskloom_namespace() {
    let mut service = service();
    for reserved in [
        r#"taskloom.priority_fn:"x:y"="1""#,
        "taskloom:x",
        "taskloom.sub:y",
    ] {
        let error = add(&mut service, &[reserved]).unwrap_err();
        assert_eq!(
            error.code(),
            crate::fakes::tag_code("tag-reserved-namespace"),
            "{reserved}"
        );
    }
    assert!(add(&mut service, &["taskloomish:x"]).is_ok());
}

// The formulas are taskloom's (ranking, not a constraint), so tagma's
// check never sees them as one of its facets, and the old
// `tagma.priority_fn` spelling is tagma's `unknown-facet`. The general
// default permits `area:` itself, so a new store checks clean at once; the
// ctxloom template leaves a project's `area:` and `repo:` keys to it, so
// tagma says its one-a-task limits constrain nothing yet, until the
// project permits one of each.
#[test]
fn test_check_is_tagmas_and_a_new_store_checks_clean() {
    let mut service = fresh();
    add(&mut service, &[]).unwrap();
    assert_eq!(codes(&mut service), []);
    declare(&mut service, &[r#"tagma.priority_fn:"x:y"="1""#], &[]);
    assert_eq!(
        codes(&mut service),
        [("unknown-facet", canonical(r#"tagma.priority_fn:"x:y"="1""#))]
    );
}

fn codes(service: &mut Store) -> Vec<(&'static str, String)> {
    let found = ready(service.schema_check(None)).unwrap();
    found
        .iter()
        .map(|f| (f.code.as_str(), f.tag.to_string()))
        .collect()
}

// The rule-on-unpermitted expectation, as the ctxloom template has it:
// `area` and `repo` until the project permits one of each.
#[test]
fn test_a_ctxloom_store_reports_area_and_repo_unpermitted_until_permitted() {
    let mut service = ctxloom();
    assert_eq!(
        codes(&mut service),
        [
            (
                "rule-on-unpermitted",
                canonical("tagma.keys-per-item:area=1")
            ),
            (
                "rule-on-unpermitted",
                canonical("tagma.keys-per-item:repo=1")
            ),
        ]
    );
    for permit in ["area:cli", "repo:tagma"] {
        ready(service.permit(PermitChange {
            permit: permit.to_string(),
            ..Default::default()
        }))
        .unwrap();
    }
    assert_eq!(codes(&mut service), []);
}

/// A service whose store holds the ctxloom template, as every store did
/// before the general default.
fn ctxloom() -> Store {
    let mut service = over(FakeIdentity::bound(), FakeStores::default());
    crate::fakes::declared(service.stores.store.inner(), &CTXLOOM_SCHEMA);
    add(&mut service, &[]).unwrap();
    service
}

#[test]
fn test_a_declare_of_a_tasks_tag_is_refused() {
    let mut service = service();
    add(&mut service, &[]).unwrap();
    let error = ready(service.declare(Declaration {
        add: vec!["area:cli".to_string()],
        remove: vec![],
        ..Default::default()
    }))
    .unwrap_err();
    let not_config = tasks::schema::SchemaRejection::NotConfig { tag: String::new() };
    assert!(
        matches!(&error, AppError::Rejected(r) if r.code() == not_config.code()),
        "{error:?}"
    );
}

// --------------------------------------------- permit and revoke

// F6: a store is an allowlist from when it is made, so a permit declares
// its own pattern and nothing else: no world to close, no target to keep.
#[test]
fn test_a_permit_declares_its_pattern_and_nothing_else() {
    let mut service = fresh();
    add(&mut service, &[]).unwrap();
    let error = add(&mut service, &["lang:en"]).unwrap_err();
    assert!(error.to_string().contains("not-permitted"), "{error}");
    ready(service.permit(PermitChange {
        permit: "lang:".to_string(),
        ..Default::default()
    }))
    .unwrap();
    let mut expected: Vec<&str> = SEEDED_SCHEMA.to_vec();
    expected.push(r#"tagma.permit:"lang:*""#);
    assert_eq!(export(&mut service), sorted(&expected));
    assert!(add(&mut service, &["lang:en"]).is_ok());
    assert!(
        add(&mut service, &["triage:kind=defect"]).is_ok(),
        "a target the seed constrains, the seed permits"
    );
    let error = add(&mut service, &["repo:tagma"]).unwrap_err();
    assert!(error.to_string().contains("not-permitted"), "{error}");
}

#[test]
fn test_each_permit_spelling_is_its_tagma_permit_pattern() {
    let mut service = fresh();
    add(&mut service, &[]).unwrap();
    for (permit, pattern) in [
        ("area:", r#"tagma.permit:"area:*""#),
        ("repo:tagma", r#"tagma.permit:"repo:tagma""#),
        ("urgent", "tagma.permit:urgent"),
    ] {
        ready(service.permit(PermitChange {
            permit: permit.to_string(),
            ..Default::default()
        }))
        .unwrap();
        assert!(
            export(&mut service).contains(&canonical(pattern)),
            "{permit}"
        );
    }
}

#[test]
fn test_a_revoke_of_what_tasks_still_carry_is_refused_listing_them() {
    let mut service = fresh();
    add(&mut service, &[]).unwrap();
    ready(service.permit(PermitChange {
        permit: "area:".to_string(),
        ..Default::default()
    }))
    .unwrap();
    add(&mut service, &["area:cli"]).unwrap();
    let error = ready(service.revoke(PermitChange {
        permit: "area:".to_string(),
        ..Default::default()
    }))
    .unwrap_err();
    assert_eq!(
        error,
        AppError::PermitInUse {
            permit: "area:".to_string(),
            harp_ids: vec!["calm-otter".to_string()],
        }
    );
    assert!(export(&mut service).contains(&canonical(r#"tagma.permit:"area:*""#)));
}

#[test]
fn test_a_revoke_no_task_needs_removes_its_permit_from_the_schema() {
    let mut service = fresh();
    add(&mut service, &[]).unwrap();
    for permit in ["area:", "lang:"] {
        ready(service.permit(PermitChange {
            permit: permit.to_string(),
            ..Default::default()
        }))
        .unwrap();
    }
    ready(service.revoke(PermitChange {
        permit: "lang:".to_string(),
        ..Default::default()
    }))
    .unwrap();
    let exported = export(&mut service);
    assert!(!exported.contains(&canonical(r#"tagma.permit:"lang:*""#)));
    assert!(exported.contains(&canonical(r#"tagma.permit:"area:*""#)));
}

// ------------------------------------------- the default and `schema init`

fn init(
    service: &mut Store,
    template: Template,
    areas: &[&str],
    dry_run: bool,
) -> Result<crate::Declared, AppError> {
    ready(service.schema_init(SchemaInit {
        template,
        areas: areas.iter().map(|a| a.to_string()).collect(),
        dry_run,
    }))
}

fn init_yes(service: &mut Store, template: Template, areas: &[&str]) -> crate::Declared {
    init(service, template, areas, false).unwrap()
}

/// A service over a store no one has written to: no project, no store.
fn untouched() -> Store {
    over(FakeIdentity::default(), FakeStores::default())
}

#[test]
fn test_a_fresh_store_accepts_an_area_as_its_first_write() {
    let mut service = fresh();
    let written = add(&mut service, &["area:cli"]).unwrap();
    assert_eq!(written.task.tags.len(), 1);
    assert_eq!(held(&service), sorted(&GENERAL_SCHEMA));
}

#[test]
fn test_the_general_default_keeps_repo_unpermitted_and_one_area_a_task() {
    let mut service = fresh();
    let error = add(&mut service, &["repo:tagma"]).unwrap_err();
    assert!(not_permitted(&error, "repo:tagma"), "{error}");
    let error = add(&mut service, &["area:cli", "area:core"]).unwrap_err();
    assert!(error.to_string().contains("too-many-keys"), "{error}");
    for out_of_the_default in [
        "triage:exposed=cli",
        "triage:blind-gate=ci",
        "triage:audited=20260101",
        "landed:x",
        "platform:linux",
        "sig:os.Mode",
    ] {
        let error = add(&mut service, &[out_of_the_default]).unwrap_err();
        assert!(
            not_permitted(&error, out_of_the_default.split('=').next().unwrap()),
            "{out_of_the_default}: {error}"
        );
    }
}

fn not_permitted(error: &AppError, tag: &str) -> bool {
    let shown = error.to_string();
    error.code() == crate::fakes::tag_code("tag-rules")
        && shown.contains("not-permitted")
        && shown.contains(tag)
}

// A store declared with the old seed is exactly as it was: the new default
// is read only where a stream has no history.
#[test]
fn test_a_store_declared_with_the_old_seed_is_unchanged_by_the_new_default() {
    let mut service = over(FakeIdentity::bound(), FakeStores::default());
    crate::fakes::declared(service.stores.store.inner(), &CTXLOOM_SCHEMA);
    let before = held(&service);
    assert_eq!(before, sorted(&CTXLOOM_SCHEMA));
    let error = add(&mut service, &["area:cli"]).unwrap_err();
    assert!(not_permitted(&error, "area:cli"), "{error}");
    assert!(add(&mut service, &["landed:x", "platform:linux", "sig:a.B"]).is_ok());
    assert_eq!(export(&mut service), before);
    assert_eq!(held(&service), before, "nothing was written to its schema");
}

// ------------------------------------------------------------- init

#[test]
fn test_init_previews_on_a_store_nothing_exists_for_and_writes_nothing() {
    let mut service = untouched();
    let preview = init(&mut service, Template::General, &[], true).unwrap();
    assert!(preview.changed);
    assert_eq!(preview.tags, sorted(&GENERAL_SCHEMA));
    assert_eq!(
        service.stores.opened.len(),
        0,
        "no store made for a preview"
    );
    assert!(
        held(&service).is_empty(),
        "the schema stream has no history"
    );
}

#[test]
fn test_init_yes_declares_the_template_on_a_store_with_no_history() {
    let mut service = untouched();
    let declared = init_yes(&mut service, Template::General, &[]);
    assert!(declared.changed);
    assert_eq!(declared.tags, sorted(&GENERAL_SCHEMA));
    assert_eq!(held(&service), sorted(&GENERAL_SCHEMA));
    assert!(add(&mut service, &["area:cli"]).is_ok());
}

#[test]
fn test_init_is_idempotent_and_a_second_run_changes_nothing() {
    let mut service = untouched();
    init_yes(&mut service, Template::General, &["cli", "docs"]);
    let first = held(&service);
    let again = init_yes(&mut service, Template::General, &["cli", "docs"]);
    assert!(!again.changed);
    assert_eq!(again.tags, first);
    assert_eq!(held(&service), first);
    let preview = init(&mut service, Template::General, &["cli", "docs"], true).unwrap();
    assert!(!preview.changed);
    assert_eq!(preview.tags, first);
}

#[test]
fn test_init_on_a_seeded_store_adds_nothing() {
    let mut service = fresh();
    add(&mut service, &[]).unwrap();
    let before = held(&service);
    let declared = init_yes(&mut service, Template::General, &[]);
    assert!(!declared.changed);
    assert_eq!(held(&service), before);
}

#[test]
fn test_init_areas_close_area_to_exactly_those_keys() {
    let mut service = untouched();
    let preview = init(&mut service, Template::General, &["cli", "docs"], true).unwrap();
    assert!(!preview
        .tags
        .contains(&canonical(r#"tagma.permit:"area:*""#)));
    assert!(preview
        .tags
        .contains(&canonical(r#"tagma.permit:"area:cli""#)));
    assert!(held(&service).is_empty());
    init_yes(&mut service, Template::General, &["cli", "docs", "cli"]);
    let held = held(&service);
    assert!(!held.contains(&canonical(r#"tagma.permit:"area:*""#)));
    assert_eq!(held.len(), GENERAL_SCHEMA.len() - 1 + 2, "a repeat is one");
    assert!(add(&mut service, &["area:docs"]).is_ok());
    let error = add(&mut service, &["area:other"]).unwrap_err();
    assert!(not_permitted(&error, "area:other"), "{error}");
}

#[test]
fn test_init_adds_more_areas_to_a_closed_set_and_never_opens_it() {
    let mut service = untouched();
    init_yes(&mut service, Template::General, &["cli"]);
    let more = init_yes(&mut service, Template::General, &["cli", "docs"]);
    assert!(more.changed);
    assert!(more
        .tags
        .contains(&canonical(r#"tagma.permit:"area:docs""#)));
    let plain = init_yes(&mut service, Template::General, &[]);
    assert!(!plain.changed, "no --area does not open a closed area");
    assert!(!plain.tags.contains(&canonical(r#"tagma.permit:"area:*""#)));
}

#[test]
fn test_init_areas_on_an_open_area_is_refused_naming_the_permit() {
    let mut service = fresh();
    add(&mut service, &[]).unwrap();
    let error = init(&mut service, Template::General, &["cli"], false).unwrap_err();
    assert_eq!(
        error,
        AppError::SchemaInitConflict {
            template: "general".to_string(),
            held: canonical(r#"tagma.permit:"area:*""#),
        }
    );
    assert_eq!(error.exit().code(false), 48);
    assert_eq!(
        init(&mut service, Template::General, &["cli"], true).unwrap_err(),
        error,
        "a preview refuses as the apply does"
    );
    assert_eq!(held(&service), sorted(&GENERAL_SCHEMA), "nothing changed");
}

#[test]
fn test_init_ctxloom_declares_the_old_seed() {
    let mut service = untouched();
    let declared = init_yes(&mut service, Template::Ctxloom, &[]);
    assert_eq!(declared.tags, sorted(&CTXLOOM_SCHEMA));
    assert_eq!(held(&service), sorted(&CTXLOOM_SCHEMA));
    let error = add(&mut service, &["area:cli"]).unwrap_err();
    assert!(not_permitted(&error, "area:cli"), "{error}");
    let mut with_areas = untouched();
    let declared = init_yes(&mut with_areas, Template::Ctxloom, &["cli"]);
    assert_eq!(declared.tags.len(), CTXLOOM_SCHEMA.len() + 1);
    assert!(add(&mut with_areas, &["area:cli"]).is_ok());
}

#[test]
fn test_init_refuses_a_template_that_would_change_a_rule_the_store_holds() {
    // The old seed's formula is not the general default's: adding the
    // general set would be a second rule for the target.
    let mut service = over(FakeIdentity::bound(), FakeStores::default());
    crate::fakes::declared(service.stores.store.inner(), &CTXLOOM_SCHEMA);
    let error = init(&mut service, Template::General, &[], false).unwrap_err();
    let AppError::SchemaInitConflict { template, held } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(template, "general");
    assert!(held.starts_with("taskloom.priority_fn"), "{held}");
    assert_eq!(held_tags(&service), sorted(&CTXLOOM_SCHEMA), "unchanged");
}

fn held_tags(service: &Store) -> Vec<String> {
    held(service)
}

#[test]
fn test_init_adds_only_what_a_store_with_its_own_rules_lacks() {
    let mut service = over(FakeIdentity::bound(), FakeStores::default());
    crate::fakes::declared(
        service.stores.store.inner(),
        &[
            r#"tagma.permit:"lang:*""#,
            r#"tagma.range:"triage:level"=1..5"#,
        ],
    );
    let declared = init(&mut service, Template::General, &[], false).unwrap();
    assert!(declared.changed);
    let mut expected: Vec<&str> = GENERAL_SCHEMA.to_vec();
    expected.push(r#"tagma.permit:"lang:*""#);
    assert_eq!(held(&service), sorted(&expected));
}

#[test]
fn test_init_migrates_a_store_from_before_f6_with_its_old_permits() {
    let mut service = over(FakeIdentity::bound(), FakeStores::default());
    crate::fakes::declared(service.stores.store.inner(), &seed_without_permits());
    let preview = init(&mut service, Template::Ctxloom, &[], true).unwrap();
    // Read as it is read everywhere, the store already holds the old
    // permits: there is nothing to add, and a preview writes nothing.
    assert!(!preview.changed);
    assert_eq!(preview.tags, sorted(&CTXLOOM_SCHEMA));
    assert_eq!(held(&service), sorted(&seed_without_permits()));
    let declared = init_yes(&mut service, Template::Ctxloom, &[]);
    assert!(!declared.changed);
    assert_eq!(declared.tags, sorted(&CTXLOOM_SCHEMA));
    assert_eq!(held(&service), sorted(&CTXLOOM_SCHEMA));
}

#[test]
fn test_init_refuses_a_blank_or_malformed_area_before_it_reads_anything() {
    let mut service = untouched();
    assert_eq!(
        init(&mut service, Template::General, &[" "], false).unwrap_err(),
        AppError::Blank { field: "area" }
    );
    for bad in ["a:b", "x=1", "a b"] {
        let error = init(&mut service, Template::General, &[bad], false).unwrap_err();
        assert!(
            matches!(&error, AppError::PermitMalformed { permit } if permit == &format!("area:{bad}")),
            "{bad}: {error:?}"
        );
    }
    assert_eq!(service.stores.opened.len(), 0);
    assert!(held(&service).is_empty());
}

#[test]
fn test_init_is_refused_where_a_layer_still_sets_a_moved_key() {
    struct Moved;
    impl ports::Configuration for Moved {
        fn tag_schema(&mut self) -> Result<Vec<String>, Report> {
            panic!("only the import reads the moved keys")
        }

        fn moved(&mut self) -> Result<(), Report> {
            Err(Report::new("a-key-moved"))
        }
    }
    impl ports::ConfigurationWriter for Moved {
        fn write_tag_permits(&mut self, _: &TagPermits) -> Result<(), Report> {
            Ok(())
        }
    }
    let mut service = Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        FakeNames::of(&HARPS),
        FakeClock::default(),
        FakeDelay::default(),
    )
    .with_configuration(Moved);
    let error = ready(service.schema_init(SchemaInit::default())).unwrap_err();
    assert!(matches!(error, AppError::Configuration(_)), "{error:?}");
}
