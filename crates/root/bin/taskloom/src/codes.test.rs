//! The codes the binary's acceptance tests expect on its output, each read
//! from the typed error that answers it, so no test spells a crate's name
//! (the owner's ruling, 2026-11-02: names by reflection). A field a code
//! does not depend on is left empty.

use angzarr_diagnostic::Diagnostic;
use tasks::schema::SchemaRejection;
use tasks::TaskRejection;
use tasks_application::{
    AppError, NoJsonlLog, PruneRefusal, QueryError, RulesRefusal, TagRefusal, Why,
};

/// `d`'s code, owned.
fn of(d: &dyn Diagnostic) -> String {
    d.code().to_string()
}

fn none() -> String {
    String::new()
}

// ------------------------------------------------------------ tasks

pub fn text_required() -> String {
    of(&TaskRejection::TextRequired)
}

pub fn trigger_required() -> String {
    of(&TaskRejection::TriggerRequired)
}

pub fn tag_required() -> String {
    of(&TaskRejection::TagRequired)
}

pub fn harp_id_required() -> String {
    of(&TaskRejection::HarpIdRequired)
}

pub fn no_task() -> String {
    of(&TaskRejection::NoTask)
}

pub fn status_reserved() -> String {
    of(&TaskRejection::StatusReserved { status: none() })
}

pub fn trigger_with_status() -> String {
    of(&TaskRejection::TriggerWithStatus {
        status: none(),
        defers: none(),
    })
}

pub fn schema_not_config() -> String {
    of(&SchemaRejection::NotConfig { tag: none() })
}

// ------------------------------------------------------------ tasks-application

pub fn blank() -> String {
    of(&AppError::Blank { field: "" })
}

pub fn import_displaced() -> String {
    of(&Why::Displaced)
}

pub fn import_task_not_imported() -> String {
    of(&Why::TaskNotImported)
}

pub fn no_jsonl_log() -> String {
    of(&NoJsonlLog { project: none() })
}

pub fn not_found() -> String {
    of(&AppError::NotFound {
        harp_ids: Vec::new(),
    })
}

pub fn permit_in_use() -> String {
    of(&AppError::PermitInUse {
        permit: none(),
        harp_ids: Vec::new(),
    })
}

pub fn prune_age_unreadable() -> String {
    of(&PruneRefusal::AgeUnreadable { age: none() })
}

pub fn tag_blank() -> String {
    of(&TagRefusal::Blank)
}

pub fn tag_operator_word() -> String {
    of(&TagRefusal::OperatorWord { tag: none() })
}

pub fn tag_reserved_namespace() -> String {
    of(&TagRefusal::ReservedNamespace {
        tag: none(),
        namespace: none(),
    })
}

pub fn tag_unparseable() -> String {
    of(&TagRefusal::Unparseable {
        tag: none(),
        detail: none(),
    })
}

pub fn tag_query() -> String {
    of(&QueryError {
        query: none(),
        postfix: none(),
    })
}

pub fn tag_rules() -> String {
    of(&RulesRefusal { findings: none() })
}

pub fn trigger_and_clear() -> String {
    of(&AppError::TriggerAndClear)
}

pub fn unknown_status_class() -> String {
    of(&AppError::UnknownStatusClass { status: none() })
}

// ------------------------------------------------------------ the frontends

pub fn import_incomplete() -> String {
    of(&tasks_cli::CliError::Incomplete {
        stopped: 0,
        lost: 0,
    })
}

pub fn lint_violations() -> String {
    of(&tasks_cli::CliError::LintViolations { count: 0 })
}

pub fn status_reversed() -> String {
    of(&tasks_cli::CliError::StatusReversed { command: none() })
}

pub fn usage() -> String {
    of(&tasks_cli::CliError::Usage { detail: none() })
}

pub fn watch_no_project() -> String {
    of(&tasks_watch::WatchError::NoProject)
}

// ------------------------------------------------------------ the adapters

pub fn key_moved() -> String {
    of(&tasks_config::ConfigError::KeyMoved {
        source: none(),
        key: none(),
    })
}

pub fn not_an_override() -> String {
    of(&tasks_config::ConfigError::NotAnOverride { entry: none() })
}

pub fn invalid_id() -> String {
    of(&project_registry::RegistryError::InvalidId {
        id: none(),
        origin: none(),
        reason: none(),
    })
}

pub fn unknown_project() -> String {
    of(&project_application::ProjectError::UnknownProject { id: none() })
}

pub fn log_malformed() -> String {
    of(&jsonl_task_log::LogError::Malformed {
        path: none(),
        line: 0,
        detail: none(),
    })
}

pub fn log_unknown_op() -> String {
    of(&jsonl_task_log::LogError::UnknownOp {
        path: none(),
        line: 0,
        op: none(),
    })
}
