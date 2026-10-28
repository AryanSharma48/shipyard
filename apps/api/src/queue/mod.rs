pub mod memory;

use crate::error::ApiError;
use crate::store::BoxFuture;
use chrono::{DateTime, Utc};
pub use memory::MemoryJobQueue;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildJob {
    pub job_id: Uuid,
    pub build_id: Uuid,
    pub project_id: Uuid,
    pub commit_sha: String,
    pub branch: String,
    pub repo_url: String,
    pub enqueued_at: DateTime<Utc>,
}

impl BuildJob {
    pub fn new(
        build_id: Uuid,
        project_id: Uuid,
        commit_sha: String,
        branch: String,
        repo_url: String,
    ) -> Self {
        Self {
            job_id: Uuid::new_v4(),
            build_id,
            project_id,
            commit_sha,
            branch,
            repo_url,
            enqueued_at: Utc::now(),
        }
    }
}

pub trait JobQueue: Send + Sync {
    fn enqueue(&self, job: BuildJob) -> BoxFuture<'_, Result<(), ApiError>>;
    fn dequeue(&self) -> BoxFuture<'_, Result<Option<BuildJob>, ApiError>>;
}
