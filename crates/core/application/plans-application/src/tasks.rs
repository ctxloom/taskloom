//! The project's tasks and rules as plans need them, from the tasks use
//! cases: the [`Tasks`] port over a [`TaskService`] and [`SchemaService`],
//! so the plans read the tasks only as any frontend does.

use angzarr_diagnostic::Report;
use ports::Tasks;
use tag_rules::StoreRules;
use tasks_application::{AppError, SchemaService, ShowQuery, TaskService};

/// The tasks of the project `project` pins (the work root's when none),
/// through `service`.
pub struct ProjectTasks<S> {
    pub service: S,
    pub project: Option<String>,
}

impl<S> ProjectTasks<S> {
    pub fn new(service: S, project: Option<String>) -> Self {
        ProjectTasks { service, project }
    }
}

impl<S: TaskService + SchemaService> Tasks for ProjectTasks<S> {
    async fn exists(&mut self, harp_id: &str) -> Result<bool, Report> {
        let query = ShowQuery {
            project: self.project.clone(),
            harp_ids: vec![harp_id.to_string()],
        };
        match self.service.show(query).await {
            Ok(details) => Ok(!details.rows.is_empty()),
            Err(AppError::NotFound { .. } | AppError::NoProject) => Ok(false),
            Err(error) => Err(Report::of(&error)),
        }
    }

    async fn rules(&mut self) -> Result<Vec<String>, Report> {
        match self.service.schema_export(self.project.clone()).await {
            Ok(rules) => Ok(rules),
            // No project yet, so no store: the rules a new store starts
            // with.
            Err(AppError::NoProject) => Ok(StoreRules::default().export()),
            Err(error) => Err(Report::of(&error)),
        }
    }
}

#[cfg(test)]
#[path = "tasks.test.rs"]
mod tests;
