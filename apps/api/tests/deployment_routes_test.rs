use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::Value;
use shipyard_api::adapter::MockRuntimeAdapter;
use shipyard_api::models::DeploymentStatus;
use shipyard_api::{AppState, create_app};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

async fn response_json(body: Body) -> Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn create_test_project(app: &axum::Router) -> String {
    let payload = serde_json::json!({
        "name": "test-service",
        "repo_url": "https://github.com/example/test-service"
    });

    let req = Request::builder()
        .uri("/projects")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let json = response_json(res.into_body()).await;
    json["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_create_deployment_success() {
    let app = create_app(AppState::in_memory());
    let project_id = create_test_project(&app).await;

    let payload = serde_json::json!({
        "project_id": project_id,
        "commit_sha": "a1b2c3d4e5f6",
        "target_type": "hosted_ecs"
    });

    let req = Request::builder()
        .uri("/deployments")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let json = response_json(res.into_body()).await;
    assert_eq!(json["project_id"], project_id);
    assert_eq!(json["commit_sha"], "a1b2c3d4e5f6");
    assert_eq!(json["target_type"], "hosted_ecs");
    assert_eq!(json["status"], "healthy");
    assert!(json["id"].is_string());
    assert!(json["started_at"].is_string());
    assert!(json["finished_at"].is_string());
}

#[tokio::test]
async fn test_create_deployment_nonexistent_project() {
    let app = create_app(AppState::in_memory());

    let non_existent_project = Uuid::new_v4();
    let payload = serde_json::json!({
        "project_id": non_existent_project,
        "commit_sha": "a1b2c3d4e5f6"
    });

    let req = Request::builder()
        .uri("/deployments")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let json = response_json(res.into_body()).await;
    assert!(json["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_create_deployment_empty_commit_sha() {
    let app = create_app(AppState::in_memory());
    let project_id = create_test_project(&app).await;

    let payload = serde_json::json!({
        "project_id": project_id,
        "commit_sha": "   "
    });

    let req = Request::builder()
        .uri("/deployments")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let json = response_json(res.into_body()).await;
    assert_eq!(json["error"], "Commit SHA cannot be empty");
}

#[tokio::test]
async fn test_get_deployment_by_id() {
    let app = create_app(AppState::in_memory());
    let project_id = create_test_project(&app).await;

    // 1. Create deployment
    let payload = serde_json::json!({
        "project_id": project_id,
        "commit_sha": "abcdef123456"
    });

    let create_req = Request::builder()
        .uri("/deployments")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let create_res = app.clone().oneshot(create_req).await.unwrap();
    let created_json = response_json(create_res.into_body()).await;
    let deployment_id = created_json["id"].as_str().unwrap();

    // 2. Fetch deployment
    let get_req = Request::builder()
        .uri(format!("/deployments/{deployment_id}"))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let get_res = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_res.status(), StatusCode::OK);

    let get_json = response_json(get_res.into_body()).await;
    assert_eq!(get_json["id"], deployment_id);
    assert_eq!(get_json["status"], "healthy");

    // 3. Fetch non-existent deployment
    let non_existent_id = Uuid::new_v4();
    let not_found_req = Request::builder()
        .uri(format!("/deployments/{non_existent_id}"))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let not_found_res = app.oneshot(not_found_req).await.unwrap();
    assert_eq!(not_found_res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_list_project_deployments() {
    let app = create_app(AppState::in_memory());
    let project_id = create_test_project(&app).await;

    // Create 2 deployments
    for sha in &["sha-001", "sha-002"] {
        let payload = serde_json::json!({
            "project_id": project_id,
            "commit_sha": sha
        });

        let req = Request::builder()
            .uri("/deployments")
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap();

        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    // List deployments for project
    let list_req = Request::builder()
        .uri(format!("/projects/{project_id}/deployments"))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let list_res = app.oneshot(list_req).await.unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);

    let list_json = response_json(list_res.into_body()).await;
    let list = list_json.as_array().unwrap();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn test_create_deployment_with_adapter_failure() {
    let adapter = Arc::new(MockRuntimeAdapter::with_failure());
    let state = AppState::in_memory_with_adapter(adapter);
    let app = create_app(state);

    let project_id = create_test_project(&app).await;

    let payload = serde_json::json!({
        "project_id": project_id,
        "commit_sha": "fail-sha"
    });

    let req = Request::builder()
        .uri("/deployments")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let json = response_json(res.into_body()).await;
    assert_eq!(json["status"], "failed");
    assert!(
        json["failure_reason"]
            .as_str()
            .unwrap()
            .contains("Mock adapter failed")
    );
}

#[tokio::test]
async fn test_deployment_state_transition_validation() {
    assert!(DeploymentStatus::Queued.can_transition_to(&DeploymentStatus::Deploying));
    assert!(DeploymentStatus::Deploying.can_transition_to(&DeploymentStatus::Verifying));
    assert!(DeploymentStatus::Verifying.can_transition_to(&DeploymentStatus::Healthy));
    assert!(DeploymentStatus::Deploying.can_transition_to(&DeploymentStatus::Failed));
    assert!(DeploymentStatus::Failed.can_transition_to(&DeploymentStatus::RollingBack));
    assert!(DeploymentStatus::RollingBack.can_transition_to(&DeploymentStatus::RolledBack));

    // Invalid jumps
    assert!(!DeploymentStatus::Queued.can_transition_to(&DeploymentStatus::Healthy));
    assert!(!DeploymentStatus::Healthy.can_transition_to(&DeploymentStatus::Deploying));
    assert!(!DeploymentStatus::RolledBack.can_transition_to(&DeploymentStatus::Healthy));
}
