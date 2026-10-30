//! The errors' English and the catalog.

use angzarr_embedded_diagnostic::Diagnostic;

crate::english_for!(&[crate::EN_US.1]);

use super::{diagnostic_samples, ConfigError};

fn arg_names(d: &dyn Diagnostic) -> Vec<&'static str> {
    d.args().into_iter().map(|(name, _)| name).collect()
}

#[test]
fn test_a_corrupt_configuration_says_where_its_copy_is() {
    let corrupt = ConfigError::Corrupt {
        file: ".claude.json".to_string(),
        detail: "x".to_string(),
        backup: "/b".to_string(),
    };
    assert_eq!(
        corrupt.english(),
        "failed to parse .claude.json: x - original backed up to /b; fix the JSON and re-run (refusing to write .claude.json to avoid deleting the MCP servers already in it)"
    );
    assert_eq!(arg_names(&corrupt), ["file", "ext_err_txt", "backup"]);
}

#[test]
fn test_no_home_names_the_variable_each_flavor_reads() {
    let unix = ConfigError::NoHome {
        flavor: os::Flavor::Unix,
    };
    let windows = ConfigError::NoHome {
        flavor: os::Flavor::Windows,
    };
    assert_eq!(
        unix.english(),
        "the home directory is not set (HOME), so the user's configuration cannot be found"
    );
    assert_eq!(
        windows.english(),
        "the home directory is not set (USERPROFILE), so the user's configuration cannot be found"
    );
    assert_eq!(arg_names(&windows), ["var"]);
    assert!(matches!(
        &windows.args()[..],
        [("var", angzarr_embedded_diagnostic::Arg::Str(v))] if v == "USERPROFILE"
    ));
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
        used.dedup();
        let mut args = arg_names(&*sample);
        args.sort_unstable();
        assert_eq!(used, args, "{}", sample.code());
    }
}
