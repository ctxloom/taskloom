//! The catalog: every rejection has an en-US message using exactly its
//! args, and every message a sample.

use angzarr_diagnostic::code;
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
            angzarr_diagnostic::Report::of(&*sample).to_string(),
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
        // A message may name an arg more than once.
        used.dedup();
        let mut args: Vec<&str> = sample.args().iter().map(|(n, _)| *n).collect();
        args.sort_unstable();
        assert_eq!(used, args, "{}", sample.code());
    }
}

#[test]
fn every_code_is_prefixed_with_the_crates_name() {
    for sample in crate::diagnostic_samples() {
        assert!(sample.code().starts_with(code!("")), "{}", sample.code());
    }
}

#[test]
fn a_wrapper_is_its_causes_diagnostic() {
    use angzarr_diagnostic::Diagnostic;

    use crate::{PathError, PlanError, PlanRejection};

    let rejection = PlanRejection::NotGoverned("t".into());
    let rejected = PlanError::Rejected(rejection.clone());
    assert_eq!(rejected.code(), rejection.code());
    assert_eq!(rejected.to_string(), rejection.to_string());
    let path = PlanError::Path(PathError::NotAPlanFile("x.md".into()));
    assert_eq!(
        path.to_string(),
        concat!(code!("not-a-plan-file"), " path=x.md")
    );
    let report = angzarr_diagnostic::Report::new("x-busy");
    for wrapper in [
        PlanError::NoWorkRoot(report.clone()),
        PlanError::Identity(report.clone()),
        PlanError::Files(report.clone()),
        PlanError::Tasks(report.clone()),
        PlanError::Failed(report),
    ] {
        assert_eq!(wrapper.code(), "x-busy");
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
