use crate::error::ApiError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildStatus {
    Queued,
    Building,
    Succeeded,
    Failed,
    Cancelled,
}

impl BuildStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            BuildStatus::Queued => "queued",
            BuildStatus::Building => "building",
            BuildStatus::Succeeded => "succeeded",
            BuildStatus::Failed => "failed",
            BuildStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "queued" => Ok(BuildStatus::Queued),
            "building" => Ok(BuildStatus::Building),
            "succeeded" => Ok(BuildStatus::Succeeded),
            "failed" => Ok(BuildStatus::Failed),
            "cancelled" => Ok(BuildStatus::Cancelled),
            other => Err(ApiError::Internal(format!(
                "Invalid build status in persistence layer: '{other}'"
            ))),
        }
    }

    pub fn can_transition_to(&self, next: &BuildStatus) -> bool {
        matches!(
            (self, next),
            (BuildStatus::Queued, BuildStatus::Building)
                | (BuildStatus::Queued, BuildStatus::Cancelled)
                | (BuildStatus::Building, BuildStatus::Succeeded)
                | (BuildStatus::Building, BuildStatus::Failed)
                | (BuildStatus::Building, BuildStatus::Cancelled)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Build {
    pub id: Uuid,
    pub project_id: Uuid,
    pub commit_sha: String,
    pub branch: String,
    pub status: BuildStatus,
    pub image_digest: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRepository {
    pub full_name: String,
    pub clone_url: Option<String>,
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubPushEvent {
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub after: String,
    pub repository: GitHubRepository,
}

impl GitHubPushEvent {
    pub fn branch_name(&self) -> &str {
        if let Some(stripped) = self.git_ref.strip_prefix("refs/heads/") {
            stripped
        } else {
            &self.git_ref
        }
    }

    pub fn is_branch_deletion(&self) -> bool {
        self.after == "0000000000000000000000000000000000000000"
    }
}
