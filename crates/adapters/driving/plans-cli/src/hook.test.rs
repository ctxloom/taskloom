//! The stamp-plan hook: the file its payload names, and that it never
//! blocks the edit it follows.

use angzarr_presentation::Format;
use plans_application::{Plan, PlanError, Stamp};

use super::*;

/// Plans that record what they were asked to stamp.
#[derive(Default)]
struct Stamping {
    asked: Vec<(String, String)>,
    failure: Option<PlanError>,
}

impl Plans for Stamping {
    async fn list(&mut self, _: &plans_application::PlanQuery) -> Result<Vec<Plan>, PlanError> {
        unreachable!()
    }
    async fn show(&mut self, _: &str) -> Result<plans_application::PlanText, PlanError> {
        unreachable!()
    }
    async fn add(&mut self, _: plans_application::NewPlan) -> Result<Plan, PlanError> {
        unreachable!()
    }
    async fn set(&mut self, _: &str, _: plans_application::SetPlan) -> Result<Plan, PlanError> {
        unreachable!()
    }
    async fn tag(&mut self, _: &str, _: &str, _: bool) -> Result<Plan, PlanError> {
        unreachable!()
    }
    async fn link(
        &mut self,
        _: &str,
        _: plans_application::Link,
        _: bool,
    ) -> Result<Plan, PlanError> {
        unreachable!()
    }
    async fn check(&mut self) -> Result<plans_application::CheckReport, PlanError> {
        unreachable!()
    }
    fn stamp(&mut self, path: &str, session: &str) -> Result<Stamp, PlanError> {
        self.asked.push((path.to_string(), session.to_string()));
        match self.failure.clone() {
            Some(error) => Err(error),
            None => Ok(Stamp::Stamped),
        }
    }
    fn snapshot(&mut self) -> Result<plans_application::Snapshot, PlanError> {
        unreachable!()
    }
}

fn run(payload: &str, plans: &mut Stamping) -> (u8, String) {
    let settings = Settings {
        format: Format::Text,
        session: "this-session".to_string(),
        prog: "taskloom",
    };
    let mut sources = vec![crate::EN_US];
    sources.extend(plans_application::CATALOGS);
    let catalog = Catalog::en_us(&sources).unwrap();
    let mut err = Vec::new();
    let code = stamp(payload, &settings, &catalog, plans, &mut err);
    (code, String::from_utf8(err).unwrap())
}

#[test]
fn the_file_a_payload_names_is_stamped_with_the_session() {
    for payload in [
        r#"{"tool_input":{"file_path":"/tree/p.plan.md"}}"#,
        r#"{"session_id":"x","tool_name":"Write","tool_input":{"file_path":"/tree/p.plan.md","content":"c"}}"#,
        r#"{"tool_input":{"notebook_path":"/tree/p.plan.md"}}"#,
    ] {
        let mut plans = Stamping::default();
        assert_eq!(run(payload, &mut plans), (0, String::new()), "{payload}");
        assert_eq!(
            plans.asked,
            [("/tree/p.plan.md".to_string(), "this-session".to_string())]
        );
    }
}

#[test]
fn a_payload_it_cannot_read_is_said_on_stderr_and_stamps_nothing_and_the_hook_ends_0() {
    for payload in [
        "",
        "not json",
        "{}",
        r#"{"tool_input":{}}"#,
        r#"{"tool_input":{"file_path":""}}"#,
    ] {
        let mut plans = Stamping::default();
        let (code, err) = run(payload, &mut plans);
        assert_eq!(code, 0, "{payload:?}");
        assert!(
            err.starts_with("taskloom: stamp-plan: "),
            "{payload:?}: {err}"
        );
        assert!(plans.asked.is_empty());
    }
}

#[test]
fn a_file_it_cannot_stamp_is_said_and_the_hook_still_ends_0() {
    let mut plans = Stamping {
        failure: Some(PlanError::NoChange),
        ..Stamping::default()
    };
    let (code, err) = run(
        r#"{"tool_input":{"file_path":"/tree/p.plan.md"}}"#,
        &mut plans,
    );
    assert_eq!(code, 0);
    assert!(err.contains("stamp-plan"), "{err}");
}
