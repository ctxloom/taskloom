//! The errors' English, their wrappers' delegation, and the catalog.

use crate::english::English;
use coded_diagnostic::{Arg, Diagnostic, Report};

use super::{diagnostic_samples, AppError};

fn cause() -> Report {
    Report::new(tasks::TaskRejection::TextRequired.code().to_string()).arg("field", "text")
}

fn arg_names(d: &dyn Diagnostic) -> Vec<&str> {
    d.args().into_iter().map(|(name, _)| name).collect()
}

#[test]
fn test_a_wrapper_is_its_causes_diagnostic() {
    for wrapper in [
        AppError::Rejected(cause()),
        AppError::TagRefused(cause()),
        AppError::Failed(cause()),
        AppError::Identity(cause()),
        AppError::Configuration(cause()),
        AppError::TagQuery(cause()),
    ] {
        assert_eq!(wrapper.code(), tasks::TaskRejection::TextRequired.code());
        assert_eq!(wrapper.english(), "a task needs text");
        assert_eq!(arg_names(&wrapper), ["field"]);
    }
}

#[test]
fn test_each_error_renders_its_english_fallback() {
    assert_eq!(
        AppError::GaveUp {
            attempts: 10,
            error: Report::new("x-busy"),
        }
        .english(),
        "gave up after 10 attempts: x-busy"
    );
    assert_eq!(
        AppError::HarpsExhausted { attempts: 100 }.english(),
        "no free task id after 100 tries"
    );
    assert_eq!(
        AppError::Vanished {
            harp_id: "swift-amber".to_string()
        }
        .english(),
        "task swift-amber was written but cannot be read back"
    );
    assert_eq!(
        AppError::NoProject.english(),
        "there is no project here yet, so no task to change"
    );
}

#[test]
fn test_each_error_gives_its_args_values() {
    let gave_up = AppError::GaveUp {
        attempts: 7,
        error: cause(),
    };
    let args = gave_up.args();
    assert!(matches!(args[0], ("attempts", Arg::Uint(7))));
    assert!(
        matches!(&args[1], ("error", Arg::Nested(e)) if e.code() == tasks::TaskRejection::TextRequired.code())
    );
    let exhausted = AppError::HarpsExhausted { attempts: 100 }.args();
    assert!(matches!(exhausted[0], ("attempts", Arg::Uint(100))));
    let vanished = AppError::Vanished {
        harp_id: "calm-otter".to_string(),
    };
    let vanished = vanished.args();
    assert!(matches!(&vanished[0], ("harp_id", Arg::Str(s)) if s == "calm-otter"));
    assert!(AppError::NoProject.args().is_empty());
}

#[test]
fn test_every_error_has_a_catalog_message_with_exactly_its_args() {
    use fluent_syntax::ast::Entry;

    let ftl = crate::EN_US.1;
    let resource = fluent_syntax::parser::parse(ftl).expect("the catalog parses");
    let samples = diagnostic_samples();
    let mut catalog: Vec<&str> = resource
        .body
        .iter()
        .filter_map(|e| match e {
            Entry::Message(m) => Some(m.id.name),
            _ => None,
        })
        .collect();
    catalog.sort_unstable();
    let mut sampled: Vec<&str> = samples.iter().map(|s| s.code()).collect();
    sampled.sort_unstable();
    assert_eq!(catalog, sampled, "catalog messages and samples");
    for sample in samples {
        // Display is the key and args, never English (decision 14.37).
        assert_eq!(
            sample.to_string(),
            coded_diagnostic::Report::of(&*sample).to_string(),
            "{}",
            sample.code()
        );
        let message = resource
            .body
            .iter()
            .find_map(|e| match e {
                Entry::Message(m) if m.id.name == sample.code() => Some(m),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no message for {}", sample.code()));
        let mut used: Vec<&str> = Vec::new();
        if let Some(pattern) = &message.value {
            variables(pattern, &mut used);
        }
        used.sort_unstable();
        used.dedup();
        let mut args = arg_names(&*sample);
        args.sort_unstable();
        assert_eq!(used, args, "{}", sample.code());
    }
}

/// The variables `pattern` refers to: in its placeables, a selector, and
/// every variant a selector picks.
fn variables<'a>(pattern: &fluent_syntax::ast::Pattern<&'a str>, used: &mut Vec<&'a str>) {
    use fluent_syntax::ast::{Expression, InlineExpression, PatternElement};

    for element in &pattern.elements {
        let PatternElement::Placeable { expression } = element else {
            continue;
        };
        match expression {
            Expression::Inline(InlineExpression::VariableReference { id }) => used.push(id.name),
            Expression::Select { selector, variants } => {
                if let InlineExpression::VariableReference { id } = selector {
                    used.push(id.name);
                }
                for variant in variants {
                    variables(&variant.value, used);
                }
            }
            Expression::Inline(_) => {}
        }
    }
}

/// The catalog is exported under the crate's package name, and every code
/// is under that name, with its message using exactly its args.
#[test]
fn test_every_code_is_under_the_crates_name_with_its_message() {
    let (name, text) = crate::EN_US;
    assert_eq!(name, env!("CARGO_PKG_NAME"));
    assert_eq!(
        coded_diagnostic::catalog::check(name, text, &crate::diagnostic_samples()),
        []
    );
}

/// Whether an error is the domain's refusal of a task that is not there,
/// as the edge asks it of a status change: the refusal itself, or carrying
/// dead letters, and nothing else.
#[test]
fn test_no_task_is_the_domains_refusal_of_a_missing_task_and_nothing_else() {
    let no_task = crate::no_task_sample();
    assert!(no_task.is_no_task());
    let lettered = AppError::DeadLettered {
        error: Box::new(no_task),
        dead_letters: vec![crate::dead_letter_sample("status")],
    };
    assert!(lettered.is_no_task());
    let refused = AppError::Rejected(Report::of(&tasks::TaskRejection::TextRequired));
    assert!(!refused.is_no_task());
    assert!(!AppError::NoProject.is_no_task());
}

/// A refusal's business status is the protos' option alone, read where the
/// refusal is made: how a refusal reaches a person (its code, args and
/// English, its exit code) is keyed by its type, so a status edited in a
/// proto cannot change it.
#[test]
fn test_no_application_source_reads_a_refusals_status() {
    let sources = [
        ("error.rs", include_str!("error.rs")),
        ("add.rs", include_str!("add.rs")),
        ("prune.rs", include_str!("prune.rs")),
        ("import.rs", include_str!("import.rs")),
        ("project.rs", include_str!("project.rs")),
        ("schema.rs", include_str!("schema.rs")),
        ("write.rs", include_str!("write.rs")),
    ];
    for (name, source) in sources {
        for forbidden in [
            "BusinessStatus",
            "REFUSAL_STATUS",
            "with_status",
            ".status()",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} reads {forbidden}: a status is the proto's option alone"
            );
        }
    }
}

/// The class of a failure, as the published status of a plain command.
fn status(error: &AppError) -> u8 {
    use exit_code::ExitClass;
    error.exit().code(false)
}

#[test]
fn test_a_request_the_content_or_the_state_forbids_is_declined_with_48() {
    let declined = [
        AppError::Rejected(cause()),
        AppError::TagRefused(cause()),
        AppError::Configuration(cause()),
        AppError::TagQuery(cause()),
        AppError::Prune(cause()),
        AppError::Priority(cause()),
        AppError::NoProject,
        AppError::UnknownStatusClass {
            status: "@x".into(),
        },
        AppError::UnknownSort { sort: "x".into() },
        AppError::Blank { field: "text" },
        AppError::TriggerAndClear,
        AppError::AddRequestIdCollides {
            request_id: "r".into(),
            probes: 1,
        },
        AppError::NotFound {
            harp_ids: vec!["a".into()],
        },
        AppError::Ambiguous {
            held: vec![("a".into(), vec!["p".into(), "q".into()])],
        },
        AppError::PermitMalformed { permit: "x".into() },
        AppError::PermitNotDeclared { permit: "x".into() },
        AppError::PermitInUse {
            permit: "x".into(),
            harp_ids: vec!["a".into()],
        },
        AppError::SchemaInitConflict {
            template: "general".into(),
            held: String::new(),
        },
    ];
    for error in declined {
        assert_eq!(status(&error), 48, "{error:?}");
    }
}

#[test]
fn test_what_the_store_or_the_kernel_broke_is_a_failure_with_32() {
    let failed = [
        AppError::Failed(cause()),
        AppError::Identity(cause()),
        AppError::GaveUp {
            attempts: 3,
            error: cause(),
        },
        AppError::HarpsExhausted { attempts: 3 },
        AppError::Vanished {
            harp_id: "a".into(),
        },
    ];
    for error in failed {
        assert_eq!(status(&error), 32, "{error:?}");
    }
}

#[test]
fn test_dead_letters_do_not_change_the_class_of_the_failure_they_ride_on() {
    for (inner, expected) in [
        (AppError::Failed(cause()), 32),
        (AppError::Blank { field: "text" }, 48),
    ] {
        let wrapped = AppError::DeadLettered {
            error: Box::new(inner),
            dead_letters: Vec::new(),
        };
        assert_eq!(status(&wrapped), expected, "{wrapped:?}");
    }
}
