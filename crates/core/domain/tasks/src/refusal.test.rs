//! A rejection travels as a typed refusal message of the protos, and comes
//! back as the `TaskRejection` (or `SchemaRejection`) whose code, args and
//! English a person reads. The mapping is by the refusal's TYPE; the
//! business status a refusal is refused under is the proto's option, read
//! from the generated `REFUSAL_STATUS` and nowhere else.

use angzarr_embedded::markup::{Refusing, TypedRefusal};

use crate::schema::{SchemaAggregate, SchemaRejection};
use crate::{
    AddTask, Blank, Declare, EditText, HarpIdRequired, NoTask, RemoveTask, Removed, Retag,
    SchemaNotConfig, SchemaTagRequired, SetStatus, StatusRequired, StatusReserved, TagRequired,
    TaskAggregate, TaskExists, TaskRejection, TextRequired, TriggerRequired, TriggerWithStatus,
    Unresolved,
};

/// Every task rejection beside the typed refusal it travels as.
fn every_task_refusal() -> Vec<(TaskRejection, TypedRefusal)> {
    let harp = || "swift-amber".to_string();
    let text = |s: &str| s.to_string();
    vec![
        (
            TaskRejection::HarpIdRequired,
            TypedRefusal::of(&HarpIdRequired {}),
        ),
        (
            TaskRejection::TaskExists { harp_id: harp() },
            TypedRefusal::of(&TaskExists { harp_id: harp() }),
        ),
        (TaskRejection::NoTask, TypedRefusal::of(&NoTask {})),
        (
            TaskRejection::Removed { harp_id: harp() },
            TypedRefusal::of(&Removed { harp_id: harp() }),
        ),
        (
            TaskRejection::TextRequired,
            TypedRefusal::of(&TextRequired {}),
        ),
        (
            TaskRejection::StatusRequired,
            TypedRefusal::of(&StatusRequired {}),
        ),
        (
            TaskRejection::StatusReserved {
                status: text("@open"),
            },
            TypedRefusal::of(&StatusReserved {
                status: text("@open"),
            }),
        ),
        (
            TaskRejection::TriggerRequired,
            TypedRefusal::of(&TriggerRequired {}),
        ),
        (
            TaskRejection::TagRequired,
            TypedRefusal::of(&TagRequired {}),
        ),
        (
            TaskRejection::Blank {
                field: text("trigger"),
            },
            TypedRefusal::of(&Blank {
                field: text("trigger"),
            }),
        ),
        (
            TaskRejection::TriggerWithStatus {
                status: text("Done"),
                defers: text("Deferred"),
            },
            TypedRefusal::of(&TriggerWithStatus {
                status: text("Done"),
                defers: text("Deferred"),
            }),
        ),
        (
            TaskRejection::Unresolved {
                field: text("defers"),
            },
            TypedRefusal::of(&Unresolved {
                field: text("defers"),
            }),
        ),
    ]
}

#[test]
fn test_every_typed_refusal_of_a_task_comes_back_as_its_rejection_whole() {
    for (rejection, refusal) in every_task_refusal() {
        assert_eq!(
            TaskRejection::try_from(&refusal),
            Ok(rejection.clone()),
            "{rejection:?}"
        );
    }
}

#[test]
fn test_every_typed_refusal_of_a_declare_comes_back_as_its_rejection_whole() {
    let tag = || "area:cli".to_string();
    assert_eq!(
        SchemaRejection::try_from(&TypedRefusal::of(&SchemaTagRequired {})),
        Ok(SchemaRejection::TagRequired)
    );
    assert_eq!(
        SchemaRejection::try_from(&TypedRefusal::of(&SchemaNotConfig { tag: tag() })),
        Ok(SchemaRejection::NotConfig { tag: tag() })
    );
}

#[test]
fn test_a_refusal_of_another_domain_or_type_is_no_rejection_of_ours() {
    let schema = TypedRefusal::of(&SchemaTagRequired {});
    assert!(TaskRejection::try_from(&schema).is_err());
    let task = TypedRefusal::of(&NoTask {});
    assert!(SchemaRejection::try_from(&task).is_err());
    let foreign = TypedRefusal::of(&RemoveTask {
        reason: crate::RemovalReason::Removed,
    });
    assert!(TaskRejection::try_from(&foreign).is_err());
    assert!(SchemaRejection::try_from(&foreign).is_err());
}

/// Whether the refusals `command` lists include the one `rejection`
/// travels as.
fn lists<C>(rejection: &TaskRejection) -> bool
where
    TaskAggregate: Refusing<C>,
    C: angzarr_embedded::markup::DomainMessage,
{
    let refusal = every_task_refusal()
        .into_iter()
        .find(|(held, _)| std::mem::discriminant(held) == std::mem::discriminant(rejection))
        .map(|(_, refusal)| refusal)
        .expect("a typed refusal for every rejection");
    let travelled = refusal.packed().type_url.clone();
    <TaskAggregate as Refusing<C>>::REFUSES
        .iter()
        .any(|listed| {
            travelled.ends_with(&format!("/{}", listed.full_name()))
                || travelled == listed.full_name()
        })
}

/// The refusals each command lists are the ones its rules raise: what the
/// aggregate's own tests drive through the kernel, listed against the
/// protos' `refuses`.
#[test]
fn test_each_command_lists_the_refusals_its_rules_raise() {
    let blank = || TaskRejection::Blank {
        field: "status".to_string(),
    };
    let reserved = || TaskRejection::StatusReserved {
        status: "@open".to_string(),
    };
    let with_status = || TaskRejection::TriggerWithStatus {
        status: "Done".to_string(),
        defers: "Deferred".to_string(),
    };
    let unresolved = || TaskRejection::Unresolved {
        field: "defers".to_string(),
    };
    let removed = || TaskRejection::Removed {
        harp_id: "swift-amber".to_string(),
    };
    let exists = || TaskRejection::TaskExists {
        harp_id: "swift-amber".to_string(),
    };
    for raised in [
        TaskRejection::HarpIdRequired,
        exists(),
        TaskRejection::TextRequired,
        unresolved(),
        blank(),
        reserved(),
        with_status(),
        TaskRejection::TriggerRequired,
    ] {
        assert!(lists::<AddTask>(&raised), "AddTask raises {raised:?}");
    }
    for raised in [
        TaskRejection::NoTask,
        removed(),
        unresolved(),
        blank(),
        reserved(),
        TaskRejection::StatusRequired,
        with_status(),
        TaskRejection::TriggerRequired,
    ] {
        assert!(lists::<SetStatus>(&raised), "SetStatus raises {raised:?}");
    }
    for raised in [
        TaskRejection::NoTask,
        removed(),
        TaskRejection::TextRequired,
    ] {
        assert!(lists::<EditText>(&raised), "EditText raises {raised:?}");
    }
    for raised in [TaskRejection::NoTask, removed(), TaskRejection::TagRequired] {
        assert!(lists::<Retag>(&raised), "Retag raises {raised:?}");
    }
    for raised in [TaskRejection::NoTask, removed()] {
        assert!(lists::<RemoveTask>(&raised), "RemoveTask raises {raised:?}");
    }
    assert!(!lists::<RemoveTask>(&TaskRejection::TextRequired));
    assert_eq!(
        <SchemaAggregate as Refusing<Declare>>::REFUSES.len(),
        2,
        "a declare lists its two"
    );
}

/// The status is the proto's alone: no hand-written source of the domain
/// spells a business status but `refusal.rs`, the one place that reads the
/// generated constant.
#[test]
fn test_no_other_hand_written_source_of_the_domain_names_a_status() {
    let sources = [
        ("aggregate.rs", include_str!("aggregate.rs")),
        ("schema.rs", include_str!("schema.rs")),
        ("lib.rs", include_str!("lib.rs")),
        ("rejection.rs", include_str!("rejection.rs")),
    ];
    for (name, source) in sources {
        for forbidden in ["BusinessStatus", "REFUSAL_STATUS", "with_status"] {
            assert!(
                !source.contains(forbidden),
                "{name} spells {forbidden}: a status is the proto's option alone"
            );
        }
    }
}
