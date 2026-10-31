//! RED: the plan aggregate's rules, decide and apply, one stream at a time.
//! Every test fails on a `todo!()` until the aggregate is written.

use super::*;

use PlanStatus::{Abandoned, Active, Done, Draft, Superseded};

const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const HASH2: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn add() -> AddPlan {
    AddPlan {
        harp_id: "brisk-cedar-otter".to_string(),
        title: "Plans move into taskloom".to_string(),
        status: Draft,
        path: ".taskloom/plans/brisk-cedar-otter.plan.md".to_string(),
        content_hash: HASH.to_string(),
        origin_session: "inept-sixth-stick".to_string(),
    }
}

/// Run `command` on `state`, folding what it records into it.
fn run(state: &mut PlanState, command: PlanCommand) -> Result<Vec<PlanEvent>, PlanRejection> {
    let events = decide(state, &command)?;
    for event in &events {
        apply(state, event);
    }
    Ok(events)
}

/// A plan added, in `status` (reached by the allowed path).
fn plan_in(status: PlanStatus) -> PlanState {
    let mut state = PlanState::default();
    run(&mut state, PlanCommand::Add(add())).unwrap();
    match status {
        Draft => {}
        Active => {
            run(&mut state, PlanCommand::SetStatus(Active)).unwrap();
        }
        Superseded => {
            run(&mut state, PlanCommand::SetStatus(Active)).unwrap();
            run(&mut state, PlanCommand::MarkSuperseded("next-plan".into())).unwrap();
        }
        Done | Abandoned => {
            run(&mut state, PlanCommand::SetStatus(Active)).unwrap();
            run(&mut state, PlanCommand::SetStatus(status)).unwrap();
        }
    }
    state
}

// ---- add ----

#[test]
fn add_records_the_plan_whole_and_folds_it() {
    let mut state = PlanState::default();
    let events = run(&mut state, PlanCommand::Add(add())).unwrap();
    assert_eq!(events, vec![PlanEvent::PlanAdded(add())]);
    assert!(state.added);
    assert_eq!(state.harp_id, "brisk-cedar-otter");
    assert_eq!(state.title, "Plans move into taskloom");
    assert_eq!(state.status, Draft);
    assert_eq!(state.path, ".taskloom/plans/brisk-cedar-otter.plan.md");
    assert_eq!(state.content_hash, HASH);
    assert_eq!(state.origin_session, "inept-sixth-stick");
    assert!(state.governs.is_empty() && state.supersedes.is_empty());
}

#[test]
fn add_may_start_active() {
    let mut state = PlanState::default();
    let mut active = add();
    active.status = Active;
    run(&mut state, PlanCommand::Add(active)).unwrap();
    assert_eq!(state.status, Active);
}

#[test]
fn add_refuses_to_start_in_any_status_but_draft_or_active() {
    for status in [Superseded, Done, Abandoned] {
        let mut asked = add();
        asked.status = status;
        assert_eq!(
            decide(&PlanState::default(), &PlanCommand::Add(asked)),
            Err(PlanRejection::InitialStatus(status)),
        );
    }
}

#[test]
fn add_refuses_a_blank_title() {
    let mut blank = add();
    blank.title = "  ".to_string();
    assert_eq!(
        decide(&PlanState::default(), &PlanCommand::Add(blank)),
        Err(PlanRejection::EmptyTitle),
    );
}

#[test]
fn add_refuses_a_hash_that_is_not_sha256_hex() {
    let mut bad = add();
    bad.content_hash = "abc".to_string();
    assert_eq!(
        decide(&PlanState::default(), &PlanCommand::Add(bad)),
        Err(PlanRejection::BadHash("abc".to_string())),
    );
}

#[test]
fn add_to_a_stream_holding_a_plan_is_refused() {
    let state = plan_in(Draft);
    assert_eq!(
        decide(&state, &PlanCommand::Add(add())),
        Err(PlanRejection::AlreadyAdded)
    );
}

#[test]
fn every_command_but_add_on_an_empty_stream_is_no_such_plan() {
    let empty = PlanState::default();
    for command in [
        PlanCommand::SetStatus(Active),
        PlanCommand::Govern("t".into()),
        PlanCommand::Ungovern("t".into()),
        PlanCommand::Supersede("p".into()),
        PlanCommand::Unsupersede("p".into()),
        PlanCommand::MarkSuperseded("p".into()),
        PlanCommand::Reinstate,
        PlanCommand::Rehash(HASH2.into()),
    ] {
        assert_eq!(
            decide(&empty, &command),
            Err(PlanRejection::NoSuchPlan),
            "{command:?}"
        );
    }
}

// ---- status ----

#[test]
fn terminal_statuses_are_superseded_done_and_abandoned() {
    let terminal: Vec<_> = PlanStatus::ALL
        .into_iter()
        .filter(|s| s.terminal())
        .collect();
    assert_eq!(terminal, vec![Superseded, Done, Abandoned]);
}

#[test]
fn status_names_round_trip_lowercase_and_parse_any_case() {
    for status in PlanStatus::ALL {
        assert_eq!(PlanStatus::parse(status.name()), Some(status));
        assert_eq!(
            PlanStatus::parse(&status.name().to_uppercase()),
            Some(status)
        );
    }
    assert_eq!(Active.name(), "active");
    assert_eq!(PlanStatus::parse("in progress"), None);
}

/// The proposed transition table (FLAGGED for review): forward from Draft to
/// Active, Active to Done or Abandoned, Draft straight to Abandoned; Done and
/// Abandoned reopen to Active; nothing returns to Draft; Superseded is set
/// and left only by a supersede link.
#[test]
fn plan_status_allows_exactly_the_proposed_transitions() {
    let allowed = [
        (Draft, Active),
        (Draft, Abandoned),
        (Active, Done),
        (Active, Abandoned),
        (Done, Active),
        (Abandoned, Active),
    ];
    for from in PlanStatus::ALL {
        for to in PlanStatus::ALL {
            let expected = allowed.contains(&(from, to));
            assert_eq!(from.may_become(to), expected, "{from:?} -> {to:?}");
        }
    }
}

#[test]
fn set_status_records_from_and_to() {
    let mut state = plan_in(Draft);
    let events = run(&mut state, PlanCommand::SetStatus(Active)).unwrap();
    assert_eq!(
        events,
        vec![PlanEvent::StatusChanged {
            from: Draft,
            to: Active
        }]
    );
    assert_eq!(state.status, Active);
}

#[test]
fn set_status_refuses_a_transition_the_table_does_not_allow() {
    let state = plan_in(Draft);
    assert_eq!(
        decide(&state, &PlanCommand::SetStatus(Done)),
        Err(PlanRejection::Transition {
            from: Draft,
            to: Done
        }),
    );
    let state = plan_in(Superseded);
    assert_eq!(
        decide(&state, &PlanCommand::SetStatus(Active)),
        Err(PlanRejection::Transition {
            from: Superseded,
            to: Active
        }),
    );
}

#[test]
fn set_status_to_the_status_held_is_refused_as_unchanged() {
    let state = plan_in(Active);
    assert_eq!(
        decide(&state, &PlanCommand::SetStatus(Active)),
        Err(PlanRejection::StatusUnchanged(Active)),
    );
}

#[test]
fn set_status_superseded_is_refused_naming_the_supersede_link() {
    let state = plan_in(Active);
    assert_eq!(
        decide(&state, &PlanCommand::SetStatus(Superseded)),
        Err(PlanRejection::SupersededBySupersedeOnly),
    );
}

#[test]
fn mark_superseded_records_the_successor_and_reinstate_returns_to_active() {
    let mut state = plan_in(Active);
    let events = run(&mut state, PlanCommand::MarkSuperseded("next-plan".into())).unwrap();
    assert_eq!(
        events,
        vec![PlanEvent::MarkedSuperseded {
            by: "next-plan".into(),
            from: Active
        }]
    );
    assert_eq!(state.status, Superseded);
    assert_eq!(state.superseded_by, "next-plan");

    run(&mut state, PlanCommand::Reinstate).unwrap();
    assert_eq!(state.status, Active);
    assert_eq!(state.superseded_by, "");
}

#[test]
fn reinstate_is_refused_unless_the_plan_is_superseded() {
    let state = plan_in(Active);
    assert_eq!(
        decide(&state, &PlanCommand::Reinstate),
        Err(PlanRejection::Transition {
            from: Active,
            to: Active
        }),
    );
}

// ---- governs ----

#[test]
fn govern_adds_a_task_and_ungovern_removes_it() {
    let mut state = plan_in(Active);
    let events = run(&mut state, PlanCommand::Govern("swift-amber-falcon".into())).unwrap();
    assert_eq!(
        events,
        vec![PlanEvent::Governed("swift-amber-falcon".into())]
    );
    run(&mut state, PlanCommand::Govern("calm-dusty-heron".into())).unwrap();
    assert_eq!(
        state.governs.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["calm-dusty-heron", "swift-amber-falcon"],
    );

    let events = run(
        &mut state,
        PlanCommand::Ungovern("swift-amber-falcon".into()),
    )
    .unwrap();
    assert_eq!(
        events,
        vec![PlanEvent::Ungoverned("swift-amber-falcon".into())]
    );
    assert_eq!(state.governs.len(), 1);
}

#[test]
fn govern_twice_is_refused_and_ungoverning_a_task_not_governed_is_refused() {
    let mut state = plan_in(Active);
    run(&mut state, PlanCommand::Govern("swift-amber-falcon".into())).unwrap();
    assert_eq!(
        decide(&state, &PlanCommand::Govern("swift-amber-falcon".into())),
        Err(PlanRejection::AlreadyGoverns("swift-amber-falcon".into())),
    );
    assert_eq!(
        decide(&state, &PlanCommand::Ungovern("calm-dusty-heron".into())),
        Err(PlanRejection::NotGoverned("calm-dusty-heron".into())),
    );
}

// ---- supersedes ----

#[test]
fn supersede_links_a_plan_and_unsupersede_unlinks_it() {
    let mut state = plan_in(Active);
    let events = run(&mut state, PlanCommand::Supersede("old-plan".into())).unwrap();
    assert_eq!(events, vec![PlanEvent::Superseded("old-plan".into())]);
    assert!(state.supersedes.contains("old-plan"));
    let events = run(&mut state, PlanCommand::Unsupersede("old-plan".into())).unwrap();
    assert_eq!(events, vec![PlanEvent::Unsuperseded("old-plan".into())]);
    assert!(state.supersedes.is_empty());
}

#[test]
fn a_plan_superseding_itself_is_refused() {
    let state = plan_in(Active);
    assert_eq!(
        decide(&state, &PlanCommand::Supersede("brisk-cedar-otter".into())),
        Err(PlanRejection::SupersedesItself),
    );
}

#[test]
fn supersede_twice_and_unsupersede_of_an_unlinked_plan_are_refused() {
    let mut state = plan_in(Active);
    run(&mut state, PlanCommand::Supersede("old-plan".into())).unwrap();
    assert_eq!(
        decide(&state, &PlanCommand::Supersede("old-plan".into())),
        Err(PlanRejection::AlreadySupersedes("old-plan".into())),
    );
    assert_eq!(
        decide(&state, &PlanCommand::Unsupersede("other".into())),
        Err(PlanRejection::NotSuperseded("other".into())),
    );
}

// ---- hash ----

#[test]
fn rehash_records_the_new_hash() {
    let mut state = plan_in(Active);
    let events = run(&mut state, PlanCommand::Rehash(HASH2.into())).unwrap();
    assert_eq!(events, vec![PlanEvent::Rehashed(HASH2.into())]);
    assert_eq!(state.content_hash, HASH2);
}

#[test]
fn rehash_to_the_recorded_hash_is_refused() {
    let state = plan_in(Active);
    assert_eq!(
        decide(&state, &PlanCommand::Rehash(HASH.into())),
        Err(PlanRejection::HashUnchanged),
    );
}
