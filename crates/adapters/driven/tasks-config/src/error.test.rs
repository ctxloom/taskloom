//! The errors' English and the catalog.

use angzarr_diagnostic::{code, Diagnostic};

angzarr_presentation::english_for!(&[crate::EN_US.1, angzarr_yaml::EN_US.1]);

use super::diagnostic_samples;

fn arg_names(d: &dyn Diagnostic) -> Vec<&str> {
    d.args().into_iter().map(|(name, _)| name).collect()
}

#[test]
fn test_each_error_renders_its_english_fallback() {
    let english: Vec<String> = diagnostic_samples().iter().map(|d| d.english()).collect();
    let path = "/home/u/.taskloom/config.yaml";
    assert_eq!(
        english,
        [
            format!("cannot read {path}: permission denied"),
            format!("taskloom-config document {path}: `homeing`: unknown key \"homeing\" (the keys are: schema_version,homing,tag_schema,plan_roots,tag_permits)"),
            format!("{path}: tag_permits: namespaces: area is maybe, which is neither open nor a list of keys"),
            format!("{path} is schema_version 2, newer than the 1 this taskloom reads"),
            "TASKLOOM_CONFIG_TAGS sets tags, which is no config key taskloom knows (known: homing, plan_roots, schema_version, tag_permits, tag_schema)".to_string(),
            "--config-set tag_schema: expected <path>=<value>".to_string(),
            format!("{path}: tag_schema has a blank item: give one, or leave it out"),
            "TASKLOOM_CONFIG_TAG_SCHEMA sets tag_schema, which moved into the project's store: run `taskloom manage migrate schema` in the work root to move a project's .taskloom/config.yaml into its store, and delete it from anywhere else".to_string(),
            "there is no work root here, so no project config to write tag permits to".to_string(),
            format!("{path} was left as it was: editing it would not have read back as meant (schema_version changed); this is a bug in taskloom"),
            format!("cannot write {path}: read-only file system"),
        ]
    );
}

#[test]
fn test_each_error_has_its_own_code() {
    let samples = diagnostic_samples();
    let mut codes: Vec<&str> = samples.iter().map(|d| d.code()).collect();
    let all = codes.len();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), all);
    // taskloom's own, or the reader's for a file it does not load.
    let unloaded = crate::loader()
        .load_into::<crate::File>(Some("c.yaml"), "schema_version: 1\nhomeing: home\n")
        .expect_err("an unknown key is refused");
    assert!(
        codes
            .iter()
            .all(|c| c.starts_with(code!("")) || *c == unloaded.code()),
        "{codes:?}"
    );
}

#[test]
fn test_every_error_has_a_catalog_message_with_exactly_its_args() {
    use fluent_syntax::ast::{Entry, Expression, InlineExpression, PatternElement};

    let ftl = crate::EN_US.1;
    let resource = fluent_syntax::parser::parse(ftl).expect("the catalog parses");
    // This crate's own codes: the reader's are in its catalog.
    let samples: Vec<_> = diagnostic_samples()
        .into_iter()
        .filter(|s| s.code().starts_with(code!("")))
        .collect();
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
/// is under that name, with its message using exactly its args. The
/// samples angzarr-yaml's errors forward are angzarr-yaml's to check.
#[test]
fn test_every_code_is_under_the_crates_name_with_its_message() {
    let (name, text) = crate::EN_US;
    assert_eq!(name, env!("CARGO_PKG_NAME"));
    let ours: Vec<_> = crate::diagnostic_samples()
        .into_iter()
        .filter(|d| d.code().starts_with(code!("")))
        .collect();
    assert_eq!(angzarr_diagnostic::catalog::check(name, text, &ours), []);
}
