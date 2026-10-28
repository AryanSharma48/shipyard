use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use shipyard_api::adapter::MockRuntimeAdapter;
use shipyard_api::builder::{
    BuildWorker, ContainerRunner, DockerfileGenerator, MockContainerRunner, NodeDetector,
    NodeProjectConfig, PackageManager,
};
use shipyard_api::models::{Build, BuildStatus, DeploymentStatus, Project};
use shipyard_api::queue::{BuildJob, JobQueue, MemoryJobQueue};
use shipyard_api::store::{MemoryProjectStore, ProjectStore};
use shipyard_api::{AppState, create_app};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

fn compute_signature(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

#[tokio::test]
async fn test_job_queue_enqueue_dequeue() {
    let queue = MemoryJobQueue::new();
    assert!(queue.is_empty().await);

    let job = BuildJob::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "sha-123456".to_string(),
        "main".to_string(),
        "https://github.com/org/repo".to_string(),
    );

    queue.enqueue(job.clone()).await.unwrap();
    assert_eq!(queue.len().await, 1);

    let popped = queue.dequeue().await.unwrap();
    assert!(popped.is_some());
    let popped_job = popped.unwrap();
    assert_eq!(popped_job.build_id, job.build_id);
    assert_eq!(popped_job.commit_sha, "sha-123456");

    let empty = queue.dequeue().await.unwrap();
    assert!(empty.is_none());
}

#[test]
fn test_node_detector_full_config() {
    let pkg_json = r#"{
        "name": "my-express-app",
        "version": "1.0.0",
        "packageManager": "pnpm@8.15.0",
        "scripts": {
            "build": "tsc -b",
            "start": "node dist/index.js"
        }
    }"#;

    let config = NodeDetector::detect_from_package_json(pkg_json).unwrap();
    assert_eq!(config.package_manager, PackageManager::Pnpm);
    assert_eq!(config.install_command, "pnpm install --frozen-lockfile");
    assert_eq!(config.build_command, Some("pnpm run build".to_string()));
    assert_eq!(config.start_command, "pnpm start");
}

#[test]
fn test_node_detector_minimal_defaults() {
    let pkg_json = r#"{
        "name": "simple-api",
        "main": "server.js"
    }"#;

    let config = NodeDetector::detect_from_package_json(pkg_json).unwrap();
    assert_eq!(config.package_manager, PackageManager::Npm);
    assert_eq!(config.install_command, "npm ci");
    assert_eq!(config.build_command, None);
    assert_eq!(config.start_command, "node server.js");
}

#[test]
fn test_node_detector_invalid_json() {
    let bad_json = "this is not json";
    let err = NodeDetector::detect_from_package_json(bad_json);
    assert!(err.is_err());
}

#[tokio::test]
async fn test_build_worker_end_to_end_success() {
    let store = Arc::new(MemoryProjectStore::new());
    let queue = Arc::new(MemoryJobQueue::new());
    let worker = BuildWorker::new(store.clone(), queue.clone());

    let project_id = Uuid::new_v4();
    let build_id = Uuid::new_v4();

    // 1. Create initial queued build in store
    let initial_build = Build {
        id: build_id,
        project_id,
        commit_sha: "commit-abc1234".to_string(),
        branch: "main".to_string(),
        status: BuildStatus::Queued,
        image_digest: None,
        created_at: chrono::Utc::now(),
        started_at: None,
        finished_at: None,
        failure_reason: None,
    };
    store.create_build(initial_build).await.unwrap();

    // 2. Enqueue build job
    let job = BuildJob::new(
        build_id,
        project_id,
        "commit-abc1234".to_string(),
        "main".to_string(),
        "https://github.com/org/repo".to_string(),
    );
    queue.enqueue(job).await.unwrap();

    // 3. Process job via worker
    let processed = worker.process_one_job().await.unwrap();
    assert!(processed.is_some());
    let finished_build = processed.unwrap();

    assert_eq!(finished_build.status, BuildStatus::Succeeded);
    assert!(finished_build.image_digest.is_some());
    assert!(finished_build.image_digest.unwrap().starts_with("sha256:"));
    assert!(finished_build.started_at.is_some());
    assert!(finished_build.finished_at.is_some());

    // 4. Verify persisted state in store
    let persisted = store.get_build(build_id).await.unwrap().unwrap();
    assert_eq!(persisted.status, BuildStatus::Succeeded);
}

#[tokio::test]
async fn test_build_worker_failure_handling() {
    let store = Arc::new(MemoryProjectStore::new());
    let queue = Arc::new(MemoryJobQueue::new());
    let worker = BuildWorker::new(store.clone(), queue.clone());

    let build_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let initial_build = Build {
        id: build_id,
        project_id,
        commit_sha: "fail-compile-err".to_string(),
        branch: "main".to_string(),
        status: BuildStatus::Queued,
        image_digest: None,
        created_at: chrono::Utc::now(),
        started_at: None,
        finished_at: None,
        failure_reason: None,
    };
    store.create_build(initial_build).await.unwrap();

    let job = BuildJob::new(
        build_id,
        project_id,
        "fail-compile-err".to_string(),
        "main".to_string(),
        "https://github.com/org/repo".to_string(),
    );
    queue.enqueue(job).await.unwrap();

    let processed = worker.process_one_job().await.unwrap();
    assert!(processed.is_some());
    let failed_build = processed.unwrap();

    assert_eq!(failed_build.status, BuildStatus::Failed);
    assert!(failed_build.failure_reason.is_some());
    assert!(
        failed_build
            .failure_reason
            .unwrap()
            .contains("Build compilation failed")
    );
}

#[tokio::test]
async fn test_webhook_enqueues_to_job_queue() {
    let queue = Arc::new(MemoryJobQueue::new());
    let store = Arc::new(MemoryProjectStore::new());
    let state = AppState::new(
        store.clone(),
        Arc::new(shipyard_api::adapter::MockRuntimeAdapter::new()),
        queue.clone(),
        "secret".to_string(),
    );
    let app = create_app(state);

    // 1. Create project
    let project_payload = serde_json::json!({
        "name": "worker-demo",
        "repo_url": "https://github.com/org/worker-demo",
        "default_branch": "main"
    });
    let create_proj_req = Request::builder()
        .uri("/projects")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(project_payload.to_string()))
        .unwrap();
    let res = app.clone().oneshot(create_proj_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // 2. Send push webhook
    let push_body = serde_json::json!({
        "ref": "refs/heads/main",
        "after": "worker-commit-111",
        "repository": {
            "full_name": "org/worker-demo"
        }
    })
    .to_string();

    let sig = compute_signature("secret", push_body.as_bytes());
    let webhook_req = Request::builder()
        .uri("/webhooks/github")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-hub-signature-256", sig)
        .body(Body::from(push_body))
        .unwrap();

    let webhook_res = app.clone().oneshot(webhook_req).await.unwrap();
    assert_eq!(webhook_res.status(), StatusCode::ACCEPTED);

    // 3. Verify job was enqueued in the queue!
    assert_eq!(queue.len().await, 1);

    // 4. Process the job via worker
    let worker = BuildWorker::new(store.clone(), queue.clone());
    let processed = worker.process_one_job().await.unwrap();
    assert!(processed.is_some());
    let build = processed.unwrap();
    assert_eq!(build.status, BuildStatus::Succeeded);
    assert_eq!(build.commit_sha, "worker-commit-111");
    assert!(build.image_digest.is_some());

    // Queue is now empty
    assert_eq!(queue.len().await, 0);
}

#[tokio::test]
async fn test_build_worker_automated_deployment_success() {
    let store = Arc::new(MemoryProjectStore::new());
    let queue = Arc::new(MemoryJobQueue::new());
    let adapter = Arc::new(MockRuntimeAdapter::new());
    let worker = BuildWorker::with_adapter(store.clone(), queue.clone(), adapter.clone());

    let project_id = Uuid::new_v4();
    let project = Project {
        id: project_id,
        name: "auto-deploy-app".to_string(),
        repo_url: "https://github.com/org/auto-deploy-app".to_string(),
        default_branch: "main".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    store.create_project(project).await.unwrap();

    let build_id = Uuid::new_v4();
    let build = Build {
        id: build_id,
        project_id,
        commit_sha: "commit-deploy-999".to_string(),
        branch: "main".to_string(),
        status: BuildStatus::Queued,
        image_digest: None,
        created_at: chrono::Utc::now(),
        started_at: None,
        finished_at: None,
        failure_reason: None,
    };
    store.create_build(build).await.unwrap();

    let job = BuildJob::new(
        build_id,
        project_id,
        "commit-deploy-999".to_string(),
        "main".to_string(),
        "https://github.com/org/auto-deploy-app".to_string(),
    );
    queue.enqueue(job).await.unwrap();

    // Process job with deployment
    let result = worker.process_one_job_with_deployment().await.unwrap();
    assert!(result.is_some());
    let (finished_build, triggered_deployment) = result.unwrap();

    // Verify build
    assert_eq!(finished_build.status, BuildStatus::Succeeded);
    assert!(finished_build.image_digest.is_some());

    // Verify automated deployment
    assert!(triggered_deployment.is_some());
    let dep = triggered_deployment.unwrap();
    assert_eq!(dep.project_id, project_id);
    assert_eq!(dep.commit_sha, "commit-deploy-999");
    assert_eq!(dep.status, DeploymentStatus::Healthy);

    // Verify store has the deployment persisted
    let deployments = store.list_deployments_by_project(project_id).await.unwrap();
    assert_eq!(deployments.len(), 1);
    assert_eq!(deployments[0].status, DeploymentStatus::Healthy);
}

#[tokio::test]
async fn test_build_worker_automated_deployment_adapter_failure() {
    let store = Arc::new(MemoryProjectStore::new());
    let queue = Arc::new(MemoryJobQueue::new());
    let adapter = Arc::new(MockRuntimeAdapter::with_failure());
    let worker = BuildWorker::with_adapter(store.clone(), queue.clone(), adapter);

    let project_id = Uuid::new_v4();
    let project = Project {
        id: project_id,
        name: "failing-deploy-app".to_string(),
        repo_url: "https://github.com/org/failing-deploy-app".to_string(),
        default_branch: "main".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    store.create_project(project).await.unwrap();

    let build_id = Uuid::new_v4();
    let build = Build {
        id: build_id,
        project_id,
        commit_sha: "commit-good-adapter-error".to_string(),
        branch: "main".to_string(),
        status: BuildStatus::Queued,
        image_digest: None,
        created_at: chrono::Utc::now(),
        started_at: None,
        finished_at: None,
        failure_reason: None,
    };
    store.create_build(build).await.unwrap();

    let job = BuildJob::new(
        build_id,
        project_id,
        "commit-good-adapter-error".to_string(),
        "main".to_string(),
        "https://github.com/org/failing-deploy-app".to_string(),
    );
    queue.enqueue(job).await.unwrap();

    let result = worker.process_one_job_with_deployment().await.unwrap();
    assert!(result.is_some());
    let (finished_build, triggered_deployment) = result.unwrap();

    // Invariant: Build still succeeded (container image was built and valid)
    assert_eq!(finished_build.status, BuildStatus::Succeeded);

    // Deployment failed with reason
    assert!(triggered_deployment.is_some());
    let dep = triggered_deployment.unwrap();
    assert_eq!(dep.status, DeploymentStatus::Failed);
    assert!(
        dep.failure_reason
            .unwrap()
            .contains("Mock adapter failed to deploy deployment")
    );
}

#[tokio::test]
async fn test_webhook_to_automated_deployment_full_loop() {
    let store = Arc::new(MemoryProjectStore::new());
    let queue = Arc::new(MemoryJobQueue::new());
    let adapter = Arc::new(MockRuntimeAdapter::new());
    let state = AppState::new(
        store.clone(),
        adapter.clone(),
        queue.clone(),
        "webhook-full-loop-secret".to_string(),
    );
    let app = create_app(state);

    // 1. Create project via API
    let project_payload = serde_json::json!({
        "name": "full-loop-service",
        "repo_url": "https://github.com/org/full-loop-service",
        "default_branch": "main"
    });
    let create_proj_req = Request::builder()
        .uri("/projects")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(project_payload.to_string()))
        .unwrap();
    let res = app.clone().oneshot(create_proj_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let projects = store.list_projects().await.unwrap();
    let project_id = projects[0].id;

    // 2. Incoming GitHub push webhook
    let push_body = serde_json::json!({
        "ref": "refs/heads/main",
        "after": "full-loop-sha-456",
        "repository": {
            "full_name": "org/full-loop-service"
        }
    })
    .to_string();

    let sig = compute_signature("webhook-full-loop-secret", push_body.as_bytes());
    let webhook_req = Request::builder()
        .uri("/webhooks/github")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-hub-signature-256", sig)
        .body(Body::from(push_body))
        .unwrap();

    let webhook_res = app.clone().oneshot(webhook_req).await.unwrap();
    assert_eq!(webhook_res.status(), StatusCode::ACCEPTED);

    // 3. Queue received job
    assert_eq!(queue.len().await, 1);

    // 4. Worker processes job with adapter
    let worker = BuildWorker::with_adapter(store.clone(), queue.clone(), adapter);
    let processed = worker.process_one_job_with_deployment().await.unwrap();
    assert!(processed.is_some());

    let (build, deployment) = processed.unwrap();
    assert_eq!(build.status, BuildStatus::Succeeded);
    assert!(deployment.is_some());

    let dep = deployment.unwrap();
    assert_eq!(dep.project_id, project_id);
    assert_eq!(dep.commit_sha, "full-loop-sha-456");
    assert_eq!(dep.status, DeploymentStatus::Healthy);

    // 5. Verify deployments endpoint returns healthy deployment
    let get_dep_req = Request::builder()
        .uri(format!("/deployments/{}", dep.id))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let get_dep_res = app.clone().oneshot(get_dep_req).await.unwrap();
    assert_eq!(get_dep_res.status(), StatusCode::OK);
}

#[test]
fn test_dockerfile_generator_npm_with_build() {
    let config = NodeProjectConfig {
        package_manager: PackageManager::Npm,
        install_command: "npm ci".to_string(),
        build_command: Some("npm run build".to_string()),
        start_command: "node dist/index.js".to_string(),
    };

    let dockerfile = DockerfileGenerator::generate(&config, Some("20-alpine"));

    // Check multi-stage structure
    assert!(dockerfile.contains("FROM node:20-alpine AS deps"));
    assert!(dockerfile.contains("COPY package.json package-lock.json* ./"));
    assert!(dockerfile.contains("RUN npm ci"));

    // Check builder stage
    assert!(dockerfile.contains("FROM node:20-alpine AS builder"));
    assert!(dockerfile.contains("RUN npm run build"));

    // Check runner stage and security invariants
    assert!(dockerfile.contains("FROM node:20-alpine AS runner"));
    assert!(dockerfile.contains("ENV NODE_ENV=production"));
    assert!(dockerfile.contains("USER node"));
    assert!(dockerfile.contains("EXPOSE 3000"));
    assert!(dockerfile.contains("CMD [\"node\", \"dist/index.js\"]"));
}

#[test]
fn test_dockerfile_generator_pnpm() {
    let config = NodeProjectConfig {
        package_manager: PackageManager::Pnpm,
        install_command: "pnpm install --frozen-lockfile".to_string(),
        build_command: None,
        start_command: "pnpm start".to_string(),
    };

    let dockerfile = DockerfileGenerator::generate(&config, None);

    assert!(dockerfile.contains("pnpm-lock.yaml*"));
    assert!(dockerfile.contains("RUN corepack enable && corepack prepare pnpm@latest --activate"));
    assert!(dockerfile.contains("RUN pnpm install --frozen-lockfile"));
    assert!(dockerfile.contains("CMD [\"pnpm\", \"start\"]"));
}

#[test]
fn test_dockerfile_generator_yarn() {
    let config = NodeProjectConfig {
        package_manager: PackageManager::Yarn,
        install_command: "yarn install --frozen-lockfile".to_string(),
        build_command: Some("yarn run build".to_string()),
        start_command: "yarn start".to_string(),
    };

    let dockerfile = DockerfileGenerator::generate(&config, Some("22-alpine"));

    assert!(dockerfile.contains("FROM node:22-alpine AS deps"));
    assert!(dockerfile.contains("yarn.lock*"));
    assert!(dockerfile.contains("RUN corepack enable && corepack prepare yarn@stable --activate"));
    assert!(dockerfile.contains("RUN yarn install --frozen-lockfile"));
}

#[tokio::test]
async fn test_container_runner_mock_build_and_logs() {
    let runner = MockContainerRunner::new();
    let project_id = Uuid::new_v4();
    let commit_sha = "mock-sha-999";
    let dockerfile = "FROM node:20-alpine\nCMD [\"node\", \"index.js\"]";

    let result = runner
        .build_image(project_id, commit_sha, dockerfile)
        .await
        .unwrap();

    assert_eq!(
        result.image_tag,
        format!("shipyard/{}:{}", project_id, commit_sha)
    );
    assert!(result.image_digest.starts_with("sha256:"));
    assert_eq!(result.logs.len(), 5);
    assert!(result.logs[0].contains("Reading Dockerfile"));
}

#[tokio::test]
async fn test_build_worker_with_custom_runner_and_logs() {
    let store = Arc::new(MemoryProjectStore::new());
    let queue = Arc::new(MemoryJobQueue::new());
    let runner = Arc::new(MockContainerRunner::new());
    let worker = BuildWorker::with_runner(store.clone(), queue.clone(), None, runner);

    let project_id = Uuid::new_v4();
    let build_id = Uuid::new_v4();

    let build = Build {
        id: build_id,
        project_id,
        commit_sha: "runner-test-commit".to_string(),
        branch: "main".to_string(),
        status: BuildStatus::Queued,
        image_digest: None,
        created_at: chrono::Utc::now(),
        started_at: None,
        finished_at: None,
        failure_reason: None,
    };
    store.create_build(build).await.unwrap();

    let job = BuildJob::new(
        build_id,
        project_id,
        "runner-test-commit".to_string(),
        "main".to_string(),
        "https://github.com/org/repo".to_string(),
    );
    queue.enqueue(job).await.unwrap();

    let processed = worker.process_one_job().await.unwrap();
    assert!(processed.is_some());
    let finished = processed.unwrap();

    assert_eq!(finished.status, BuildStatus::Succeeded);
    assert!(finished.image_digest.is_some());
    assert!(finished.image_digest.unwrap().starts_with("sha256:"));
}
