use crate::AppState;
use crate::error::ApiError;
use crate::models::{CreateDeploymentRequest, Deployment, DeploymentStatus};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use uuid::Uuid;

pub async fn create_deployment(
    State(state): State<AppState>,
    Json(payload): Json<CreateDeploymentRequest>,
) -> Result<(StatusCode, Json<Deployment>), ApiError> {
    payload.validate()?;

    // Verify target project exists
    state
        .store
        .get_project(payload.project_id)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "Project with ID '{}' not found",
                payload.project_id
            ))
        })?;

    let now = Utc::now();
    let deployment = Deployment {
        id: Uuid::new_v4(),
        project_id: payload.project_id,
        commit_sha: payload.commit_sha.trim().to_string(),
        target_type: payload.target_type.unwrap_or_default(),
        status: DeploymentStatus::Queued,
        created_at: now,
        started_at: None,
        finished_at: None,
        failure_reason: None,
    };

    let saved = state.store.create_deployment(deployment).await?;

    // Transition: Queued -> Deploying
    let deploying = state
        .store
        .update_deployment_status(saved.id, DeploymentStatus::Deploying, None)
        .await?;

    // Trigger runtime adapter
    match state.adapter.deploy(&deploying).await {
        Ok(()) => {
            // Transition: Deploying -> Verifying -> Healthy
            state
                .store
                .update_deployment_status(saved.id, DeploymentStatus::Verifying, None)
                .await?;

            let healthy = state
                .store
                .update_deployment_status(saved.id, DeploymentStatus::Healthy, None)
                .await?;

            Ok((StatusCode::CREATED, Json(healthy)))
        }
        Err(err) => {
            let reason = err.to_string();
            let failed = state
                .store
                .update_deployment_status(saved.id, DeploymentStatus::Failed, Some(reason))
                .await?;

            Ok((StatusCode::CREATED, Json(failed)))
        }
    }
}

pub async fn get_deployment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Deployment>, ApiError> {
    let deployment = state
        .store
        .get_deployment(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Deployment with ID '{id}' not found")))?;

    Ok(Json(deployment))
}

pub async fn list_project_deployments(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<Deployment>>, ApiError> {
    // Verify project exists
    state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Project with ID '{project_id}' not found")))?;

    let deployments = state.store.list_deployments_by_project(project_id).await?;
    Ok(Json(deployments))
}
