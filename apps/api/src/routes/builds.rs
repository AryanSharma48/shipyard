use crate::AppState;
use crate::error::ApiError;
use crate::models::Build;
use axum::{
    Json,
    extract::{Path, State},
};
use uuid::Uuid;

pub async fn get_build(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Build>, ApiError> {
    let build = state
        .store
        .get_build(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Build with ID '{id}' not found")))?;

    Ok(Json(build))
}

pub async fn list_project_builds(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<Build>>, ApiError> {
    // Verify project exists
    state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Project with ID '{project_id}' not found")))?;

    let builds = state.store.list_builds_by_project(project_id).await?;
    Ok(Json(builds))
}
