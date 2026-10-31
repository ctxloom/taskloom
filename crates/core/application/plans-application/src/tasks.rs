//! The project's tasks as plans need them, from the tasks use cases: the
//! [`Tasks`] port over a [`TaskService`], so the plans use cases read and
//! retag tasks only as any frontend does.

use angzarr_embedded_diagnostic::Report;
use ports::Tasks;
use tasks_application::{AppError, ListQuery, ShowQuery, Tag, TagChange, TaskService};

/// The tasks of the project `project` pins (the work root's when none),
/// through `service`. The dead letters its writes report are kept for the
/// plans use case to hand up.
pub struct ProjectTasks<S> {
    pub service: S,
    pub project: Option<String>,
    dead_letters: Vec<String>,
}

impl<S> ProjectTasks<S> {
    pub fn new(service: S, project: Option<String>) -> Self {
        ProjectTasks {
            service,
            project,
            dead_letters: Vec::new(),
        }
    }
}

impl<S: TaskService> Tasks for ProjectTasks<S> {
    async fn exists(&mut self, harp_id: &str) -> Result<bool, Report> {
        let query = ShowQuery {
            project: self.project.clone(),
            global: false,
            harp_ids: vec![harp_id.to_string()],
        };
        match self.service.show(query).await {
            Ok(details) => Ok(!details.rows.is_empty()),
            Err(AppError::NotFound { .. } | AppError::NoProject) => Ok(false),
            Err(error) => Err(Report::of(&error)),
        }
    }

    async fn tagged(&mut self, key: &str) -> Result<Vec<(String, String)>, Report> {
        let query = ListQuery {
            project: self.project.clone(),
            all: true,
            ..ListQuery::default()
        };
        let listing = match self.service.list(query).await {
            Ok(listing) => listing,
            Err(AppError::NoProject) => return Ok(Vec::new()),
            Err(error) => return Err(Report::of(&error)),
        };
        Ok(listing
            .tasks()
            .flat_map(|task| {
                task.tags
                    .iter()
                    .filter(|tag| tag.target() == key)
                    .map(|tag| (task.harp_id.clone(), tag.value.clone().unwrap_or_default()))
            })
            .collect())
    }

    async fn untag(&mut self, harp_id: &str, key: &str, value: &str) -> Result<(), Report> {
        let (namespace, key) = match key.split_once(':') {
            Some((namespace, key)) => (Some(namespace.to_string()), key.to_string()),
            None => (None, key.to_string()),
        };
        let tag = Tag {
            namespace,
            key,
            value: Some(value.to_string()),
        };
        let change = TagChange {
            project: self.project.clone(),
            harp_id: harp_id.to_string(),
            add: Vec::new(),
            remove: vec![tag.to_string()],
            request_id: None,
        };
        match self.service.tag(change).await {
            Ok(written) => {
                self.dead_letters
                    .extend(written.dead_letters.into_iter().map(|l| l.line));
                Ok(())
            }
            Err(error) => {
                self.dead_letters
                    .extend(error.dead_letters().iter().map(|l| l.line.clone()));
                Err(Report::of(&error))
            }
        }
    }

    fn dead_letters(&mut self) -> Vec<String> {
        std::mem::take(&mut self.dead_letters)
    }
}

#[cfg(test)]
#[path = "tasks.test.rs"]
mod tests;
