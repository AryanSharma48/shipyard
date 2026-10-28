use crate::error::ApiError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetType {
    HostedEcs,
    CustomerAwsEcs,
}

impl Default for TargetType {
    fn default() -> Self {
        TargetType::HostedEcs
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Queued,
    Deploying,
    Verifying,
    Healthy,
    Failed,
    RollingBack,
    RolledBack,
}

impl DeploymentStatus {
    pub fn can_transition_to(&self, next: &DeploymentStatus) -> bool {
        match (self, next) {
            // Forward progression
            (DeploymentStatus::Queued, DeploymentStatus::Deploying) => true,
            (DeploymentStatus::Deploying, DeploymentStatus::Verifying) => true,
            (DeploymentStatus::Verifying, DeploymentStatus::Healthy) => true,

            // Failure progression
            (DeploymentStatus::Queued, DeploymentStatus::Failed) => true,
            (DeploymentStatus::Deploying, DeploymentStatus::Failed) => true,
            (DeploymentStatus::Verifying, DeploymentStatus::Failed) => true,

            // Rollback progression
            (DeploymentStatus::Failed, DeploymentStatus::RollingBack) => true,
            (DeploymentStatus::RollingBack, DeploymentStatus::RolledBack) => true,
            (DeploymentStatus::RollingBack, DeploymentStatus::Failed) => true,

            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Deployment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub commit_sha: String,
    pub target_type: TargetType,
    pub status: DeploymentStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateDeploymentRequest {
    pub project_id: Uuid,
    pub commit_sha: String,
    pub target_type: Option<TargetType>,
}

impl CreateDeploymentRequest {
    pub fn validate(&self) -> Result<(), ApiError> {
        if self.commit_sha.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "Commit SHA cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}
