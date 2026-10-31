//! The catalog: every rejection has an en-US message using exactly its
//! args, and every message a sample.

use fluent_syntax::ast::{Entry, Expression, InlineExpression, PatternElement};

#[test]
fn every_rejection_has_a_catalog_message_with_exactly_its_args() {
    let resource = fluent_syntax::parser::parse(crate::EN_US.1).expect("the catalog parses");
    let samples = crate::diagnostic_samples();
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
        assert_eq!(
            sample.to_string(),
            angzarr_embedded_diagnostic::Report::of(&*sample).to_string(),
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
        let mut args: Vec<&str> = sample.args().iter().map(|(n, _)| *n).collect();
        args.sort_unstable();
        assert_eq!(used, args, "{}", sample.code());
    }
}

#[test]
fn every_code_is_prefixed_with_the_crates_name() {
    for sample in crate::diagnostic_samples() {
        assert!(sample.code().starts_with("plans-cli-"), "{}", sample.code());
    }
}

#[test]
fn a_usage_error_exits_2_and_any_other_1() {
    use crate::CliError;
    use plans_application::PlanError;

    assert_eq!(CliError::NoLink { command: "unlink" }.exit_code(), 2);
    assert_eq!(
        CliError::UnknownStatus { status: "x".into() }.exit_code(),
        2
    );
    assert_eq!(
        CliError::Plan(PlanError::NoSuchPlan("p".into())).exit_code(),
        1
    );
    assert_eq!(CliError::Output { detail: "x".into() }.exit_code(), 1);
}
