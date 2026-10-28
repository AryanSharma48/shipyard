use crate::adapter::RuntimeAdapter;
use crate::error::ApiError;
use crate::models::{Deployment, DeploymentStatus, TargetType};
use crate::store::ProjectStore;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

/// Executes the end-to-end deployment lifecycle for a project commit.
///
/// State Machine:
/// Queued -> Deploying -> (RuntimeAdapter) -> Verifying -> Healthy | Failed
pub async fn execute_deployment(
    store: &Arc<dyn ProjectStore>,
    adapter: &Arc<dyn RuntimeAdapter>,
    project_id: Uuid,
    commit_sha: &str,
    target_type: Option<TargetType>,
) -> Result<Deployment, ApiError> {
    // 1. Verify target project exists
    store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Project with ID '{project_id}' not found")))?;

    // 2. Initialize Queued deployment record
    let now = Utc::now();
    let deployment = Deployment {
        id: Uuid::new_v4(),
        project_id,
        commit_sha: commit_sha.trim().to_string(),
        target_type: target_type.unwrap_or_default(),
        status: DeploymentStatus::Queued,
        created_at: now,
        started_at: None,
        finished_at: None,
        failure_reason: None,
    };

    let saved = store.create_deployment(deployment).await?;

    // 3. Transition: Queued -> Deploying
    let deploying = store
        .update_deployment_status(saved.id, DeploymentStatus::Deploying, None)
        .await?;

    // 4. Trigger runtime adapter
    match adapter.deploy(&deploying).await {
        Ok(()) => {
            // Transition: Deploying -> Verifying -> Healthy
            store
                .update_deployment_status(saved.id, DeploymentStatus::Verifying, None)
                .await?;

            let healthy = store
                .update_deployment_status(saved.id, DeploymentStatus::Healthy, None)
                .await?;

            Ok(healthy)
        }
        Err(err) => {
            let reason = err.to_string();
            let failed = store
                .update_deployment_status(saved.id, DeploymentStatus::Failed, Some(reason))
                .await?;

            Ok(failed)
        }
    }
}
