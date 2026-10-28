use crate::error::ApiError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub repo_url: String,
    pub default_branch: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub repo_url: String,
    pub default_branch: Option<String>,
}

impl CreateProjectRequest {
    pub fn validate(&self) -> Result<(), ApiError> {
        if self.name.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "Project name cannot be empty".to_string(),
            ));
        }

        if self.repo_url.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "Repository URL cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}
