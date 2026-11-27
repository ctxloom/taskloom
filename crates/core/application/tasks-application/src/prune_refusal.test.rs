use super::PruneRefusal;
use crate::english::English;
use coded_diagnostic::Diagnostic;

#[test]
fn test_each_refusal_renders_its_english_and_names_its_age() {
    assert_eq!(
        PruneRefusal::AgeUnreadable {
            age: "soon".to_string()
        }
        .english(),
        "the age soon is neither an offset (30d, 12w, 720h) nor a date (2026-01-01)"
    );
    assert_eq!(
        PruneRefusal::AgeNotPositive {
            age: "0d".to_string()
        }
        .english(),
        "the age 0d is not a positive age"
    );
    assert_eq!(
        PruneRefusal::AgeRequired.english(),
        "a prune needs an age: an offset like 30d, or a date like 2026-01-01"
    );
    assert_eq!(
        PruneRefusal::NoStatus.english(),
        "a prune needs at least one status to select by: an empty status set is never read as every status"
    );
    let unreadable = PruneRefusal::AgeUnreadable {
        age: "soon".to_string(),
    };
    let args = unreadable.args();
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].0, "age");
    assert!(PruneRefusal::NoStatus.args().is_empty());
    assert!(PruneRefusal::AgeRequired.args().is_empty());
}
