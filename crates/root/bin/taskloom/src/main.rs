//! taskloom: per-project task tracking. The composition root: it parses the
//! command line (the global flags, the task commands, and `import`, the
//! `project` group, `mcp`, `watch` and `manage` mounted beside them), then
//! builds only what the command parsed uses: the tasks use cases over their
//! adapters (the project registry, harp names, the SQLite task store, the
//! system's clock, ids, timer and randomness, and taskloom's
//! configuration) for the task commands and `watch`, and for nothing else.
//! `import` alone reads the JSONL task logs, through an importer built for
//! it. The `project` group runs over the registry and the SQLite store's
//! integrity check; `mcp` serves the task tools over stdio, each call on a
//! service of its own; `watch` streams the project's changes until SIGINT
//! or SIGTERM (never at a closed stdin); `manage` registers `taskloom mcp`
//! in the agents' configurations under HOME. It holds no logic of its own.

use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use file_system::{Fs, Os};
use harp_names::HarpNames;
use jsonl_task_log::JsonlTaskLogDir;
use project_application::Projects;
use project_registry::{Environment, ProjectRegistry};
use system::System;
use task_store_sqlite::SqliteStores;
use tasks_application::{Importer, Service};
use tasks_cli::Context;
use tasks_config::TaskloomConfig;

/// Every crate's en-US catalog the binary's diagnostics come from: annals'
/// crates the binary links (annals-diagnostic defines no codes), then its
/// own.
fn catalog() -> Result<presentation::Catalog, presentation::CatalogError> {
    tasks_cli::catalog(&[
        annals::EN_US,
        annals_store_sql::EN_US,
        project_cli::EN_US,
        project_application::EN_US,
        registration_cli::EN_US,
        registration_application::EN_US,
        agent_configs::EN_US,
        file_lock::EN_US,
        project_registry::EN_US,
        task_store_sqlite::EN_US,
        jsonl_task_log::EN_US,
        tasks_config::EN_US,
        tasks_mcp::EN_US,
        tasks_watch::EN_US,
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
    Mcp(tasks_mcp::Group),
    #[command(flatten)]
    Watch(tasks_watch::Group),
    #[command(flatten)]
    Manage(registration_cli::Group),
}

/// The tasks use cases over the real adapters, configured from the work
/// root's config, as one call or command needs them. `vars` are the
/// environment's, as the config reads them.
fn service(
    fs: &Arc<dyn Fs>,
    env: &Environment,
    vars: Vec<(String, String)>,
) -> Service<ProjectRegistry<HarpNames>, SqliteStores, HarpNames, System, System, TaskloomConfig> {
    let identity = ProjectRegistry::new(Arc::clone(fs), env.clone(), HarpNames);
    // The work root's config, where there is a work root: a command that
    // needs one fails on resolving it, and says why.
    let config = TaskloomConfig::new(env.home.clone(), identity.work_root().ok(), vars);
    Service::new(identity, SqliteStores::default(), HarpNames, System, System)
        .with_configuration(config)
}

/// `taskloom import`: the JSONL task logs under `env`'s home imported into
/// each project's store, by an importer built for this command alone.
fn import(
    runtime: &tokio::runtime::Runtime,
    fs: &Arc<dyn Fs>,
    env: &Environment,
    project: Option<String>,
    format: presentation::Format,
    catalog: &presentation::Catalog,
) -> ExitCode {
    let mut importer = Importer::new(
        ProjectRegistry::new(Arc::clone(fs), env.clone(), HarpNames),
        SqliteStores::default(),
        System,
        JsonlTaskLogDir::in_home(env.home.as_deref()),
    );
    ExitCode::from(runtime.block_on(tasks_cli::run_import(
        project,
        format,
        catalog,
        &mut importer,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )))
}

/// `taskloom mcp`: serve the task tools on stdio until the client goes
/// away, each call on a service of its own.
fn serve_mcp(
    runtime: tokio::runtime::Runtime,
    fs: Arc<dyn Fs>,
    env: Environment,
    settings: tasks_mcp::Settings,
    catalog: presentation::Catalog,
) -> ExitCode {
    let catalog = Arc::new(catalog);
    let vars = utf8_vars();
    let tools = tasks_mcp::TaskTools::new(
        move || service(&fs, &env, vars.clone()),
        settings,
        Arc::clone(&catalog),
    );
    let served = runtime.block_on(tools.serve_stdio());
    // Nothing is left to wait for: a stdin read still blocked is
    // abandoned, not joined.
    runtime.shutdown_background();
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}: {}", tasks_cli::PROG, catalog.message(&e));
            ExitCode::FAILURE
        }
    }
}

/// Resolves at SIGINT or SIGTERM (Ctrl-C alone off Unix), listening from
/// the moment it is made, so a signal before the first await is not lost
/// to the default disposition. Made on the runtime.
fn interrupted() -> std::io::Result<impl std::future::Future<Output = ()>> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        Ok(std::future::poll_fn(move |cx| {
            let interrupted = interrupt.poll_recv(cx).is_ready();
            let terminated = terminate.poll_recv(cx).is_ready();
            if interrupted || terminated {
                std::task::Poll::Ready(())
            } else {
                std::task::Poll::Pending
            }
        }))
    }
    #[cfg(not(unix))]
    {
        Ok(async {
            let _ = tokio::signal::ctrl_c().await;
        })
    }
}

/// `taskloom watch`: the project's changes on stdout, polled every
/// debounce, until interrupted.
fn watch<W: tasks_application::WatchService>(
    runtime: &tokio::runtime::Runtime,
    service: &mut W,
    settings: &tasks_watch::Settings,
    catalog: &presentation::Catalog,
) -> ExitCode {
    let code = runtime.block_on(async {
        let stop = match interrupted() {
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

/// `taskloom manage`: `taskloom mcp` registered in the agents'
/// configurations under `home`.
fn manage(
    fs: Arc<dyn Fs>,
    words: Vec<String>,
    format: presentation::Format,
    home: Option<PathBuf>,
    catalog: &presentation::Catalog,
) -> ExitCode {
    let settings = registration_cli::Settings {
        format,
        stdout_is_terminal: io::stdout().is_terminal(),
        prog: tasks_cli::PROG,
    };
    let configs = agent_configs::AgentConfigFiles {
        fs,
        home,
        path: std::env::var_os("PATH"),
        waits: file_lock::Waits::DEFAULT,
    };
    let mut registrations = registration_application::Registrations::taskloom(configs);
    ExitCode::from(registration_cli::run(
        words,
        &settings,
        &mut registrations,
        catalog,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    ))
}

/// The `project` group, over the project registry and the SQLite store's
/// integrity check.
fn project_group(
    fs: Arc<dyn Fs>,
    env: Environment,
    group: project_cli::Group,
    settings: &project_cli::Settings,
    catalog: &presentation::Catalog,
) -> ExitCode {
    let mut projects = Projects::new(
        ProjectRegistry::new(fs, env, HarpNames),
        SqliteStores::default(),
    );
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
        // the project registry's `index.yaml.lock`, and `manage`'s home
        // sidecar for the agent configuration it rewrites, with the reap
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
    // Stdout and stderr are locked for one call at a time: the MCP
    // server's writes come from another thread.
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
    let cwd = std::env::current_dir().map_err(|e| e.to_string());
    let Some(fs) = filesystem(&cwd) else {
        return ExitCode::FAILURE;
    };
    let env = Environment {
        home: std::env::var_os("HOME").map(PathBuf::from),
        cwd,
        root: std::env::var_os("CTXLOOM_ROOT").map(PathBuf::from),
        project_id: std::env::var("CTXLOOM_PROJECT_ID").ok(),
    };
    let group = match command {
        Command::Tasks(command) => {
            let mut service = service(&fs, &env, utf8_vars());
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
            import(&runtime, &fs, &env, project, format, &catalog)
        }
        Mounted::Project(group) => {
            let settings = project_cli::Settings {
                format,
                project,
                prog: tasks_cli::PROG,
            };
            project_group(fs, env, group, &settings, &catalog)
        }
        Mounted::Manage(registration_cli::Group::Manage { words }) => {
            manage(fs, words, format, env.home, &catalog)
        }
        Mounted::Mcp(tasks_mcp::Group::Mcp) => {
            let settings = tasks_mcp::Settings {
                project,
                session: context.session,
                name: tasks_cli::PROG.to_string(),
                version: context.version,
            };
            serve_mcp(runtime, fs, env, settings, catalog)
        }
        Mounted::Watch(tasks_watch::Group::Watch) => {
            let settings = tasks_watch::Settings {
                format,
                project,
                prog: tasks_cli::PROG,
            };
            watch(
                &runtime,
                &mut service(&fs, &env, utf8_vars()),
                &settings,
                &catalog,
            )
        }
    }
}

/// The one filesystem every adapter is given, relative paths resolved
/// against `cwd`, the working directory `main` read once. With none
/// readable (it was deleted), the top: the registry refuses on the working
/// directory's own error before it names a relative path.
fn filesystem(cwd: &Result<PathBuf, String>) -> Option<Arc<dyn Fs>> {
    match Os::new(cwd.as_deref().unwrap_or(Path::new("/"))) {
        Ok(fs) => Some(Arc::new(fs)),
        Err(e) => {
            eprintln!("{}: {e}", tasks_cli::PROG);
            None
        }
    }
}

/// The environment's variables whose names and values are UTF-8.
fn utf8_vars() -> Vec<(String, String)> {
    std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect()
}

#[cfg(test)]
#[path = "main.test.rs"]
mod tests;
