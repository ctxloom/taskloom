//! The group parsed as a binary mounts it, run over fake use-case ports:
//! what each command asks for, and what it writes where.

use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};

use angzarr_diagnostic::{Diagnostic, Report};
use angzarr_presentation::Catalog;
use clap::Parser;
use plans_application::{DigestPlans, PlanError};
use project_application::{Change, Exported, ProjectAdmin, Projects, StoreExport, Survey};
use prost::Message;
use tasks_application::{AppError, DigestService, Folded};

use super::{run, Group, Settings, StoreCommand, UseCases};

/// A binary mounting the group beside nothing else.
#[derive(Parser, Debug)]
struct Bin {
    #[command(subcommand)]
    group: Group,
}

fn parse(args: &[&str]) -> Group {
    Bin::try_parse_from(std::iter::once("bin").chain(args.iter().copied()))
        .unwrap()
        .group
}

/// A survey resolving the project `jolly-mute-magma`.
struct Fake {
    pinned: Vec<Option<String>>,
}

impl ProjectAdmin for Fake {
    fn survey(&mut self, pinned: Option<&str>) -> Result<Survey, Report> {
        self.pinned.push(pinned.map(str::to_string));
        Ok(Survey {
            work_root: "/w".to_string(),
            root: "/w".to_string(),
            resolved: Some((
                "jolly-mute-magma".to_string(),
                project_application::IdSource::Marker,
            )),
            marker: Some("jolly-mute-magma".to_string()),
            legacy_marker: None,
            registry: Vec::new(),
            databases: Vec::new(),
            projects: PathBuf::from("/h/.ctxloom/projects"),
        })
    }

    fn apply(&mut self, _: &Survey, _: &[Change]) -> Result<(), Report> {
        unreachable!("the store group changes nothing")
    }
}

/// An export writing `lines` and answering `answer`; the descriptor sets
/// each was given recorded.
struct FakeExport {
    answer: Result<Option<Exported>, Report>,
    given: Vec<usize>,
}

impl StoreExport for FakeExport {
    fn export(
        &mut self,
        _: &Path,
        descriptors: &[&[u8]],
        out: &mut dyn Write,
    ) -> Result<Option<Exported>, Report> {
        self.given.push(descriptors.len());
        if self.answer.is_ok() {
            out.write_all(b"{\"header\":{}}\n{\"end\":{}}\n").unwrap();
        }
        self.answer.clone()
    }
}

/// A digest answering `folded`, the pins it was given recorded.
struct Digest<E> {
    folded: Result<Vec<Folded>, E>,
    pins: Vec<Option<String>>,
}

impl DigestService for Digest<AppError> {
    fn fold_digest(
        &mut self,
        project: Option<String>,
    ) -> impl Future<Output = Result<Vec<Folded>, AppError>> + Send {
        self.pins.push(project);
        std::future::ready(self.folded.clone())
    }
}

impl DigestPlans for Digest<PlanError> {
    fn fold_digest(&mut self) -> impl Future<Output = Result<Vec<Folded>, PlanError>> {
        std::future::ready(self.folded.clone())
    }
}

fn catalog() -> Catalog {
    Catalog::en_us(&[
        crate::EN_US,
        angzarr_store_transfer::EN_US,
        project_application::EN_US,
    ])
    .unwrap()
}

/// One stream folded, its state angzarr's `FactOrigin` (named by
/// angzarr's own descriptors).
fn folded(n: u8, id: &str) -> Folded {
    Folded {
        cover: angzarr_store_transfer::angzarr_ports::StreamRef::main(
            "tasks",
            angzarr_store_transfer::angzarr_ports::Uuid::from_bytes([n; 16]),
        )
        .to_cover(),
        head: u64::from(n),
        state: prost_types::Any {
            type_url: "/io.angzarr.v1.FactOrigin".to_string(),
            value: angzarr_store_transfer::angzarr_ports::angzarr_proto::v1::FactOrigin {
                external_id: id.to_string(),
                description: String::new(),
            }
            .encode_to_vec(),
        },
    }
}

struct Ran {
    code: u8,
    out: String,
    err: String,
}

struct World {
    projects: Projects<Fake, FakeExport>,
    tasks: Digest<AppError>,
    plans: Digest<PlanError>,
}

impl World {
    fn new() -> World {
        World {
            projects: Projects::new(
                Fake { pinned: Vec::new() },
                FakeExport {
                    answer: Ok(Some(Exported {
                        records: 0,
                        streams: 0,
                    })),
                    given: Vec::new(),
                },
            ),
            tasks: Digest {
                folded: Ok(vec![folded(1, "one")]),
                pins: Vec::new(),
            },
            plans: Digest {
                folded: Ok(vec![folded(2, "two")]),
                pins: Vec::new(),
            },
        }
    }

    fn run(&mut self, args: &[&str], project: Option<&str>) -> Ran {
        let sets: [&[u8]; 1] = [b""];
        let settings = Settings {
            project,
            descriptors: &sets[..0],
            prog: "taskloom",
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let use_cases = UseCases {
            projects: &mut self.projects,
            tasks: &mut self.tasks,
            plans: &mut self.plans,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let code = runtime.block_on(run(
            parse(args),
            &settings,
            &catalog(),
            use_cases,
            &mut out,
            &mut err,
        ));
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }
}

#[test]
fn test_the_group_parses_export_and_fold_digest() {
    assert_eq!(
        parse(&["store", "export"]),
        Group::Store(StoreCommand::Export)
    );
    assert_eq!(
        parse(&["store", "fold-digest"]),
        Group::Store(StoreCommand::FoldDigest)
    );
}

#[test]
fn test_export_writes_the_file_to_stdout_and_its_note_to_stderr() {
    let mut world = World::new();
    world.projects = Projects::new(
        Fake { pinned: Vec::new() },
        FakeExport {
            answer: Ok(Some(Exported {
                records: 5,
                streams: 2,
            })),
            given: Vec::new(),
        },
    );
    let ran = world.run(&["store", "export"], Some("jolly-mute-magma"));
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(ran.out, "{\"header\":{}}\n{\"end\":{}}\n", "the file alone");
    assert_eq!(ran.err, "taskloom: exported 5 records over 2 streams\n");
    assert_eq!(
        world.projects.admin().pinned,
        [Some("jolly-mute-magma".to_string())]
    );
    assert_eq!(world.projects.checks().given, [0], "the descriptors given");
}

#[test]
fn test_an_export_refused_says_why_on_stderr_and_exits_1() {
    let mut world = World::new();
    world.projects = Projects::new(
        Fake { pinned: Vec::new() },
        FakeExport {
            answer: Ok(None),
            given: Vec::new(),
        },
    );
    let ran = world.run(&["store", "export"], None);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.out, "", "nothing on stdout");
    assert_eq!(
        ran.err,
        "taskloom: project jolly-mute-magma has no database to export\n"
    );
}

#[test]
fn test_fold_digest_writes_the_tasks_lines_then_the_plans() {
    let mut world = World::new();
    let ran = world.run(&["store", "fold-digest"], Some("jolly-mute-magma"));
    assert_eq!(ran.code, 0, "{}", ran.err);
    let lines: Vec<serde_json::Value> = ran
        .out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "one line per stream");
    assert_eq!(lines[0]["state"]["externalId"], "one", "the tasks' first");
    assert_eq!(lines[1]["state"]["externalId"], "two", "then the plans'");
    assert_eq!(lines[1]["head"], "2");
    assert_eq!(ran.err, "");
    assert_eq!(world.tasks.pins, [Some("jolly-mute-magma".to_string())]);
}

#[test]
fn test_a_digest_that_fails_says_why_and_writes_nothing() {
    let mut world = World::new();
    world.tasks.folded = Err(AppError::NoProject);
    let ran = world.run(&["store", "fold-digest"], None);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.out, "");
    assert!(ran.err.starts_with("taskloom: "), "{}", ran.err);
    assert_eq!(
        ran.err.trim_end(),
        format!(
            "taskloom: {}",
            catalog().message(&AppError::NoProject as &dyn Diagnostic)
        )
    );
}
