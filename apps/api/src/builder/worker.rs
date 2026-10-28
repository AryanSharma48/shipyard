use crate::adapter::RuntimeAdapter;
use crate::builder::detector::{NodeProjectConfig, PackageManager};
use crate::builder::docker::{ContainerRunner, MockContainerRunner};
use crate::builder::docker_template::DockerfileGenerator;
use crate::error::ApiError;
use crate::models::{Build, BuildStatus, Deployment};
use crate::queue::{BuildJob, JobQueue};
use crate::store::ProjectStore;
use std::sync::Arc;

pub struct BuildWorker {
    pub store: Arc<dyn ProjectStore>,
    pub queue: Arc<dyn JobQueue>,
    pub adapter: Option<Arc<dyn RuntimeAdapter>>,
    pub runner: Arc<dyn ContainerRunner>,
}

impl BuildWorker {
    pub fn new(store: Arc<dyn ProjectStore>, queue: Arc<dyn JobQueue>) -> Self {
        Self {
            store,
            queue,
            adapter: None,
            runner: Arc::new(MockContainerRunner::new()),
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
            runner: Arc::new(MockContainerRunner::new()),
        }
    }

    pub fn with_runner(
        store: Arc<dyn ProjectStore>,
        queue: Arc<dyn JobQueue>,
        adapter: Option<Arc<dyn RuntimeAdapter>>,
        runner: Arc<dyn ContainerRunner>,
    ) -> Self {
        Self {
            store,
            queue,
            adapter,
            runner,
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

        // 1. Synthesize multi-stage Dockerfile
        let config = NodeProjectConfig {
            package_manager: PackageManager::Npm,
            install_command: "npm ci".to_string(),
            build_command: Some("npm run build".to_string()),
            start_command: "node server.js".to_string(),
        };
        let dockerfile = DockerfileGenerator::generate(&config, Some("20-alpine"));

        // 2. Build container image via ContainerRunner
        let result = self
            .runner
            .build_image(job.project_id, &job.commit_sha, &dockerfile)
            .await?;

        // 3. Transition: Building -> Succeeded with real container image digest
        let finished = self
            .store
            .update_build_status(
                job.build_id,
                BuildStatus::Succeeded,
                Some(result.image_digest),
                None,
            )
            .await?;

        Ok(finished)
    }
}
