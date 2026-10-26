//! Each tool's arguments translated into a use case's request, and its
//! answer into the tool's result shape.

use presentation::Catalog;
use rmcp::model::Tool;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tasks_application::{
    ListQuery, Listing, NewTask, Reach, Scope, StatusChange, Summary, TagChange, TextEdit, Written,
};
use tasks_presentation::{CompactJson, RowJson, SummaryJson, TaskJson};

use crate::{McpError, McpNotice, Settings, TOOLS};

pub const TASK_LIST: &str = "task_list";
pub const TASK_ADD: &str = "task_add";
pub const TASK_SET_STATUS: &str = "task_set_status";
pub const TASK_EDIT: &str = "task_edit";
pub const TASK_TAG: &str = "task_tag";

/// The tools `TOOLS` describes.
pub fn offered() -> Vec<Tool> {
    serde_json::from_str(TOOLS).expect("TOOLS is a JSON array of tools")
}

// A string argument left out (or null) is `None`; one given is `Some`,
// passed on as given: an explicit "" is never read as one left out, and the
// use case refuses it as blank, as it refuses the command line's.

/// `task_list`'s arguments.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct ListArgs {
    statuses: Option<Vec<String>>,
    include_completed: bool,
    term: Option<String>,
    tag_query: Option<String>,
    include_summary: bool,
    global: bool,
    sort: Option<String>,
    compact: bool,
    limit: i64,
}

/// `task_add`'s arguments.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddArgs {
    text: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    trigger: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

/// `task_set_status`'s arguments.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetStatusArgs {
    harp_id: String,
    status: String,
    #[serde(default)]
    trigger: Option<String>,
}

/// `task_edit`'s arguments.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    harp_id: String,
    text: String,
}

/// `task_tag`'s arguments.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TagArgs {
    harp_id: String,
    #[serde(default)]
    add: Option<Vec<String>>,
    #[serde(default)]
    remove: Option<Vec<String>>,
}

fn arguments<T: DeserializeOwned>(tool: &str, arguments: Value) -> Result<T, McpError> {
    serde_json::from_value(arguments).map_err(|e| McpError::Arguments {
        tool: tool.to_string(),
        detail: e.to_string(),
    })
}

/// What `task_list` asks for: the listing, whether its rows are compact,
/// and whether a summary of what it reached comes with them.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ListRequest {
    pub query: ListQuery,
    pub compact: bool,
    pub summary: bool,
}

impl ListRequest {
    /// The scope the summary counts, once the listing has read `reach`:
    /// the same projects its rows came from.
    pub fn summary_scope(&self, reach: &Reach) -> Scope {
        Scope {
            project: self.query.project.clone(),
            global: reach.is_every(),
            config_set: self.query.config_set.clone(),
        }
    }
}

/// The listing `task_list`'s `arguments` ask for. A limit of 0 or less
/// is none.
pub fn list_request(arguments: Value, settings: &Settings) -> Result<ListRequest, McpError> {
    let args: ListArgs = self::arguments(TASK_LIST, arguments)?;
    Ok(ListRequest {
        query: ListQuery {
            project: settings.project.clone(),
            statuses: args.statuses.unwrap_or_default(),
            all: args.include_completed,
            tag_query: args.tag_query,
            config_set: Vec::new(),
            term: args.term,
            global: args.global,
            limit: usize::try_from(args.limit).unwrap_or(0),
            sort: args.sort,
        },
        compact: args.compact,
        summary: args.include_summary,
    })
}

/// The task `task_add`'s `arguments` ask for, added in the server's
/// session.
pub fn new_task(arguments: Value, settings: &Settings) -> Result<NewTask, McpError> {
    let args: AddArgs = self::arguments(TASK_ADD, arguments)?;
    Ok(NewTask {
        project: settings.project.clone(),
        text: args.text,
        status: args.status,
        trigger: args.trigger,
        tags: args.tags.unwrap_or_default(),
        origin_session: settings.session(),
        config_set: Vec::new(),
    })
}

/// The move `task_set_status`'s `arguments` ask for.
pub fn status_change(arguments: Value, settings: &Settings) -> Result<StatusChange, McpError> {
    let args: SetStatusArgs = self::arguments(TASK_SET_STATUS, arguments)?;
    Ok(StatusChange {
        project: settings.project.clone(),
        harp_id: args.harp_id,
        status: args.status,
        trigger: args.trigger,
        config_set: Vec::new(),
    })
}

/// The edit `task_edit`'s `arguments` ask for.
pub fn text_edit(arguments: Value, settings: &Settings) -> Result<TextEdit, McpError> {
    let args: EditArgs = self::arguments(TASK_EDIT, arguments)?;
    Ok(TextEdit {
        project: settings.project.clone(),
        harp_id: args.harp_id,
        text: args.text,
        config_set: Vec::new(),
    })
}

/// The retag `task_tag`'s `arguments` ask for.
pub fn tag_change(arguments: Value, settings: &Settings) -> Result<TagChange, McpError> {
    let args: TagArgs = self::arguments(TASK_TAG, arguments)?;
    Ok(TagChange {
        project: settings.project.clone(),
        harp_id: args.harp_id,
        add: args.add.unwrap_or_default(),
        remove: args.remove.unwrap_or_default(),
        config_set: Vec::new(),
    })
}

/// A listed task: the task and the project it is in; or, compact, its
/// compact row.
type Row<'a> = RowJson<'a, TaskJson<'a>>;
type CompactRow<'a> = RowJson<'a, CompactJson<'a>>;

/// `task_list`'s result. `tasks` is null when the rows are compact.
#[derive(Serialize)]
struct ListResult<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_dir: Option<&'a str>,
    tasks: Option<Vec<Row<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<SummaryJson<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    compact_tasks: Vec<CompactRow<'a>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    global: bool,
    #[serde(skip_serializing_if = "is_zero")]
    project_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    notice: Option<String>,
    #[serde(skip_serializing_if = "is_zero")]
    hidden_completed: usize,
    #[serde(skip_serializing_if = "is_zero")]
    hidden_deferred: usize,
    #[serde(skip_serializing_if = "is_zero")]
    omitted_by_limit: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    priority_warning: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// A write's result, for `task_add`, `task_set_status`, `task_edit` and
/// `task_tag` alike.
#[derive(Serialize)]
struct WriteResult<'a> {
    task: TaskJson<'a>,
    project_id: &'a str,
    project_dir: &'a str,
}

/// `listing` as `task_list` answers it, its rows compact if `compact`,
/// with `summary` if one was asked for. Where no project was read, a
/// `warning` says so; where every project was, `global` and
/// `project_count` say so, and `notice` says why when no project here
/// led to it (an explicit `global` needs no notice).
pub fn listed(
    listing: &Listing,
    compact: bool,
    summary: Option<&Summary>,
    catalog: &Catalog,
) -> Value {
    let project = listing.project();
    let (rows, compact_rows) = match compact {
        false => (
            Some(
                listing
                    .ranked()
                    .map(|(row, priority)| {
                        RowJson::of(
                            TaskJson::of(&row.task).with_priority(priority),
                            &row.project,
                        )
                    })
                    .collect(),
            ),
            Vec::new(),
        ),
        true => (
            None,
            listing
                .rows
                .iter()
                .map(|row| RowJson::of(CompactJson::of(&row.task), &row.project))
                .collect(),
        ),
    };
    let (global, project_count, notice, warning) = match &listing.reach {
        Reach::Project(_) => (false, 0, None, None),
        Reach::NoProject => (false, 0, None, Some(catalog.message(&McpNotice::NoProject))),
        Reach::Every { projects, fallback } => (
            true,
            *projects,
            fallback.as_ref().map(|directory| {
                catalog.message(&McpNotice::EveryProject {
                    directory: directory.clone(),
                })
            }),
            None,
        ),
    };
    let result = ListResult {
        project_id: project.map(|p| p.id.as_str()),
        project_dir: project.map(|p| p.root.as_str()),
        tasks: rows,
        summary: summary.map(SummaryJson::of),
        compact_tasks: compact_rows,
        global,
        project_count,
        notice,
        hidden_completed: listing.hidden.completed,
        hidden_deferred: listing.hidden.deferred,
        omitted_by_limit: listing.omitted,
        priority_warning: listing
            .priority_notes
            .iter()
            .map(|note| catalog.message(note))
            .collect::<Vec<_>>()
            .join("\n"),
        warning,
    };
    serde_json::to_value(result).unwrap_or_default()
}

/// `written` as a write tool answers it.
pub fn written(written: &Written) -> Value {
    let result = WriteResult {
        task: TaskJson::of(&written.task),
        project_id: &written.project.id,
        project_dir: &written.project.root,
    };
    serde_json::to_value(result).unwrap_or_default()
}

#[cfg(test)]
#[path = "tools.test.rs"]
mod tests;
