//! The errors' keys and args, and the catalog.

use angzarr_diagnostic::{code, Arg, Diagnostic};

use super::{diagnostic_samples, StoreOpenError};

fn arg_names(d: &dyn Diagnostic) -> Vec<&str> {
    d.args().into_iter().map(|(name, _)| name).collect()
}

// RED, step 3c of taskloom-on-angzarr-embedded.plan.md: on
// angzarr-diagnostic an error's `Display` is its key and its
// args, a text value quoted where it would not parse back, never English
// (angzarr decision 14.37); a third party's text (the OS's, through std's
// io::Error) is the arg `ext_err_txt`, as angzarr-store-sqlite carries
// SQLite's. And the store from before angzarr-embedded (ruling D2) has its
// key here, with the path and its backup.
#[test]
fn test_each_error_displays_its_code_and_args_never_english() {
    let displayed: Vec<String> = diagnostic_samples().iter().map(|d| d.to_string()).collect();
    assert_eq!(
        displayed,
        [
            concat!(
                code!("create-dir"),
                r#" path=/home/u/.ctxloom/projects/jolly-mute-magma ext_err_txt="permission denied""#
            ),
            concat!(
                code!("unplaced"),
                r#" path=/home/u/.ctxloom/projects/jolly-mute-magma/state.db ext_err_txt="not an absolute path: C:state.db""#
            ),
            concat!(code!("no-runtime"), r#" ext_err_txt="too many open files""#),
            concat!(
                code!("predates-embedded"),
                " path=/home/u/.ctxloom/projects/jolly-mute-magma/state.db \
             backup=/home/u/.ctxloom/projects/jolly-mute-magma/state.db.pre-embedded"
            ),
            concat!(code!("no-output"), r#" ext_err_txt="broken pipe""#),
        ]
    );
}

#[test]
fn test_the_catalog_says_a_store_predates_angzarr_embedded_with_its_path_and_backup() {
    use fluent_syntax::ast::{Entry, Expression, InlineExpression, PatternElement};

    let resource = fluent_syntax::parser::parse(crate::EN_US.1).expect("the catalog parses");
    let message = resource
        .body
        .iter()
        .find_map(|e| match e {
            Entry::Message(m) if m.id.name == code!("predates-embedded") => Some(m),
            _ => None,
        })
        .expect("a message for the key");
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
    assert_eq!(used, ["backup", "path"]);
}

#[test]
fn test_each_error_gives_its_args_values() {
    let error = StoreOpenError::CreateDir {
        path: "p".to_string(),
        detail: "d".to_string(),
    };
    assert!(
        matches!(&error.args()[..], [("path", Arg::Str(p)), ("ext_err_txt", Arg::Str(d))] if p == "p" && d == "d"),
        "{error:?}"
    );
    let error = StoreOpenError::Unplaced {
        path: "p".to_string(),
        detail: "d".to_string(),
    };
    assert!(
        matches!(&error.args()[..], [("path", Arg::Str(p)), ("ext_err_txt", Arg::Str(d))] if p == "p" && d == "d"),
        "{error:?}"
    );
    let error = StoreOpenError::NoRuntime {
        detail: "d".to_string(),
    };
    assert!(
        matches!(&error.args()[..], [("ext_err_txt", Arg::Str(d))] if d == "d"),
        "{error:?}"
    );
    assert!(
        matches!(error, StoreOpenError::NoRuntime { .. }),
        "{error:?}"
    );
}

#[test]
fn test_every_error_has_a_catalog_message_with_exactly_its_args() {
    use fluent_syntax::ast::{Entry, Expression, InlineExpression, PatternElement};

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
        let mut args = arg_names(&*sample);
        args.sort_unstable();
        assert_eq!(used, args, "{}", sample.code());
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
