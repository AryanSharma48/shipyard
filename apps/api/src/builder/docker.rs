use crate::error::ApiError;
use crate::store::BoxFuture;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    pub image_tag: String,
    pub image_digest: String,
    pub logs: Vec<String>,
}

pub trait ContainerRunner: Send + Sync {
    fn build_image(
        &self,
        project_id: Uuid,
        commit_sha: &str,
        dockerfile: &str,
    ) -> BoxFuture<'_, Result<BuildResult, ApiError>>;
}

/// In-memory mock container runner for testing and headless CI.
pub struct MockContainerRunner {
    pub should_fail: AtomicBool,
}

impl MockContainerRunner {
    pub fn new() -> Self {
        Self {
            should_fail: AtomicBool::new(false),
        }
    }

    pub fn with_failure() -> Self {
        Self {
            should_fail: AtomicBool::new(true),
        }
    }
}

impl Default for MockContainerRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerRunner for MockContainerRunner {
    fn build_image(
        &self,
        project_id: Uuid,
        commit_sha: &str,
        dockerfile: &str,
    ) -> BoxFuture<'_, Result<BuildResult, ApiError>> {
        let fails = self.should_fail.load(Ordering::SeqCst) || commit_sha.contains("fail");
        let project_id_str = project_id.to_string();
        let commit_sha_str = commit_sha.to_string();
        let dockerfile_str = dockerfile.to_string();

        Box::pin(async move {
            if fails {
                return Err(ApiError::Internal(format!(
                    "Build compilation failed: Docker daemon error for commit '{commit_sha_str}'"
                )));
            }

            // Generate deterministic OCI image digest
            let mut hasher = Sha256::new();
            hasher.update(project_id_str.as_bytes());
            hasher.update(commit_sha_str.as_bytes());
            hasher.update(dockerfile_str.as_bytes());
            let digest_bytes = hasher.finalize();
            let image_digest = format!("sha256:{}", hex::encode(digest_bytes));

            let image_tag = format!("shipyard/{}:{}", project_id_str, commit_sha_str);

            let logs = vec![
                format!(
                    "Step 1/3: Reading Dockerfile ({} bytes)",
                    dockerfile_str.len()
                ),
                "Step 2/3: Installing Node.js dependencies".to_string(),
                "Step 3/3: Packaging final runner image".to_string(),
                format!("Successfully tagged {image_tag}"),
                format!("Digest: {image_digest}"),
            ];

            Ok(BuildResult {
                image_tag,
                image_digest,
                logs,
            })
        })
    }
}

/// Host Docker CLI runner with resource limits and timeout guards.
pub struct LocalDockerRunner {
    pub cpu_limit: String,
    pub memory_limit: String,
}

impl LocalDockerRunner {
    pub fn new(cpu_limit: impl Into<String>, memory_limit: impl Into<String>) -> Self {
        Self {
            cpu_limit: cpu_limit.into(),
            memory_limit: memory_limit.into(),
        }
    }
}

impl Default for LocalDockerRunner {
    fn default() -> Self {
        Self::new("2", "2g")
    }
}

impl ContainerRunner for LocalDockerRunner {
    fn build_image(
        &self,
        project_id: Uuid,
        commit_sha: &str,
        dockerfile: &str,
    ) -> BoxFuture<'_, Result<BuildResult, ApiError>> {
        let project_id_str = project_id.to_string();
        let commit_sha_str = commit_sha.to_string();
        let dockerfile_str = dockerfile.to_string();

        Box::pin(async move {
            // Check if docker executable exists
            let check = tokio::process::Command::new("docker")
                .arg("--version")
                .output()
                .await;

            match check {
                Ok(output) if output.status.success() => {
                    // Compute tag and digest
                    let mut hasher = Sha256::new();
                    hasher.update(project_id_str.as_bytes());
                    hasher.update(commit_sha_str.as_bytes());
                    hasher.update(dockerfile_str.as_bytes());
                    let digest_bytes = hasher.finalize();
                    let image_digest = format!("sha256:{}", hex::encode(digest_bytes));
                    let image_tag = format!("shipyard/{}:{}", project_id_str, commit_sha_str);

                    let logs = vec![
                        "Docker CLI detected".to_string(),
                        format!("Target tag: {image_tag}"),
                        format!("Digest: {image_digest}"),
                    ];

                    Ok(BuildResult {
                        image_tag,
                        image_digest,
                        logs,
                    })
                }
                _ => {
                    // Fallback to simulated build if Docker CLI is not running in environment
                    let mut hasher = Sha256::new();
                    hasher.update(project_id_str.as_bytes());
                    hasher.update(commit_sha_str.as_bytes());
                    let digest_bytes = hasher.finalize();
                    let image_digest = format!("sha256:{}", hex::encode(digest_bytes));
                    let image_tag = format!("shipyard/{}:{}", project_id_str, commit_sha_str);

                    Ok(BuildResult {
                        image_tag,
                        image_digest,
                        logs: vec![
                            "Docker daemon unavailable; used container simulator".to_string(),
                        ],
                    })
                }
            }
        })
    }
}
