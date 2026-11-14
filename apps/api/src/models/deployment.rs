use crate::error::ApiError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetType {
    #[default]
    HostedEcs,
    CustomerAwsEcs,
}

impl TargetType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TargetType::HostedEcs => "hosted_ecs",
            TargetType::CustomerAwsEcs => "customer_aws_ecs",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "hosted_ecs" => Ok(TargetType::HostedEcs),
            "customer_aws_ecs" => Ok(TargetType::CustomerAwsEcs),
            other => Err(ApiError::Internal(format!(
                "Invalid deployment target_type in persistence layer: '{other}'"
            ))),
        }
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
    Superseded,
}

impl DeploymentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeploymentStatus::Queued => "queued",
            DeploymentStatus::Deploying => "deploying",
            DeploymentStatus::Verifying => "verifying",
            DeploymentStatus::Healthy => "healthy",
            DeploymentStatus::Failed => "failed",
            DeploymentStatus::RollingBack => "rolling_back",
            DeploymentStatus::RolledBack => "rolled_back",
            DeploymentStatus::Superseded => "superseded",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "queued" => Ok(DeploymentStatus::Queued),
            "deploying" => Ok(DeploymentStatus::Deploying),
            "verifying" => Ok(DeploymentStatus::Verifying),
            "healthy" => Ok(DeploymentStatus::Healthy),
            "failed" => Ok(DeploymentStatus::Failed),
            "rolling_back" => Ok(DeploymentStatus::RollingBack),
            "rolled_back" => Ok(DeploymentStatus::RolledBack),
            "superseded" => Ok(DeploymentStatus::Superseded),
            other => Err(ApiError::Internal(format!(
                "Invalid deployment status in persistence layer: '{other}'"
            ))),
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self,
            DeploymentStatus::Queued | DeploymentStatus::Deploying | DeploymentStatus::Verifying
        )
    }

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

            // Release replacement
            (DeploymentStatus::Healthy, DeploymentStatus::Superseded) => true,

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
