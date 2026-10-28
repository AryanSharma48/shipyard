use crate::error::ApiError;
use crate::models::{Build, BuildStatus, Deployment, DeploymentStatus, Project};
use chrono::Utc;
use std::collections::{HashMap, HashSet};
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
    fn find_project_by_repo(
        &self,
        repo_identifier: &str,
    ) -> BoxFuture<'_, Result<Option<Project>, ApiError>>;

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

    fn create_build(&self, build: Build) -> BoxFuture<'_, Result<Build, ApiError>>;
    fn get_build(&self, id: Uuid) -> BoxFuture<'_, Result<Option<Build>, ApiError>>;
    fn list_builds_by_project(
        &self,
        project_id: Uuid,
    ) -> BoxFuture<'_, Result<Vec<Build>, ApiError>>;
    fn update_build_status(
        &self,
        id: Uuid,
        status: BuildStatus,
        image_digest: Option<String>,
        failure_reason: Option<String>,
    ) -> BoxFuture<'_, Result<Build, ApiError>>;

    fn is_delivery_processed(&self, delivery_id: &str) -> BoxFuture<'_, Result<bool, ApiError>>;
    fn record_delivery(&self, delivery_id: String) -> BoxFuture<'_, Result<(), ApiError>>;
}

#[derive(Clone, Default)]
pub struct MemoryProjectStore {
    projects: Arc<RwLock<HashMap<Uuid, Project>>>,
    deployments: Arc<RwLock<HashMap<Uuid, Deployment>>>,
    builds: Arc<RwLock<HashMap<Uuid, Build>>>,
    processed_deliveries: Arc<RwLock<HashSet<String>>>,
}

impl MemoryProjectStore {
    pub fn new() -> Self {
        Self {
            projects: Arc::new(RwLock::new(HashMap::new())),
            deployments: Arc::new(RwLock::new(HashMap::new())),
            builds: Arc::new(RwLock::new(HashMap::new())),
            processed_deliveries: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    fn normalize_repo_url(url: &str) -> String {
        let trimmed = url.trim().trim_end_matches('/').trim_end_matches(".git");
        trimmed.to_lowercase()
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

    fn find_project_by_repo(
        &self,
        repo_identifier: &str,
    ) -> BoxFuture<'_, Result<Option<Project>, ApiError>> {
        let target = Self::normalize_repo_url(repo_identifier);
        Box::pin(async move {
            let lock = self.projects.read().await;
            for p in lock.values() {
                let norm = Self::normalize_repo_url(&p.repo_url);
                if norm == target || norm.ends_with(&format!("/{}", target)) {
                    return Ok(Some(p.clone()));
                }
            }
            Ok(None)
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

    fn create_build(&self, build: Build) -> BoxFuture<'_, Result<Build, ApiError>> {
        Box::pin(async move {
            let mut lock = self.builds.write().await;
            lock.insert(build.id, build.clone());
            Ok(build)
        })
    }

    fn get_build(&self, id: Uuid) -> BoxFuture<'_, Result<Option<Build>, ApiError>> {
        Box::pin(async move {
            let lock = self.builds.read().await;
            Ok(lock.get(&id).cloned())
        })
    }

    fn list_builds_by_project(
        &self,
        project_id: Uuid,
    ) -> BoxFuture<'_, Result<Vec<Build>, ApiError>> {
        Box::pin(async move {
            let lock = self.builds.read().await;
            let mut list: Vec<Build> = lock
                .values()
                .filter(|b| b.project_id == project_id)
                .cloned()
                .collect();
            list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            Ok(list)
        })
    }

    fn update_build_status(
        &self,
        id: Uuid,
        new_status: BuildStatus,
        image_digest: Option<String>,
        failure_reason: Option<String>,
    ) -> BoxFuture<'_, Result<Build, ApiError>> {
        Box::pin(async move {
            let mut lock = self.builds.write().await;
            let build = lock
                .get_mut(&id)
                .ok_or_else(|| ApiError::NotFound(format!("Build with ID '{id}' not found")))?;

            if !build.status.can_transition_to(&new_status) {
                return Err(ApiError::Conflict(format!(
                    "Cannot transition build '{id}' from '{:?}' to '{:?}'",
                    build.status, new_status
                )));
            }

            let now = Utc::now();
            build.status = new_status;

            if new_status == BuildStatus::Building && build.started_at.is_none() {
                build.started_at = Some(now);
            }

            if matches!(
                new_status,
                BuildStatus::Succeeded | BuildStatus::Failed | BuildStatus::Cancelled
            ) {
                build.finished_at = Some(now);
            }

            if image_digest.is_some() {
                build.image_digest = image_digest;
            }

            if failure_reason.is_some() {
                build.failure_reason = failure_reason;
            }

            Ok(build.clone())
        })
    }

    fn is_delivery_processed(&self, delivery_id: &str) -> BoxFuture<'_, Result<bool, ApiError>> {
        let key = delivery_id.to_string();
        Box::pin(async move {
            let lock = self.processed_deliveries.read().await;
            Ok(lock.contains(&key))
        })
    }

    fn record_delivery(&self, delivery_id: String) -> BoxFuture<'_, Result<(), ApiError>> {
        Box::pin(async move {
            let mut lock = self.processed_deliveries.write().await;
            lock.insert(delivery_id);
            Ok(())
        })
    }
}
