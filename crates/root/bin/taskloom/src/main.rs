//! taskloom: per-project task tracking, for people and agents alike on
//! its command line (it serves no MCP). The composition root: it parses the
//! command line (the global flags, the task commands, and `import`, the
//! `project` group and `watch` mounted beside them), builds the wiring
//! from the process (tasks-lib's `Taskloom`, the `--config-set` overrides
//! bound there), and hands the frontend the command parsed only the use
//! case it runs: the tasks use cases for the task commands and `watch`;
//! the importer, the only reader of the JSONL task logs, for `import`;
//! and the project use cases for the `project` group. `watch`
//! streams the project's changes until SIGINT or SIGTERM (on Windows
//! Ctrl-C, Ctrl-Break, or the console's close, logoff or shutdown; never
//! at a closed stdin). It holds no logic of its own, and wires no adapter.

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use tasks_cli::Context;
use tasks_lib::Taskloom;

/// The binary's en-US catalog: every diagnostic the wiring can raise
/// (tasks-lib's), then each frontend's own.
fn catalog() -> Result<presentation::Catalog, presentation::CatalogError> {
    tasks_lib::catalog(&[tasks_cli::EN_US, project_cli::EN_US, tasks_watch::EN_US])
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
    #[command(flatten)]
    Tasks(tasks_cli::Command),
    #[command(flatten)]
    Mounted(Mounted),
}

/// The commands mounted beside the task commands.
#[derive(clap::Subcommand, Debug)]
enum Mounted {
    #[command(flatten)]
    Import(tasks_cli::ImportGroup),
    #[command(flatten)]
    Project(project_cli::Group),
    #[command(flatten)]
    Watch(tasks_watch::Group),
}

/// `taskloom import`: the JSONL task logs under the home imported into
/// each project's store, by an importer built for this command alone.
fn import(
    runtime: &tokio::runtime::Runtime,
    taskloom: &Taskloom,
    project: Option<String>,
    format: presentation::Format,
    catalog: &presentation::Catalog,
) -> ExitCode {
    let mut importer = taskloom.importer();
    ExitCode::from(runtime.block_on(tasks_cli::run_import(
        project,
        format,
        catalog,
        &mut importer,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )))
}

/// `taskloom watch`: the project's changes on stdout, polled every
/// debounce, until interrupted.
fn watch<W: tasks_application::WatchService>(
    runtime: &tokio::runtime::Runtime,
    os: &dyn os::Os,
    service: &mut W,
    settings: &tasks_watch::Settings,
    catalog: &presentation::Catalog,
) -> ExitCode {
    let code = runtime.block_on(async {
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
        let mut ticks = tasks_watch::Interval::every(tasks_watch::DEBOUNCE);
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
    });
    ExitCode::from(code)
}

/// The `project` group, over the project registry and the SQLite store's
/// integrity check.
fn project_group(
    taskloom: &Taskloom,
    group: project_cli::Group,
    settings: &project_cli::Settings,
    catalog: &presentation::Catalog,
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

/// What the command line reads from the process besides its arguments.
fn context() -> Context {
    Context {
        session: std::env::var("CTXLOOM_SESSION_HARP").unwrap_or_default(),
        stdout_is_terminal: io::stdout().is_terminal(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        // True of every lock file taskloom takes, each through file-lock:
        // the project registry's `index.yaml.lock`, with the reap
        // interlock held shared. It takes none on a JSONL task log (the
        // importer reads without one); its store is SQLite's to lock.
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
    let group = match command {
        Command::Tasks(command) => {
            let mut service = taskloom.tasks();
            return ExitCode::from(runtime.block_on(tasks_cli::run(
                command,
                globals,
                &context,
                &catalog,
                &mut service,
                &mut stdout.lock(),
                &mut stderr.lock(),
            )));
        }
        Command::Mounted(group) => group,
    };
    let format = match globals.format(&context, &catalog, &mut stderr.lock()) {
        Ok(format) => format,
        Err(code) => return ExitCode::from(code),
    };
    let project = globals.project;
    match group {
        Mounted::Import(tasks_cli::ImportGroup::Import) => {
            import(&runtime, &taskloom, project, format, &catalog)
        }
        Mounted::Project(group) => {
            let settings = project_cli::Settings {
                format,
                project,
                prog: tasks_cli::PROG,
            };
            project_group(&taskloom, group, &settings, &catalog)
        }
        Mounted::Watch(tasks_watch::Group::Watch) => {
            let settings = tasks_watch::Settings {
                format,
                project,
                prog: tasks_cli::PROG,
            };
            watch(
                &runtime,
                taskloom.fs().os(),
                &mut taskloom.tasks(),
                &settings,
                &catalog,
            )
        }
    }
}

#[cfg(test)]
#[path = "main.test.rs"]
mod tests;
