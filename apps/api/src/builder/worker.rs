use crate::adapter::RuntimeAdapter;
use crate::error::ApiError;
use crate::models::{Build, BuildStatus, Deployment};
use crate::queue::{BuildJob, JobQueue};
use crate::store::ProjectStore;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub struct BuildWorker {
    pub store: Arc<dyn ProjectStore>,
    pub queue: Arc<dyn JobQueue>,
    pub adapter: Option<Arc<dyn RuntimeAdapter>>,
}

impl BuildWorker {
    pub fn new(store: Arc<dyn ProjectStore>, queue: Arc<dyn JobQueue>) -> Self {
        Self {
            store,
            queue,
            adapter: None,
        }
    }

    pub fn with_adapter(
        store: Arc<dyn ProjectStore>,
        queue: Arc<dyn JobQueue>,
        adapter: Arc<dyn RuntimeAdapter>,
    ) -> Self {
        Self {
            store,
            queue,
            adapter: Some(adapter),
        }
    }

    /// Pulls and processes a single build job from the queue.
    /// If an adapter is configured, automatically triggers a deployment when build succeeds.
    /// Returns Ok(Some(build)) if a job was processed, Ok(None) if queue was empty.
    pub async fn process_one_job(&self) -> Result<Option<Build>, ApiError> {
        self.process_one_job_with_deployment()
            .await
            .map(|opt| opt.map(|(build, _)| build))
    }

    /// Pulls and processes a single build job from the queue, returning the build and any triggered deployment.
    pub async fn process_one_job_with_deployment(
        &self,
    ) -> Result<Option<(Build, Option<Deployment>)>, ApiError> {
        let job = match self.queue.dequeue().await? {
            Some(j) => j,
            None => return Ok(None),
        };

        let result = self.execute_build(&job).await;
        match result {
            Ok(build) => {
                let deployment = if let Some(adapter) = &self.adapter {
                    match crate::orchestrator::execute_deployment(
                        &self.store,
                        adapter,
                        job.project_id,
                        &job.commit_sha,
                        None,
                    )
                    .await
                    {
                        Ok(dep) => Some(dep),
                        Err(err) => {
                            eprintln!("Automated deployment skipped or failed: {err}");
                            None
                        }
                    }
                } else {
                    None
                };

                Ok(Some((build, deployment)))
            }
            Err(err) => {
                let reason = err.to_string();
                let failed_build = self
                    .store
                    .update_build_status(job.build_id, BuildStatus::Failed, None, Some(reason))
                    .await?;
                Ok(Some((failed_build, None)))
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
