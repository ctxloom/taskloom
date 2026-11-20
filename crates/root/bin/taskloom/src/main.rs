//! taskloom: per-project task tracking, for people and agents alike on
//! its command line (it serves no MCP). The composition root: it parses the
//! command line (the global flags, and the groups `tasks`, `plans` and
//! `manage` with `loadout` beside them: noun-verb, no `help` command), builds
//! the wiring from the process (tasks-lib's `Taskloom`, the `--config-set`
//! overrides bound there), and hands the frontend the command parsed only
//! the use case it runs: the tasks use cases for the task commands, the
//! schema and `tasks watch`; the importer, the only reader of the JSONL task
//! logs, for `manage migrate tasks`; the plans use cases, over plan files,
//! for `plans` and the `manage hook stamp-plan` hook; and the project use
//! cases for `manage project` and `manage doctor`; and, for `manage store`,
//! the project use cases' export and the tasks' digest, with the app's
//! descriptor sets. `tasks watch` streams
//! the project's changes, and `plans watch` the plan files', until SIGINT
//! or SIGTERM (on Windows Ctrl-C, Ctrl-Break, or the console's close,
//! logoff or shutdown; never at a closed stdin). Each use case that holds a
//! store is closed before the process exits (tasks-lib's `Close`, ruling
//! 131), so the store's file holds every commit with no log left beside it.
//! It holds no logic of its own, and wires no adapter.

use std::io::{self, IsTerminal, Read};
use std::process::ExitCode;
use std::time::Duration;

use tasks_cli::Context;
use tasks_lib::{Close, Taskloom};

/// The binary's en-US catalog: every diagnostic the wiring can raise
/// (tasks-lib's), then each frontend's own.
fn catalog() -> Result<angzarr_presentation::Catalog, angzarr_presentation::CatalogError> {
    tasks_lib::catalog(&[
        tasks_cli::EN_US,
        project_cli::EN_US,
        tasks_watch::EN_US,
        plans_cli::EN_US,
        store_cli::EN_US,
    ])
}

/// The command line.
#[derive(clap::Parser, Debug)]
#[command(name = tasks_cli::PROG, about = "Manage the per-project task store")]
struct Cli {
    #[command(flatten)]
    globals: tasks_cli::Globals,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Add, list, show and change tasks
    #[command(subcommand)]
    Tasks(TasksVerb),
    /// Plan files, their status and links
    #[command(subcommand)]
    Plans(PlansVerb),
    /// The store's rules, migrations, hooks and diagnosis
    #[command(subcommand)]
    Manage(Manage),
    /// Print what taskloom contributes to a ctxloom session (as yaml)
    #[command(after_help = "Examples:\n  taskloom loadout --format yaml")]
    Loadout,
}

/// The verbs of `tasks`: the task commands, and `watch` mounted beside them.
#[derive(clap::Subcommand, Debug)]
enum TasksVerb {
    #[command(flatten)]
    Task(tasks_cli::Tasks),
    #[command(flatten)]
    Watch(tasks_watch::Group),
}

/// The verbs of `plans`: the plan commands, and `watch` mounted beside them.
#[derive(clap::Subcommand, Debug)]
enum PlansVerb {
    #[command(flatten)]
    Plan(plans_cli::Command),
    #[command(flatten)]
    Watch(plans_cli::Group),
}

/// The groups of `manage`.
#[derive(clap::Subcommand, Debug)]
enum Manage {
    /// Commands ctxloom calls on a session's events
    #[command(subcommand)]
    Hook(Hook),
    /// The store's tag rules, permits, tags and statuses
    #[command(subcommand)]
    Schema(tasks_cli::Schema),
    /// Move older data into the store
    #[command(subcommand)]
    Migrate(Migrate),
    /// Diagnose and repair which project a directory is
    #[command(subcommand)]
    Doctor(project_cli::DoctorCommand),
    /// Which project this directory is
    #[command(subcommand)]
    Project(project_cli::ProjectCommand),
    /// Carry the project's database across a proto generation
    #[command(subcommand)]
    Store(store_cli::Command),
    /// Print the taskloom version
    #[command(after_help = "Examples:\n  taskloom manage version")]
    Version,
}

/// The hooks ctxloom calls.
#[derive(clap::Subcommand, Debug)]
enum Hook {
    /// Record the session in a plan file's frontmatter, as it is edited
    #[command(after_help = "Examples:\n  taskloom manage hook stamp-plan")]
    StampPlan,
}

/// The migrations.
#[derive(clap::Subcommand, Debug)]
enum Migrate {
    /// Import Go taskloom's task logs into each project's store
    #[command(after_help = "Examples:\n  taskloom manage migrate tasks")]
    Tasks,
    /// Move the config file's tag rules into the project's store
    #[command(after_help = "Examples:\n  taskloom manage migrate schema")]
    Schema,
}

/// Where a parsed command goes: the use case that answers it.
enum Routed {
    Tasks(tasks_cli::Command),
    Mounted(Mounted),
}

/// The commands that are no task use case's.
enum Mounted {
    Import,
    Watch(tasks_watch::Args),
    PlansWatch,
    StampPlan,
    Plans(plans_cli::Command),
    Project(project_cli::Command),
    Store(store_cli::Command),
}

impl From<Command> for Routed {
    fn from(command: Command) -> Routed {
        match command {
            Command::Tasks(TasksVerb::Task(tasks)) => {
                Routed::Tasks(tasks_cli::Command::Tasks(tasks))
            }
            Command::Tasks(TasksVerb::Watch(tasks_watch::Group::Watch(args))) => {
                Routed::Mounted(Mounted::Watch(args))
            }
            Command::Plans(PlansVerb::Plan(plans)) => Routed::Mounted(Mounted::Plans(plans)),
            Command::Plans(PlansVerb::Watch(plans_cli::Group::Watch)) => {
                Routed::Mounted(Mounted::PlansWatch)
            }
            Command::Loadout => Routed::Tasks(tasks_cli::Command::Loadout),
            Command::Manage(Manage::Version) => Routed::Tasks(tasks_cli::Command::Version),
            Command::Manage(Manage::Hook(Hook::StampPlan)) => Routed::Mounted(Mounted::StampPlan),
            Command::Manage(Manage::Schema(schema)) => {
                Routed::Tasks(tasks_cli::Command::Schema(schema))
            }
            Command::Manage(Manage::Migrate(Migrate::Tasks)) => Routed::Mounted(Mounted::Import),
            Command::Manage(Manage::Migrate(Migrate::Schema)) => {
                Routed::Tasks(tasks_cli::Command::ImportConfig)
            }
            Command::Manage(Manage::Doctor(doctor)) => {
                Routed::Mounted(Mounted::Project(project_cli::Command::Doctor(doctor)))
            }
            Command::Manage(Manage::Project(project)) => {
                Routed::Mounted(Mounted::Project(project_cli::Command::Project(project)))
            }
            Command::Manage(Manage::Store(store)) => Routed::Mounted(Mounted::Store(store)),
        }
    }
}

/// `code`, the command's, once `held`'s store is closed and waited for
/// (ruling 131), before the process exits: dropped, its close would be left
/// to the driver's thread, which the exit never lets run, and the store's
/// log would stay beside it. A close that fails is said, and fails a
/// command that had not.
fn closing(
    runtime: &tokio::runtime::Runtime,
    held: impl Close,
    code: u8,
    catalog: &angzarr_presentation::Catalog,
) -> ExitCode {
    match runtime.block_on(held.close()) {
        Ok(()) => ExitCode::from(code),
        Err(e) => {
            eprintln!("{}: {}", tasks_cli::PROG, catalog.message(&e));
            ExitCode::from(code.max(1))
        }
    }
}

/// `taskloom manage migrate tasks`: the JSONL task logs under the home imported into
/// each project's store, by an importer built for this command alone.
fn import(
    runtime: &tokio::runtime::Runtime,
    taskloom: &Taskloom,
    project: Option<String>,
    format: angzarr_presentation::Format,
    catalog: &angzarr_presentation::Catalog,
) -> ExitCode {
    let mut importer = taskloom.importer();
    let code = runtime.block_on(tasks_cli::run_import(
        project,
        format,
        catalog,
        &mut importer,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    ));
    closing(runtime, importer, code, catalog)
}

/// `taskloom tasks watch`: the project's task events on stdout, polled every
/// `POLL`, until interrupted.
fn watch<W: tasks_application::WatchService>(
    runtime: &tokio::runtime::Runtime,
    os: &dyn os::Os,
    service: &mut W,
    settings: &tasks_watch::Settings,
    catalog: &angzarr_presentation::Catalog,
) -> u8 {
    runtime.block_on(async {
        // SIGINT or SIGTERM (on Windows Ctrl-C, Ctrl-Break, or the
        // console's close, logoff or shutdown), listened for from here,
        // made on the runtime.
        let stop = match os.stop_signal() {
            Ok(stop) => stop,
            Err(e) => {
                eprintln!("{}: cannot listen for signals: {e}", tasks_cli::PROG);
                return 1;
            }
        };
        // Stdin is not watched: the VS Code extension starts the watch
        // with stdin from /dev/null.
        let mut ticks = tasks_watch::Interval::every(tasks_watch::POLL);
        tasks_watch::run(
            service,
            settings,
            &mut ticks,
            stop,
            catalog,
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        )
        .await
    })
}

/// `taskloom manage store`: the project's database written whole as JSONL,
/// and its streams' digest (store-export-import.plan.md), over the project
/// use cases and the tasks' digest, the app's descriptor sets given.
fn store_group(
    runtime: &tokio::runtime::Runtime,
    taskloom: &Taskloom,
    command: store_cli::Command,
    project: Option<String>,
    catalog: &angzarr_presentation::Catalog,
) -> ExitCode {
    let mut projects = taskloom.projects();
    let mut tasks = taskloom.tasks();
    let settings = store_cli::Settings {
        project: project.as_deref(),
        descriptors: tasks_lib::DESCRIPTOR_SETS,
        prog: tasks_cli::PROG,
    };
    let use_cases = store_cli::UseCases {
        projects: &mut projects,
        tasks: &mut tasks,
    };
    let code = runtime.block_on(store_cli::run(
        command,
        &settings,
        catalog,
        use_cases,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    ));
    // The projects' check and export close the stores they open.
    closing(runtime, tasks, code, catalog)
}

/// The project commands, over the project registry and the SQLite store's
/// integrity check.
fn project_group(
    taskloom: &Taskloom,
    group: project_cli::Command,
    settings: &project_cli::Settings,
    catalog: &angzarr_presentation::Catalog,
) -> ExitCode {
    let mut projects = taskloom.projects();
    ExitCode::from(project_cli::run(
        group,
        settings,
        catalog,
        &mut projects,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    ))
}

/// How often `plans watch` looks at the plan files: the work tree is
/// walked each time, so less often than the task store's token is polled.
const PLANS_POLL: Duration = Duration::from_millis(500);

/// `plans watch`'s ticks: [`PLANS_POLL`], on the runtime's timer.
struct PlanTicks(tasks_watch::Interval);

impl plans_cli::Ticks for PlanTicks {
    async fn tick(&mut self) {
        tasks_watch::Ticks::tick(&mut self.0).await;
    }
}

/// `taskloom plans watch`: the plan files' changes on stdout, until
/// interrupted.
fn plans_watch<P: plans_cli::Plans>(
    runtime: &tokio::runtime::Runtime,
    os: &dyn os::Os,
    plans: &mut P,
    settings: &plans_cli::Settings,
    catalog: &angzarr_presentation::Catalog,
) -> u8 {
    runtime.block_on(async {
        let stop = match os.stop_signal() {
            Ok(stop) => stop,
            Err(e) => {
                eprintln!("{}: cannot listen for signals: {e}", tasks_cli::PROG);
                return 1;
            }
        };
        let mut ticks = PlanTicks(tasks_watch::Interval::every(PLANS_POLL));
        plans_cli::watch(
            plans,
            settings,
            &mut ticks,
            stop,
            catalog,
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        )
        .await
    })
}

/// `taskloom manage hook stamp-plan`: the session recorded in the plan
/// file the hook payload on stdin names.
fn stamp_plan(
    runtime: &tokio::runtime::Runtime,
    taskloom: &Taskloom,
    project: Option<String>,
    settings: &plans_cli::Settings,
    catalog: &angzarr_presentation::Catalog,
) -> ExitCode {
    let mut payload = String::new();
    // A payload that cannot be read is the hook's to say, as an empty one.
    let _ = io::stdin().lock().read_to_string(&mut payload);
    let mut plans = taskloom.plans(project);
    let code = plans_cli::stamp(
        &payload,
        settings,
        catalog,
        &mut plans,
        &mut io::stderr().lock(),
    );
    closing(runtime, plans, code, catalog)
}

/// The `plans` group, over the plans use cases: the plan files wherever
/// the plan roots are, and the project's tasks and rules.
fn plan_group(
    runtime: &tokio::runtime::Runtime,
    taskloom: &Taskloom,
    group: plans_cli::Command,
    project: Option<String>,
    settings: &plans_cli::Settings,
    catalog: &angzarr_presentation::Catalog,
) -> ExitCode {
    let mut plans = taskloom.plans(project);
    let code = runtime.block_on(plans_cli::run(
        group,
        settings,
        catalog,
        &mut plans,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    ));
    closing(runtime, plans, code, catalog)
}

/// What the command line reads from the process besides its arguments.
fn context() -> Context {
    Context {
        session: std::env::var("CTXLOOM_SESSION_HARP").unwrap_or_default(),
        stdout_is_terminal: io::stdout().is_terminal(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        // True of every lock file taskloom takes, each through file-lock:
        // the project registry's `index.yaml.lock`. It takes none on a
        // JSONL task log (the importer reads without one); its store is
        // SQLite's to lock.
        lock_protocol: file_lock::LOCK_PROTOCOL,
    }
}

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("{}: cannot start: {e}", tasks_cli::PROG);
            return ExitCode::FAILURE;
        }
    };
    let catalog = match catalog() {
        Ok(catalog) => catalog,
        Err(e) => {
            eprintln!("{}: {e}", tasks_cli::PROG);
            return ExitCode::FAILURE;
        }
    };
    let context = context();
    // Stdout and stderr are locked for one call at a time.
    let (stdout, stderr) = (io::stdout(), io::stderr());
    let parsed = tasks_cli::parse(
        std::env::args_os(),
        &context,
        &catalog,
        &mut stdout.lock(),
        &mut stderr.lock(),
    );
    let Cli { globals, command } = match parsed {
        Ok(cli) => cli,
        Err(code) => return ExitCode::from(code),
    };
    // The working directory, HOME, CTXLOOM_ROOT, CTXLOOM_PROJECT_ID and
    // the variables, read once; an unreadable working directory fails
    // only the commands that need it.
    let built = tasks_lib::Builder::from_process()
        .config_set(globals.config_set.clone())
        .build();
    let taskloom = match built {
        Ok(taskloom) => taskloom,
        Err(e) => {
            eprintln!("{}: {}", tasks_cli::PROG, catalog.message(&e));
            return ExitCode::FAILURE;
        }
    };
    let routed = Routed::from(command);
    let command = match routed {
        Routed::Tasks(command) => command,
        Routed::Mounted(other) => {
            return mounted(&runtime, &taskloom, other, globals, &context, &catalog)
        }
    };
    let mut service = taskloom.tasks();
    let code = runtime.block_on(tasks_cli::run(
        command,
        globals,
        &context,
        &catalog,
        &mut service,
        &mut stdout.lock(),
        &mut stderr.lock(),
    ));
    closing(&runtime, service, code, &catalog)
}

/// The commands that are no task use case's: `manage migrate tasks`, `tasks
/// watch`, `plans` and the project commands.
fn mounted(
    runtime: &tokio::runtime::Runtime,
    taskloom: &Taskloom,
    routed: Mounted,
    globals: tasks_cli::Globals,
    context: &Context,
    catalog: &angzarr_presentation::Catalog,
) -> ExitCode {
    let format = match globals.format(context, catalog, &mut io::stderr().lock()) {
        Ok(format) => format,
        Err(code) => return ExitCode::from(code),
    };
    let project = globals.project;
    let plan_settings = || plans_cli::Settings {
        format,
        session: context.session.clone(),
        prog: tasks_cli::PROG,
    };
    match routed {
        Mounted::Import => import(runtime, taskloom, project, format, catalog),
        Mounted::Project(group) => {
            let settings = project_cli::Settings {
                format,
                project,
                prog: tasks_cli::PROG,
            };
            project_group(taskloom, group, &settings, catalog)
        }
        Mounted::Store(command) => store_group(runtime, taskloom, command, project, catalog),
        Mounted::Plans(group) => {
            plan_group(runtime, taskloom, group, project, &plan_settings(), catalog)
        }
        Mounted::PlansWatch => {
            let mut plans = taskloom.plans(project);
            let code = plans_watch(
                runtime,
                taskloom.fs().os(),
                &mut plans,
                &plan_settings(),
                catalog,
            );
            closing(runtime, plans, code, catalog)
        }
        Mounted::StampPlan => stamp_plan(runtime, taskloom, project, &plan_settings(), catalog),
        Mounted::Watch(args) => {
            let settings = tasks_watch::Settings {
                format,
                project,
                since: args.since,
                heartbeat: tasks_watch::heartbeat_ticks(args.heartbeat),
                prog: tasks_cli::PROG,
            };
            let mut service = taskloom.tasks();
            let code = watch(
                runtime,
                taskloom.fs().os(),
                &mut service,
                &settings,
                catalog,
            );
            closing(runtime, service, code, catalog)
        }
    }
}

#[cfg(test)]
#[path = "main.test.rs"]
mod tests;
