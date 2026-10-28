use crate::AppState;
use crate::error::ApiError;
use crate::models::{CreateProjectRequest, Project};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use uuid::Uuid;

pub async fn create_project(
    State(state): State<AppState>,
    Json(payload): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    payload.validate()?;

    let now = Utc::now();
    let project = Project {
        id: Uuid::new_v4(),
        name: payload.name.trim().to_string(),
        repo_url: payload.repo_url.trim().to_string(),
        default_branch: payload
            .default_branch
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "main".to_string()),
        created_at: now,
        updated_at: now,
    };

    let created = state.store.create_project(project).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn list_projects(State(state): State<AppState>) -> Result<Json<Vec<Project>>, ApiError> {
    let projects = state.store.list_projects().await?;
    Ok(Json(projects))
}

pub async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Project>, ApiError> {
    let maybe_project: Option<Project> = state.store.get_project(id).await?;
    let project = maybe_project
        .ok_or_else(|| ApiError::NotFound(format!("Project with ID '{id}' not found")))?;

    Ok(Json(project))
}
