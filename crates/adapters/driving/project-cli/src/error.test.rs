//! The errors' English, the wrapper's delegation, and the catalog.

use angzarr_diagnostic::{Arg, Diagnostic};
use project_application::ProjectError;

angzarr_presentation::english_for!(&[crate::EN_US.1]);

use super::{diagnostic_samples, CliError};

#[test]
fn test_the_wrapper_is_its_causes_diagnostic() {
    let cause = ProjectError::NotOrphan {
        id: "x".to_string(),
    };
    let wrapper = CliError::Project(cause.clone());
    assert_eq!(wrapper.code(), cause.code());
    assert_eq!(wrapper.to_string(), cause.to_string());
    assert_eq!(wrapper.args().len(), 1);
}

#[test]
fn test_output_renders_its_english_and_its_args() {
    let output = &diagnostic_samples()[0];
    assert_eq!(output.english(), "cannot write the output: broken pipe");
    assert!(matches!(&output.args()[..], [("ext_err_txt", Arg::Str(d))] if d == "broken pipe"));
}

#[test]
fn test_every_error_has_a_catalog_message_with_exactly_its_args() {
    use fluent_syntax::ast::{Entry, Expression, InlineExpression, PatternElement};

    let ftl = crate::EN_US.1;
    let resource = fluent_syntax::parser::parse(ftl).expect("the catalog parses");
    for sample in diagnostic_samples() {
        let message = resource
            .body
            .iter()
            .find_map(|e| match e {
                Entry::Message(m) if m.id.name == sample.code() => Some(m),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no message for {}", sample.code()));
        let used: Vec<&str> = message
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
        let args: Vec<&str> = sample.args().into_iter().map(|(n, _)| n).collect();
        assert_eq!(used, args, "{}", sample.code());
    }
    let messages = resource
        .body
        .iter()
        .filter(|e| matches!(e, Entry::Message(_)))
        .count();
    assert_eq!(messages, diagnostic_samples().len());
}

#[test]
fn test_each_diagnostic_renders_its_catalog_message_and_displays_as_its_key() {
    let catalog = angzarr_presentation::Catalog::en_us(&[project_application::EN_US, crate::EN_US])
        .expect("the catalog loads");
    let samples = diagnostic_samples()
        .into_iter()
        .chain(project_application::diagnostic_samples());
    for sample in samples {
        // Display is the key and args, never English (decision 14.37).
        assert_eq!(
            sample.to_string(),
            angzarr_diagnostic::Report::of(&*sample).to_string(),
            "{}",
            sample.code()
        );
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
        angzarr_diagnostic::catalog::check(name, text, &crate::diagnostic_samples()),
        []
    );
}
