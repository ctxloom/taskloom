//! RED (red/self-hosted-schema, for the owner's review): the store's own
//! tag rules, over fake ports and angzarr-ports' in-memory store
//! (docs/plans/self-hosted-schema.md).
//!
//! The boundary these pin: the rules are tagma config tags on the store's
//! schema stream, and tagma alone interprets them (`Rules::validate`,
//! `Rules::check`). A rule taskloom has no code for (keys-per-item, range
//! intersection, introduced/existing) works through taskloom because
//! tagma's does; no file layer is read for them.

use angzarr_embedded_diagnostic::{Diagnostic, Report};
use ports::TagPermits;

use crate::fakes::{ready, FakeClock, FakeDelay, FakeIdentity, FakeNames, FakeStores};
use crate::{
    AppError, Declaration, ListQuery, NewTask, Origin, PermitChange, PermitService, SchemaService,
    Scope, Service, TagChange, TaskService, Written, SEEDED_SCHEMA,
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

fn service() -> Store {
    Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        FakeNames::of(&HARPS),
        FakeClock::default(),
        FakeDelay::default(),
    )
    .with_configuration(Files::default())
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
    crate::fakes::run(&service.stores.store.inner, harp, add.into());
}

// ------------------------------------------------------- seeding

#[test]
fn test_the_seeded_schema_is_tagma_syntax_in_its_canonical_spelling() {
    for spelled in SEEDED_SCHEMA {
        assert_eq!(canonical(spelled), spelled);
    }
}

#[test]
fn test_a_new_store_is_seeded_with_the_built_in_rules_when_it_is_made() {
    let mut service = service();
    add(&mut service, &[]).unwrap();
    let mut seeded: Vec<String> = SEEDED_SCHEMA.iter().map(|s| s.to_string()).collect();
    seeded.sort();
    assert_eq!(export(&mut service), seeded);
}

#[test]
fn test_a_store_made_before_the_schema_stream_is_seeded_on_first_open() {
    let mut service = service();
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
    assert_eq!(code, "tasks-application-tag-rules");
    assert!(shown.contains("not-in-enum"), "{shown}");
    assert!(shown.contains("area:x=b"), "{shown}");
    let error = tag(&mut service, "swift-amber", &["area:x=b"], &[]).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-rules");
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
    assert_eq!(error.code(), "tasks-application-tag-rules");
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
    assert_eq!(found, [("calm-otter", "tasks-application-lint-rule")]);
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
            "tasks-application-tag-reserved-namespace",
            "{reserved}"
        );
    }
    assert!(add(&mut service, &["taskloomish:x"]).is_ok());
}

// The formulas are taskloom's (ranking, not a constraint), so tagma's
// check never sees them as one of its facets: a seeded store checks
// clean, and the old `tagma.priority_fn` spelling is tagma's
// `unknown-facet`.
#[test]
fn test_check_is_tagmas_and_the_seeded_store_checks_clean() {
    let mut service = service();
    add(&mut service, &[]).unwrap();
    assert_eq!(ready(service.schema_check(None)).unwrap(), []);
    declare(&mut service, &[r#"tagma.priority_fn:"x:y"="1""#], &[]);
    let found = ready(service.schema_check(None)).unwrap();
    let codes: Vec<(&str, String)> = found
        .iter()
        .map(|f| (f.code.as_str(), f.tag.to_string()))
        .collect();
    assert_eq!(
        codes,
        [("unknown-facet", canonical(r#"tagma.priority_fn:"x:y"="1""#))]
    );
}

#[test]
fn test_a_declare_of_a_tasks_tag_is_refused() {
    let mut service = service();
    add(&mut service, &[]).unwrap();
    let error = ready(service.declare(Declaration {
        add: vec!["area:cli".to_string()],
        remove: vec![],
    }))
    .unwrap_err();
    assert!(
        error.to_string().contains("tasks-schema-not-config"),
        "{error}"
    );
}

// --------------------------------------------- permit and revoke

#[test]
fn test_a_first_permit_closes_the_world_and_keeps_the_declared_targets_admitted() {
    let mut service = service();
    add(&mut service, &[]).unwrap();
    ready(service.permit(PermitChange {
        permit: "area:".to_string(),
    }))
    .unwrap();
    let exported = export(&mut service);
    for expected in [
        r#"tagma.permit:"*:*"=false"#,
        r#"tagma.permit:"area:*"=true"#,
    ] {
        assert!(exported.contains(&canonical(expected)), "{expected}");
    }
    assert!(add(&mut service, &["area:cli"]).is_ok());
    assert!(
        add(&mut service, &["triage:kind=defect"]).is_ok(),
        "a target the schema declares rules for stays admitted"
    );
    let error = add(&mut service, &["lang:en"]).unwrap_err();
    assert!(error.to_string().contains("not-permitted"), "{error}");
}

#[test]
fn test_each_permit_spelling_is_its_tagma_permit_pattern() {
    let mut service = service();
    add(&mut service, &[]).unwrap();
    for (permit, pattern) in [
        ("area:", r#"tagma.permit:"area:*"=true"#),
        ("repo:tagma", r#"tagma.permit:"repo:tagma"=true"#),
        ("urgent", "tagma.permit:urgent=true"),
    ] {
        ready(service.permit(PermitChange {
            permit: permit.to_string(),
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
    let mut service = service();
    add(&mut service, &[]).unwrap();
    ready(service.permit(PermitChange {
        permit: "area:".to_string(),
    }))
    .unwrap();
    add(&mut service, &["area:cli"]).unwrap();
    let error = ready(service.revoke(PermitChange {
        permit: "area:".to_string(),
    }))
    .unwrap_err();
    assert_eq!(
        error,
        AppError::PermitInUse {
            permit: "area:".to_string(),
            harp_ids: vec!["calm-otter".to_string()],
        }
    );
    assert!(export(&mut service).contains(&canonical(r#"tagma.permit:"area:*"=true"#)));
}

#[test]
fn test_a_revoke_no_task_needs_removes_its_permit_from_the_schema() {
    let mut service = service();
    add(&mut service, &[]).unwrap();
    for permit in ["area:", "lang:"] {
        ready(service.permit(PermitChange {
            permit: permit.to_string(),
        }))
        .unwrap();
    }
    ready(service.revoke(PermitChange {
        permit: "lang:".to_string(),
    }))
    .unwrap();
    let exported = export(&mut service);
    assert!(!exported.contains(&canonical(r#"tagma.permit:"lang:*"=true"#)));
    assert!(exported.contains(&canonical(r#"tagma.permit:"area:*"=true"#)));
}
