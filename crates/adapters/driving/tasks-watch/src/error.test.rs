//! The errors' English, the wrapper's delegation, and the catalog.

use angzarr_embedded_diagnostic::Diagnostic;
use tasks_application::AppError;

crate::english_for!(&[crate::EN_US.1]);

use super::{diagnostic_samples, WatchError};

fn arg_names(d: &dyn Diagnostic) -> Vec<&'static str> {
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
        "no project here yet; add a task first (taskloom add \"<task text>\")"
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
