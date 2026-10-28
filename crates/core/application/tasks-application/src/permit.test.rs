//! Tag permits, over fake ports and annals' in-memory store: what a write
//! admits under them, how a permit and a revoke change them, and what
//! lint says of a stored tag they do not permit.
//!
//! The rulings cited are tag-permits.plan.md's (session
//! inept-sixth-stick); a test marked PROPOSAL pins an answer no ruling
//! has given yet, for the human to accept or change.

use std::collections::{BTreeMap, BTreeSet};

use annals_diagnostic::{Diagnostic, Report};
use ports::{NamespacePermit, TagPermits};

use crate::fakes::{ready, FakeClock, FakeDelay, FakeIdentity, FakeNames, FakeStores};
use crate::{
    AppError, Finding, NewTask, PermitChange, PermitList, PermitService, Permitted, Scope, Service,
    StatusChange, TagChange, TaskService, Written,
};

/// The project's permits as configured (`None`: it declares none), the
/// tag_schema declarations configured beside the default schema, and
/// every set of permits written, in order.
#[derive(Default)]
struct FakeConfig {
    permits: Option<TagPermits>,
    schema: Vec<String>,
    written: Vec<TagPermits>,
}

impl ports::Configuration for FakeConfig {
    fn tag_schema(&mut self) -> Result<Vec<String>, Report> {
        Ok(self.schema.clone())
    }

    fn tag_permits(&mut self) -> Result<Option<TagPermits>, Report> {
        Ok(self.permits.clone())
    }
}

impl ports::ConfigurationWriter for FakeConfig {
    fn write_tag_permits(&mut self, permits: &TagPermits) -> Result<(), Report> {
        self.written.push(permits.clone());
        self.permits = Some(permits.clone());
        Ok(())
    }
}

type Permits = Service<FakeIdentity, FakeStores, FakeNames, FakeClock, FakeDelay, FakeConfig>;

fn keys(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

/// Permits of `closed` namespaces with their keys, `open` namespaces and
/// `flat` tags.
fn permits(closed: &[(&str, &[&str])], open: &[&str], flat: &[&str]) -> TagPermits {
    let mut namespaces = BTreeMap::new();
    for (namespace, list) in closed {
        namespaces.insert(namespace.to_string(), NamespacePermit::Closed(keys(list)));
    }
    for namespace in open {
        namespaces.insert(namespace.to_string(), NamespacePermit::Open);
    }
    TagPermits {
        namespaces,
        flat: keys(flat),
    }
}

/// The permits most tests run under: `area:` closed to `cli` and `mcp`,
/// `relates:` open, and the flat `urgent`.
fn usual() -> TagPermits {
    permits(&[("area", &["cli", "mcp"])], &["relates"], &["urgent"])
}

/// A service whose project declares `permits`, minting `names` in order.
fn under(permits: Option<TagPermits>, names: &[&str]) -> Permits {
    Service::new(
        FakeIdentity::default(),
        FakeStores::default(),
        FakeNames::of(names),
        FakeClock::default(),
        FakeDelay::default(),
    )
    .with_configuration(FakeConfig {
        permits,
        ..FakeConfig::default()
    })
}

/// The targets the default schema declares, sorted: every one is
/// permitted without a permit.
const DEFAULT_TARGETS: [&str; 8] = [
    "triage:audited",
    "triage:blind-gate",
    "triage:blocks-release",
    "triage:effort",
    "triage:exposed",
    "triage:kind",
    "triage:level",
    "triage:verdict",
];

/// The factual flags (ruling 14): bare keys of `triage:`, scored by
/// nothing, permitted by a project through a closed `triage:` list.
const FLAGS: [&str; 7] = [
    "security",
    "data-loss",
    "crashes",
    "no-workaround",
    "regression",
    "silent-no-op",
    "flaky",
];

/// `harp_id` moved to `status` (Deferred given a trigger).
fn moved(service: &mut Permits, harp_id: &str, status: &str) {
    ready(service.set_status(StatusChange {
        harp_id: harp_id.to_string(),
        status: status.to_string(),
        trigger: (status == "Deferred").then(|| "when it matters".to_string()),
        ..StatusChange::default()
    }))
    .unwrap();
}

/// Lint's violations, by harp id.
fn lint_findings(service: &mut Permits) -> Vec<(String, Finding)> {
    let report = ready(service.lint(Scope::default())).unwrap();
    report
        .violations
        .into_iter()
        .map(|v| (v.harp_id, v.finding))
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

fn arg(error: &dyn Diagnostic, name: &str) -> String {
    error
        .args()
        .into_iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, value)| match value {
            annals_diagnostic::Arg::Str(s) => Some(s),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no arg {name} on {}", error.code()))
}

fn spelled(written: &Written) -> Vec<String> {
    written.task.tags.iter().map(ToString::to_string).collect()
}

// ------------------------------------------------------------ admission

// Ruling 2: a tag not permitted is refused, naming the tag and the command
// that would permit it, before a project is resolved or a store opened.
// The permit named is the tag's target, never its value: a key, not its
// namespace, so the suggestion is the narrowest that admits it.
#[test]
fn test_an_add_with_a_tag_not_permitted_is_refused_naming_it_and_its_permit() {
    for (tag, named) in [
        ("area:host-hardening", "area:host-hardening"),
        ("sig:foo=bar", "sig:foo"),
        ("wibble", "wibble"),
    ] {
        let mut service = under(Some(usual()), &["swift-amber"]);
        let error = add(&mut service, &["urgent", tag]).unwrap_err();
        assert_eq!(error.code(), "tasks-application-tag-not-permitted", "{tag}");
        assert!(matches!(error, AppError::TagRefused(_)));
        assert_eq!(arg(&error, "tag"), tag);
        assert_eq!(arg(&error, "permit"), named);
        assert!(
            error
                .to_string()
                .contains(&format!("`taskloom permit {named}`")),
            "{error}"
        );
        assert!(service.identity.pins.is_empty(), "refused before resolving");
        assert!(service.stores.opened.is_empty(), "nothing written");
    }
}

#[test]
fn test_a_tag_change_adding_a_tag_not_permitted_is_refused_and_changes_nothing() {
    let mut service = under(Some(usual()), &["swift-amber"]);
    add(&mut service, &["area:cli"]).unwrap();
    let change = TagChange {
        harp_id: "swift-amber".to_string(),
        add: vec!["urgent".to_string(), "area:host-hardening".to_string()],
        ..TagChange::default()
    };
    let error = ready(service.tag(change)).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-not-permitted");
    assert_eq!(arg(&error, "permit"), "area:host-hardening");
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
    let mut service = under(Some(usual()), &["swift-amber"]);
    let written = add(&mut service, &["area:cli", "relates:calm-otter", "urgent"]).unwrap();
    assert_eq!(
        spelled(&written),
        ["area:cli", "relates:calm-otter", "urgent"]
    );
}

// Ruling 6 (kept from the first proposal): a permit is of a target, never
// a value, so a closed namespace's key carries any value the schema admits.
#[test]
fn test_a_closed_namespaces_key_may_carry_a_value() {
    let mut service = under(Some(usual()), &["swift-amber"]);
    let written = add(&mut service, &["area:cli=x"]).unwrap();
    assert_eq!(spelled(&written), ["area:cli=x"]);
}

// Ruling 14: a target the resolved schema declares (the built-in
// triage:* among them) is permitted without a permit, as a KEY: its
// namespace is not opened by it. triage:was-release is declared nowhere,
// so it is refused.
#[test]
fn test_the_schemas_declared_targets_are_permitted_without_a_permit() {
    let mut service = under(Some(TagPermits::default()), &["swift-amber", "calm-otter"]);
    let written = add(
        &mut service,
        &["triage:kind=defect", "triage:level=2", "triage:exposed=cli"],
    )
    .unwrap();
    assert_eq!(written.task.tags.len(), 3);
    let error = add(&mut service, &["triage:was-release=1.0.0"]).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-not-permitted");
    assert_eq!(arg(&error, "permit"), "triage:was-release");
}

// Ruling 5: triage:blocks-release=<semver> is the one release form, and
// permits do not break it: admitted with no permit, still held to its
// declared type, and any other spelling refused as not permitted.
#[test]
fn test_release_has_one_form_and_permits_do_not_break_it() {
    let mut service = under(
        Some(usual()),
        &["swift-amber", "calm-otter", "brisk-oak", "lone-elm"],
    );
    let written = add(&mut service, &["triage:blocks-release=1.2.0"]).unwrap();
    assert_eq!(spelled(&written), ["triage:blocks-release=1.2.0"]);
    let error = add(&mut service, &["triage:blocks-release=next"]).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-not-semver");
    for other in ["release:1.2.0", "blocks-release", "release=1.2.0"] {
        let error = add(&mut service, &[other]).unwrap_err();
        assert_eq!(
            error.code(),
            "tasks-application-tag-not-permitted",
            "{other}"
        );
    }
}

// Ruling 6 (opt-in): a project whose config declares no permits admits
// every tag the schema does, as before.
#[test]
fn test_a_project_declaring_no_permits_stays_permissive() {
    let mut service = under(None, &["swift-amber"]);
    let written = add(&mut service, &["area:host-hardening", "wibble"]).unwrap();
    assert_eq!(written.task.tags.len(), 2);
}

// Only what a write adds is held to the permits: a tag held from before is
// removed, and kept beside one added, without refusal.
#[test]
fn test_a_tag_held_from_before_the_permits_is_never_checked() {
    let mut service = under(None, &["swift-amber"]);
    add(&mut service, &["wibble", "area:host-hardening"]).unwrap();
    service.configuration.permits = Some(usual());
    let change = TagChange {
        harp_id: "swift-amber".to_string(),
        add: vec!["urgent".to_string()],
        remove: vec!["wibble".to_string()],
        ..TagChange::default()
    };
    let written = ready(service.tag(change)).unwrap();
    assert_eq!(spelled(&written), ["area:host-hardening", "urgent"]);
}

// Ruling 14: the factual flags stay bare keys, permitted by a closed
// `triage:` list in the project's permits. That list closes `triage:` to
// its keys, yet the targets the schema declares (kind, level,
// blind-gate, ...) stay permitted beside it, each still held to its
// declaration; a key neither lists nor declares is refused.
#[test]
fn test_a_closed_triage_list_coexists_with_the_schemas_declared_targets() {
    let mut service = under(
        Some(permits(&[("triage", &FLAGS)], &[], &[])),
        &[
            "swift-amber",
            "calm-otter",
            "brisk-oak",
            "lone-elm",
            "tall-oak",
        ],
    );
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
    for (tag, named) in [
        ("triage:fact=security", "triage:fact"),
        ("triage:was-release=1.0.0", "triage:was-release"),
        ("triage:flake", "triage:flake"),
    ] {
        let error = add(&mut service, &[tag]).unwrap_err();
        assert_eq!(error.code(), "tasks-application-tag-not-permitted", "{tag}");
        assert_eq!(arg(&error, "permit"), named, "{tag}");
    }
    let error = add(&mut service, &["triage:kind=bug"]).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-not-in-enum");
}

// Ruling 14: triage:blind-gate is declared by the default schema (see
// schema.test.rs), so a project permitting nothing admits it, more than
// one gate at once (it is repeatable), and refuses a gate not in its enum.
#[test]
fn test_blind_gate_is_permitted_by_its_declaration_and_held_to_its_enum() {
    let mut service = under(Some(TagPermits::default()), &["swift-amber", "calm-otter"]);
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
    assert_eq!(error.code(), "tasks-application-tag-not-in-enum");
}

// Ruling 13: the core vocabulary is COPIED into each project's permits;
// nothing is built in. A project permitting nothing refuses every core
// target the schema does not declare, and its permits list none.
#[test]
fn test_no_vocabulary_is_built_in_the_config_is_the_only_source() {
    let mut service = under(Some(TagPermits::default()), &["swift-amber"]);
    for tag in [
        "human:decision",
        "relates:calm-otter",
        "area:cli",
        "repo:annals",
        "queue:20261027",
        "platform:linux",
        "triage:security",
    ] {
        let error = add(&mut service, &[tag]).unwrap_err();
        assert_eq!(error.code(), "tasks-application-tag-not-permitted", "{tag}");
    }
    let listed = ready(service.permits()).unwrap();
    assert_eq!(listed.permits, Some(TagPermits::default()));
}

// ------------------------------------------------------------ the list

// Ruling 9: what is permitted is readable, by a person (`taskloom
// permits`) and an agent (`taskloom://tag-permits`) alike: the permits as
// configured, and every target the resolved schema declares, sorted, a
// project's own declarations among them. It is configuration, so no
// project is resolved and no store opened.
#[test]
fn test_the_permits_are_listed_with_the_targets_the_schema_permits() {
    let mut service = under(Some(usual()), &[]);
    service.configuration.schema = vec![r#"tagma.enum:"area:x"="a,b""#.to_string()];
    let listed = ready(service.permits()).unwrap();
    let mut targets = vec!["area:x".to_string()];
    targets.extend(DEFAULT_TARGETS.iter().map(|t| t.to_string()));
    assert_eq!(
        listed,
        PermitList {
            permits: Some(usual()),
            schema_targets: targets,
        }
    );
    assert!(service.identity.pins.is_empty(), "no project resolved");
    assert!(service.stores.opened.is_empty() && service.stores.looked.is_empty());
}

// Ruling 6 (opt-in), as the list says it: no permits declared is `None`
// (everything the schema admits is permitted), not an empty set (nothing
// is).
#[test]
fn test_a_project_declaring_no_permits_lists_none() {
    let mut service = under(None, &[]);
    let listed = ready(service.permits()).unwrap();
    assert_eq!(listed.permits, None);
    assert_eq!(listed.schema_targets, DEFAULT_TARGETS);
}

// ------------------------------------------------------------ permit

// Ruling 3: a permit persists through the configuration; it needs no task
// store, so none is opened.
#[test]
fn test_a_permit_grants_a_namespace_a_key_or_a_flat_tag() {
    let cases: [(&str, TagPermits); 3] = [
        ("area:", permits(&[], &["area"], &[])),
        ("area:cli", permits(&[("area", &["cli"])], &[], &[])),
        ("urgent", permits(&[], &[], &["urgent"])),
    ];
    for (spelled, expected) in cases {
        let mut service = under(Some(TagPermits::default()), &[]);
        let permitted = permit(&mut service, spelled).unwrap();
        assert_eq!(
            permitted,
            Permitted {
                permit: spelled.to_string(),
                changed: true
            }
        );
        assert_eq!(service.configuration.written, [expected], "{spelled}");
        assert!(service.stores.opened.is_empty() && service.stores.looked.is_empty());
    }
}

// The opt-in moment: the first permit of a project that declares none
// starts its permits, and every tag not permitted is refused from then on.
#[test]
fn test_the_first_permit_of_a_project_declaring_none_starts_its_permits() {
    let mut service = under(None, &["swift-amber"]);
    permit(&mut service, "area:cli").unwrap();
    assert_eq!(
        service.configuration.written,
        [permits(&[("area", &["cli"])], &[], &[])]
    );
    let error = add(&mut service, &["wibble"]).unwrap_err();
    assert_eq!(error.code(), "tasks-application-tag-not-permitted");
}

#[test]
fn test_a_permit_adds_a_key_to_a_closed_namespace() {
    let mut service = under(Some(usual()), &[]);
    permit(&mut service, "area:config").unwrap();
    assert_eq!(
        service.configuration.written,
        [permits(
            &[("area", &["cli", "config", "mcp"])],
            &["relates"],
            &["urgent"]
        )]
    );
}

// A permit is spelled as the tag is, canonically: whitespace trimmed, as a
// tag given to a write is.
#[test]
fn test_a_permit_is_answered_spelled_canonically() {
    let mut service = under(Some(TagPermits::default()), &[]);
    let permitted = permit(&mut service, "  area:cli ").unwrap();
    assert_eq!(permitted.permit, "area:cli");
}

// Idempotent: what is already permitted (a closed namespace's key, any key
// of an open one, a flat tag) is answered unchanged, and nothing written.
#[test]
fn test_a_permit_of_what_is_already_permitted_changes_nothing() {
    for spelled in ["area:cli", "relates:calm-otter", "relates:", "urgent"] {
        let mut service = under(Some(usual()), &[]);
        let permitted = permit(&mut service, spelled).unwrap();
        assert!(!permitted.changed, "{spelled}");
        assert!(service.configuration.written.is_empty(), "{spelled}");
    }
}

// PROPOSAL: permitting a closed namespace opens it, its keys dropped:
// only widening, so no stored tag becomes unpermitted.
#[test]
fn test_permitting_a_closed_namespace_opens_it() {
    let mut service = under(Some(usual()), &[]);
    let permitted = permit(&mut service, "area:").unwrap();
    assert!(permitted.changed);
    assert_eq!(
        service.configuration.written,
        [permits(&[], &["area", "relates"], &["urgent"])]
    );
}

#[test]
fn test_a_permit_not_spelled_as_one_is_refused_and_nothing_written() {
    for spelled in [
        "area:cli=x",
        "urgent=1",
        "tagma.enum:",
        "tagma:x",
        "and",
        "a b",
        ":",
    ] {
        let mut service = under(Some(usual()), &[]);
        let error = permit(&mut service, spelled).unwrap_err();
        assert_eq!(
            error.code(),
            "tasks-application-permit-malformed",
            "{spelled}"
        );
        assert!(service.configuration.written.is_empty(), "{spelled}");
    }
    let mut service = under(Some(usual()), &[]);
    let error = permit(&mut service, "  ").unwrap_err();
    assert_eq!(error, AppError::Blank { field: "permit" });
    let error = revoke(&mut service, "").unwrap_err();
    assert_eq!(error, AppError::Blank { field: "permit" });
}

// ------------------------------------------------------------ revoke

#[test]
fn test_a_revoke_no_task_needs_removes_the_permit() {
    let cases: [(&str, TagPermits); 3] = [
        (
            "area:mcp",
            permits(&[("area", &["cli"])], &["relates"], &["urgent"]),
        ),
        (
            "relates:",
            permits(&[("area", &["cli", "mcp"])], &[], &["urgent"]),
        ),
        (
            "urgent",
            permits(&[("area", &["cli", "mcp"])], &["relates"], &[]),
        ),
    ];
    for (spelled, expected) in cases {
        let mut service = under(Some(usual()), &[]);
        let revoked = revoke(&mut service, spelled).unwrap();
        assert!(revoked.changed, "{spelled}");
        assert_eq!(service.configuration.written, [expected], "{spelled}");
    }
}

// Ruling 4: revoking what stored tasks still carry is refused, listing
// them by harp id (sorted), and nothing is written. A key's tasks are
// those carrying it with any value; a namespace's, any key of it.
#[test]
fn test_a_revoke_of_what_tasks_still_carry_is_refused_listing_them() {
    let mut service = under(Some(usual()), &["swift-amber", "calm-otter", "brisk-oak"]);
    add(&mut service, &["area:cli"]).unwrap();
    add(&mut service, &["area:cli=x", "urgent"]).unwrap();
    add(&mut service, &["area:mcp"]).unwrap();
    let cases: [(&str, &[&str]); 3] = [
        ("area:cli", &["calm-otter", "swift-amber"]),
        ("area:", &["brisk-oak", "calm-otter", "swift-amber"]),
        ("urgent", &["calm-otter"]),
    ];
    for (spelled, carriers) in cases {
        let error = revoke(&mut service, spelled).unwrap_err();
        let expected: Vec<String> = carriers.iter().map(|c| c.to_string()).collect();
        assert_eq!(
            error,
            AppError::PermitInUse {
                permit: spelled.to_string(),
                harp_ids: expected
            }
        );
        assert!(service.configuration.written.is_empty(), "{spelled}");
    }
}

// Ruling 6 (revoke refused while in use): a task carries a tag whatever
// its status, so a finished one still carrying it blocks the revoke too:
// retag it, or prune it. (Lint, unlike revoke, leaves a closed row's tags
// alone: they are its history, not a vocabulary in use.)
#[test]
fn test_a_finished_task_still_carrying_a_permit_blocks_its_revoke() {
    let mut service = under(Some(usual()), &["swift-amber"]);
    add(&mut service, &["area:mcp"]).unwrap();
    ready(service.set_status(StatusChange {
        harp_id: "swift-amber".to_string(),
        status: "Done".to_string(),
        ..StatusChange::default()
    }))
    .unwrap();
    let error = revoke(&mut service, "area:mcp").unwrap_err();
    assert_eq!(error.code(), "tasks-application-permit-in-use");
}

// A revoke of what is not among the permits is refused: a namespace never
// permitted, a key never permitted, a key of an OPEN namespace (its
// namespace's permit admits it; revoke that), and anything at all where
// the project declares no permits.
#[test]
fn test_a_revoke_of_what_is_not_among_the_permits_is_refused() {
    for spelled in ["sig:", "area:config", "relates:calm-otter", "wibble"] {
        let mut service = under(Some(usual()), &[]);
        let error = revoke(&mut service, spelled).unwrap_err();
        assert_eq!(
            error,
            AppError::PermitNotDeclared {
                permit: spelled.to_string()
            }
        );
        assert!(service.configuration.written.is_empty(), "{spelled}");
    }
    let mut service = under(None, &[]);
    let error = revoke(&mut service, "area:cli").unwrap_err();
    assert_eq!(error.code(), "tasks-application-permit-not-declared");
}

// ------------------------------------------------------------ lint

// Lint sees what admission cannot: a stored tag the permits do not permit
// (written before them, or imported). Ruling 15: only on the rows still
// in play, open and Deferred; closed rows are the next test's.
#[test]
fn test_lint_reports_a_stored_tag_the_permits_do_not_permit() {
    let mut service = under(None, &["swift-amber", "calm-otter"]);
    add(&mut service, &["area:host-hardening", "area:cli", "urgent"]).unwrap();
    add(&mut service, &["wibble"]).unwrap();
    moved(&mut service, "calm-otter", "Deferred");
    service.configuration.permits = Some(usual());
    let found = lint_findings(&mut service);
    assert_eq!(
        found,
        [
            (
                "calm-otter".to_string(),
                Finding::NotPermitted {
                    tag: "wibble".to_string(),
                    permit: "wibble".to_string(),
                }
            ),
            (
                "swift-amber".to_string(),
                Finding::NotPermitted {
                    tag: "area:host-hardening".to_string(),
                    permit: "area:host-hardening".to_string(),
                }
            ),
        ]
    );
    assert_eq!(found[0].1.code(), "tasks-application-lint-not-permitted");
}

// Ruling 15: a closed row's tags are its history (nothing is stripped from
// a closed row), so lint does not report them, Done or Archived. The open
// row beside them shows the lint did look.
#[test]
fn test_lint_leaves_a_closed_rows_tags_alone() {
    let mut service = under(None, &["swift-amber", "calm-otter", "brisk-oak"]);
    add(&mut service, &["area:host-hardening"]).unwrap();
    add(&mut service, &["wibble", "area:cli", "area:mcp"]).unwrap();
    add(&mut service, &["wibble"]).unwrap();
    moved(&mut service, "swift-amber", "Done");
    moved(&mut service, "calm-otter", "Archived");
    service.configuration.permits = Some(usual());
    let found = lint_findings(&mut service);
    let subjects: Vec<&str> = found.iter().map(|(harp, _)| harp.as_str()).collect();
    assert_eq!(subjects, ["brisk-oak"], "{found:?}");
}

// Ruling 15: a task takes one `area:` and one `repo:`. Arity binds only
// `ns:key=value` targets, so this is a lint, never a write refusal: the
// write is admitted and lint reports it, on open and Deferred rows, with
// the distinct keys (a key carried with and without a value is one key).
#[test]
fn test_lint_reports_a_task_carrying_more_than_one_area_or_repo() {
    let both = permits(
        &[("area", &["cli", "mcp"]), ("repo", &["annals", "tagma"])],
        &[],
        &[],
    );
    let mut service = under(
        Some(both),
        &["swift-amber", "calm-otter", "brisk-oak", "lone-elm"],
    );
    add(&mut service, &["area:cli", "area:mcp"]).unwrap();
    add(&mut service, &["repo:annals", "repo:tagma", "area:cli"]).unwrap();
    add(&mut service, &["area:cli", "area:cli=x", "repo:annals"]).unwrap();
    add(&mut service, &["area:cli", "area:mcp"]).unwrap();
    moved(&mut service, "calm-otter", "Deferred");
    moved(&mut service, "lone-elm", "Done");
    let found = lint_findings(&mut service);
    let many = |namespace: &str, keys: &[&str]| Finding::ManyKeys {
        namespace: namespace.to_string(),
        keys: keys.iter().map(|k| k.to_string()).collect(),
    };
    assert_eq!(
        found,
        [
            ("calm-otter".to_string(), many("repo", &["annals", "tagma"])),
            ("swift-amber".to_string(), many("area", &["cli", "mcp"])),
        ]
    );
    assert_eq!(found[0].1.code(), "tasks-application-lint-many-keys");
    assert_eq!(
        found[1].1.to_string(),
        "area: carries 2 keys (cli, mcp), but a task takes one"
    );
}

// PROPOSAL (opt-in, ruling 6): the one-area rule comes with the permits,
// so a project declaring none sees no new finding from it; its lint says
// what it said before permits existed. (Paired with the test above, which
// fails while the rule is not there at all.)
#[test]
fn test_lint_holds_a_task_to_one_area_only_where_the_project_declares_permits() {
    let mut service = under(None, &["swift-amber"]);
    add(&mut service, &["area:cli", "area:mcp", "repo:a", "repo:b"]).unwrap();
    assert_eq!(lint_findings(&mut service), []);
}
