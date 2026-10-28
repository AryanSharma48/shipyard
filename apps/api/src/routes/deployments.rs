use crate::AppState;
use crate::error::ApiError;
use crate::models::{CreateDeploymentRequest, Deployment};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use uuid::Uuid;

pub async fn create_deployment(
    State(state): State<AppState>,
    Json(payload): Json<CreateDeploymentRequest>,
) -> Result<(StatusCode, Json<Deployment>), ApiError> {
    payload.validate()?;

    let deployment = crate::orchestrator::execute_deployment(
        &state.store,
        &state.adapter,
        payload.project_id,
        &payload.commit_sha,
        payload.target_type,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(deployment)))
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
