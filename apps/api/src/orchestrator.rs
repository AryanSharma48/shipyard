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
/// Queued -> Deploying -> Verifying -> Healthy
/// Deploying / Verifying -> Failed (-> RollingBack -> RolledBack when a prior Healthy release exists)
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

    // Check if there is a previous Healthy release to revert to on failure
    let previous_healthy = store
        .list_deployments_by_project(project_id)
        .await?
        .into_iter()
        .find(|d| d.status == DeploymentStatus::Healthy);

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

    // 4. Trigger runtime adapter deploy
    if let Err(err) = adapter.deploy(&deploying).await {
        return handle_failure_and_auto_rollback(
            store,
            adapter,
            saved.id,
            err.to_string(),
            previous_healthy.as_ref(),
        )
        .await;
    }

    // 5. Transition: Deploying -> Verifying and run health gate
    let verifying = store
        .update_deployment_status(saved.id, DeploymentStatus::Verifying, None)
        .await?;

    match adapter.check_health(&verifying).await {
        Ok(true) => {
            let healthy = store
                .update_deployment_status(saved.id, DeploymentStatus::Healthy, None)
                .await?;
            Ok(healthy)
        }
        Ok(false) => {
            let reason = format!(
                "Health check verification failed for deployment '{}'",
                saved.id
            );
            handle_failure_and_auto_rollback(
                store,
                adapter,
                saved.id,
                reason,
                previous_healthy.as_ref(),
            )
            .await
        }
        Err(err) => {
            let reason = format!("Health check error: {err}");
            handle_failure_and_auto_rollback(
                store,
                adapter,
                saved.id,
                reason,
                previous_healthy.as_ref(),
            )
            .await
        }
    }
}

/// Transitions a Failed deployment through `RollingBack -> RolledBack`, restoring the prior healthy release.
pub async fn execute_rollback(
    store: &Arc<dyn ProjectStore>,
    adapter: &Arc<dyn RuntimeAdapter>,
    deployment_id: Uuid,
) -> Result<Deployment, ApiError> {
    let deployment = store.get_deployment(deployment_id).await?.ok_or_else(|| {
        ApiError::NotFound(format!("Deployment with ID '{deployment_id}' not found"))
    })?;

    let previous_healthy = store
        .list_deployments_by_project(deployment.project_id)
        .await?
        .into_iter()
        .find(|d| d.id != deployment_id && d.status == DeploymentStatus::Healthy);

    rollback_deployment_internal(store, adapter, deployment_id, previous_healthy.as_ref()).await
}

async fn handle_failure_and_auto_rollback(
    store: &Arc<dyn ProjectStore>,
    adapter: &Arc<dyn RuntimeAdapter>,
    deployment_id: Uuid,
    reason: String,
    previous_healthy: Option<&Deployment>,
) -> Result<Deployment, ApiError> {
    let failed = store
        .update_deployment_status(deployment_id, DeploymentStatus::Failed, Some(reason))
        .await?;

    if previous_healthy.is_some() {
        rollback_deployment_internal(store, adapter, failed.id, previous_healthy).await
    } else {
        Ok(failed)
    }
}

async fn rollback_deployment_internal(
    store: &Arc<dyn ProjectStore>,
    adapter: &Arc<dyn RuntimeAdapter>,
    deployment_id: Uuid,
    previous_healthy: Option<&Deployment>,
) -> Result<Deployment, ApiError> {
    let rolling_back = store
        .update_deployment_status(deployment_id, DeploymentStatus::RollingBack, None)
        .await?;

    match adapter.rollback(&rolling_back, previous_healthy).await {
        Ok(()) => {
            let rolled_back = store
                .update_deployment_status(deployment_id, DeploymentStatus::RolledBack, None)
                .await?;
            Ok(rolled_back)
        }
        Err(err) => {
            let _ = store
                .update_deployment_status(
                    deployment_id,
                    DeploymentStatus::Failed,
                    Some(err.to_string()),
                )
                .await;
            Err(err)
        }
    }
}
