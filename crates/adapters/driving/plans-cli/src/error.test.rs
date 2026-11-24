//! The catalog: every rejection has an en-US message using exactly its
//! args, and every message a sample.

use coded_diagnostic::code;
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
fn a_request_the_content_or_the_state_forbids_exits_48_and_what_broke_32() {
    use crate::CliError;
    use cli_support::ExitClass;
    use plans_application::PlanError;

    let code = |error: CliError| error.exit().code(false);
    assert_eq!(code(CliError::NoLink { command: "unlink" }), 48);
    assert_eq!(
        code(CliError::Plan(PlanError::UnknownStatus {
            status: "x".into()
        })),
        48
    );
    assert_eq!(code(CliError::Plan(PlanError::NoChange)), 48);
    assert_eq!(code(CliError::Plan(PlanError::NoSuchPlan("p".into()))), 48);
    assert_eq!(
        code(CliError::Plan(PlanError::Unreadable { path: "p".into() })),
        32
    );
    assert_eq!(code(CliError::Output { detail: "x".into() }), 32);
    assert_eq!(code(CliError::HookNoFile), 48);
    assert_eq!(
        code(CliError::WatchFailed {
            error: PlanError::Unreadable { path: "p".into() }
        }),
        32
    );
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
