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
    pub fn can_transition_to(&self, next: &BuildStatus) -> bool {
        match (self, next) {
            (BuildStatus::Queued, BuildStatus::Building) => true,
            (BuildStatus::Queued, BuildStatus::Cancelled) => true,
            (BuildStatus::Building, BuildStatus::Succeeded) => true,
            (BuildStatus::Building, BuildStatus::Failed) => true,
            (BuildStatus::Building, BuildStatus::Cancelled) => true,
            _ => false,
        }
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
