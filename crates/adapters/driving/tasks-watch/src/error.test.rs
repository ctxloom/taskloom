//! The errors' English, the wrapper's delegation, and the catalog.

use cli_support::ExitClass;
use coded_diagnostic::Diagnostic;
use tasks_application::AppError;

presentation::english_for!(&[crate::EN_US.1]);

use super::{diagnostic_samples, WatchError};

fn arg_names(d: &dyn Diagnostic) -> Vec<&str> {
    d.args().into_iter().map(|(name, _)| name).collect()
}

#[test]
fn test_a_use_case_error_is_its_own_diagnostic() {
    let app = AppError::HarpsExhausted { attempts: 100 };
    let error = WatchError::App(app.clone());
    assert_eq!(error.code(), app.code());
    assert_eq!(error.to_string(), app.to_string());
    assert_eq!(arg_names(&error), ["attempts"]);
}

#[test]
fn test_no_project_is_gos_message() {
    assert_eq!(
        WatchError::NoProject.english(),
        "no project here yet; add a task first (taskloom tasks add \"<task text>\")"
    );
}

#[test]
fn test_every_error_has_a_catalog_message_with_exactly_its_args() {
    use fluent_syntax::ast::{Entry, Expression, InlineExpression, PatternElement};

    let resource = fluent_syntax::parser::parse(crate::EN_US.1).expect("the catalog parses");
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
        let mut used: Vec<&str> = message
            .value
            .iter()
            .flat_map(|p| &p.elements)
            .filter_map(|el| match el {
                PatternElement::Placeable {
                    expression: Expression::Inline(InlineExpression::VariableReference { id }),
                } => Some(id.name),
                _ => None,
            })
            .collect();
        used.sort_unstable();
        let mut args = arg_names(&*sample);
        args.sort_unstable();
        assert_eq!(used, args, "{}", sample.code());
    }
}

#[test]
fn test_each_diagnostic_renders_its_catalog_message_and_displays_as_its_key() {
    let catalog = presentation::Catalog::en_us(&[crate::EN_US]).expect("the catalog loads");
    for sample in diagnostic_samples() {
        let message = catalog.render(&*sample).expect("a catalog message");
        assert!(!message.is_empty(), "{}", sample.code());
        assert!(
            sample.to_string().starts_with(sample.code()),
            "Display is the key and args, never English: {sample}"
        );
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

#[test]
fn test_a_request_the_content_or_the_state_forbids_is_declined_and_what_broke_is_a_failure() {
    let invalid = [
        WatchError::CursorMalformed,
        WatchError::CursorVersion {
            found: 2,
            supported: 1,
        },
        WatchError::CursorForeign {
            cursor_project: "a".to_string(),
            project: "b".to_string(),
        },
    ];
    for error in invalid {
        assert_eq!(error.exit().code(false), 48, "{error}");
    }
    let forbidden = [
        WatchError::NoProject,
        WatchError::UnknownProject {
            project: "p".to_string(),
        },
    ];
    for error in forbidden {
        assert_eq!(error.exit().code(false), 48, "{error}");
    }
    let broke = [
        WatchError::Output {
            detail: "gone".to_string(),
        },
        WatchError::App(AppError::HarpsExhausted { attempts: 1 }),
    ];
    for error in broke {
        assert_eq!(error.exit().code(false), 32, "{error}");
    }
}

#[test]
fn test_the_cursor_errors_say_what_to_do_and_name_the_versions() {
    assert_eq!(
        WatchError::CursorVersion {
            found: 999,
            supported: 1
        }
        .english(),
        "the cursor is version 999; this taskloom reads version 1"
    );
    assert_eq!(
        WatchError::CursorForeign {
            cursor_project: "jolly-mute-magma".to_string(),
            project: "swift-amber-falcon".to_string()
        }
        .english(),
        "the cursor belongs to project jolly-mute-magma, not to swift-amber-falcon"
    );
    assert_eq!(
        WatchError::CursorMalformed.english(),
        "--since is not a cursor: pass the cursor of a watch line, unchanged"
    );
    assert_eq!(
        WatchError::UnknownProject {
            project: "no-such".to_string()
        }
        .english(),
        "no project no-such has a store here"
    );
}
