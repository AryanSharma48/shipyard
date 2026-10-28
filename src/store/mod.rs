use crate::error::ApiError;
use crate::models::{Deployment, DeploymentStatus, Project};
use chrono::Utc;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ProjectStore: Send + Sync {
    fn create_project(&self, project: Project) -> BoxFuture<'_, Result<Project, ApiError>>;
    fn get_project(&self, id: Uuid) -> BoxFuture<'_, Result<Option<Project>, ApiError>>;
    fn list_projects(&self) -> BoxFuture<'_, Result<Vec<Project>, ApiError>>;

    fn create_deployment(
        &self,
        deployment: Deployment,
    ) -> BoxFuture<'_, Result<Deployment, ApiError>>;
    fn get_deployment(&self, id: Uuid) -> BoxFuture<'_, Result<Option<Deployment>, ApiError>>;
    fn list_deployments_by_project(
        &self,
        project_id: Uuid,
    ) -> BoxFuture<'_, Result<Vec<Deployment>, ApiError>>;
    fn update_deployment_status(
        &self,
        id: Uuid,
        status: DeploymentStatus,
        failure_reason: Option<String>,
    ) -> BoxFuture<'_, Result<Deployment, ApiError>>;
}

#[derive(Clone, Default)]
pub struct MemoryProjectStore {
    projects: Arc<RwLock<HashMap<Uuid, Project>>>,
    deployments: Arc<RwLock<HashMap<Uuid, Deployment>>>,
}

impl MemoryProjectStore {
    pub fn new() -> Self {
        Self {
            projects: Arc::new(RwLock::new(HashMap::new())),
            deployments: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl ProjectStore for MemoryProjectStore {
    fn create_project(&self, project: Project) -> BoxFuture<'_, Result<Project, ApiError>> {
        Box::pin(async move {
            let mut lock = self.projects.write().await;
            lock.insert(project.id, project.clone());
            Ok(project)
        })
    }

    fn get_project(&self, id: Uuid) -> BoxFuture<'_, Result<Option<Project>, ApiError>> {
        Box::pin(async move {
            let lock = self.projects.read().await;
            Ok(lock.get(&id).cloned())
        })
    }

    fn list_projects(&self) -> BoxFuture<'_, Result<Vec<Project>, ApiError>> {
        Box::pin(async move {
            let lock = self.projects.read().await;
            let mut list: Vec<Project> = lock.values().cloned().collect();
            list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            Ok(list)
        })
    }

    fn create_deployment(
        &self,
        deployment: Deployment,
    ) -> BoxFuture<'_, Result<Deployment, ApiError>> {
        Box::pin(async move {
            let mut lock = self.deployments.write().await;
            lock.insert(deployment.id, deployment.clone());
            Ok(deployment)
        })
    }

    fn get_deployment(&self, id: Uuid) -> BoxFuture<'_, Result<Option<Deployment>, ApiError>> {
        Box::pin(async move {
            let lock = self.deployments.read().await;
            Ok(lock.get(&id).cloned())
        })
    }

    fn list_deployments_by_project(
        &self,
        project_id: Uuid,
    ) -> BoxFuture<'_, Result<Vec<Deployment>, ApiError>> {
        Box::pin(async move {
            let lock = self.deployments.read().await;
            let mut list: Vec<Deployment> = lock
                .values()
                .filter(|d| d.project_id == project_id)
                .cloned()
                .collect();
            list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            Ok(list)
        })
    }

    fn update_deployment_status(
        &self,
        id: Uuid,
        new_status: DeploymentStatus,
        failure_reason: Option<String>,
    ) -> BoxFuture<'_, Result<Deployment, ApiError>> {
        Box::pin(async move {
            let mut lock = self.deployments.write().await;
            let deployment = lock.get_mut(&id).ok_or_else(|| {
                ApiError::NotFound(format!("Deployment with ID '{id}' not found"))
            })?;

            if !deployment.status.can_transition_to(&new_status) {
                return Err(ApiError::Conflict(format!(
                    "Cannot transition deployment '{id}' from '{:?}' to '{:?}'",
                    deployment.status, new_status
                )));
            }

            let now = Utc::now();
            deployment.status = new_status;

            if new_status == DeploymentStatus::Deploying && deployment.started_at.is_none() {
                deployment.started_at = Some(now);
            }

            if matches!(
                new_status,
                DeploymentStatus::Healthy | DeploymentStatus::Failed | DeploymentStatus::RolledBack
            ) {
                deployment.finished_at = Some(now);
            }

            if failure_reason.is_some() {
                deployment.failure_reason = failure_reason;
            }

            Ok(deployment.clone())
        })
    }
}
