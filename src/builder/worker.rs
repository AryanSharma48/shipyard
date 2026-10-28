use crate::error::ApiError;
use crate::models::{Build, BuildStatus};
use crate::queue::{BuildJob, JobQueue};
use crate::store::ProjectStore;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub struct BuildWorker {
    pub store: Arc<dyn ProjectStore>,
    pub queue: Arc<dyn JobQueue>,
}

impl BuildWorker {
    pub fn new(store: Arc<dyn ProjectStore>, queue: Arc<dyn JobQueue>) -> Self {
        Self { store, queue }
    }

    /// Pulls and processes a single build job from the queue.
    /// Returns Ok(Some(build)) if a job was processed, Ok(None) if queue was empty.
    pub async fn process_one_job(&self) -> Result<Option<Build>, ApiError> {
        let job = match self.queue.dequeue().await? {
            Some(j) => j,
            None => return Ok(None),
        };

        let result = self.execute_build(&job).await;
        match result {
            Ok(build) => Ok(Some(build)),
            Err(err) => {
                let reason = err.to_string();
                let failed_build = self
                    .store
                    .update_build_status(job.build_id, BuildStatus::Failed, None, Some(reason))
                    .await?;
                Ok(Some(failed_build))
            }
        }
    }

    async fn execute_build(&self, job: &BuildJob) -> Result<Build, ApiError> {
        // Transition: Queued -> Building
        self.store
            .update_build_status(job.build_id, BuildStatus::Building, None, None)
            .await?;

        // Check for simulated failure
        if job.commit_sha.contains("fail") {
            return Err(ApiError::Internal(format!(
                "Build compilation failed for commit '{}'",
                job.commit_sha
            )));
        }

        // Generate simulated container image digest (SHA256 of project + commit)
        let mut hasher = Sha256::new();
        hasher.update(job.project_id.as_bytes());
        hasher.update(job.commit_sha.as_bytes());
        let digest_bytes = hasher.finalize();
        let image_digest = format!("sha256:{}", hex::encode(digest_bytes));

        // Transition: Building -> Succeeded
        let finished = self
            .store
            .update_build_status(
                job.build_id,
                BuildStatus::Succeeded,
                Some(image_digest),
                None,
            )
            .await?;

        Ok(finished)
    }
}
