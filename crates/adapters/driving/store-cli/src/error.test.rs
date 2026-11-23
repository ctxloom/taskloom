//! The crate's catalog against its diagnostics, and their English.

use std::collections::BTreeSet;

angzarr_presentation::english_for!(&[crate::EN_US.1]);

use super::diagnostic_samples;

#[test]
fn test_each_diagnostic_renders_its_english() {
    let english: Vec<String> = diagnostic_samples().iter().map(|d| d.english()).collect();
    assert_eq!(
        english,
        [
            "cannot write the output: broken pipe",
            "exported 6171 records over 412 streams",
        ]
    );
}

#[test]
fn test_every_diagnostic_has_a_catalog_message_with_exactly_its_args() {
    let (name, text) = crate::EN_US;
    assert_eq!(name, env!("CARGO_PKG_NAME"));
    let samples = diagnostic_samples();
    let codes: BTreeSet<&str> = samples.iter().map(|d| d.code()).collect();
    assert_eq!(codes.len(), samples.len());
    assert_eq!(angzarr_diagnostic::catalog::check(name, text, &samples), []);
}

#[test]
fn test_a_state_that_forbids_is_declined_and_what_broke_is_a_failure() {
    use crate::CliError;
    use angzarr_store_transfer::ExportError;
    use cli_support::ExitClass;
    use project_application::ProjectError;
    use tasks_application::AppError;

    let status = |error: CliError| error.exit().code(false);
    assert_eq!(
        status(CliError::Project(ProjectError::NoDatabase {
            id: "x".into()
        })),
        48
    );
    assert_eq!(status(CliError::Tasks(AppError::NoProject)), 48);
    assert_eq!(
        status(CliError::Digest(ExportError::PendingOutbox { entries: 1 })),
        32
    );
    assert_eq!(
        status(CliError::Output {
            detail: "closed".into()
        }),
        32
    );
}
