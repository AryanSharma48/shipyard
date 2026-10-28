use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::Value;
use shipyard_api::{AppState, create_app};
use tower::ServiceExt;
use uuid::Uuid;

async fn response_json(body: Body) -> Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn test_health_check() {
    let app = create_app(AppState::in_memory());

    let req = Request::builder()
        .uri("/health")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let json = response_json(res.into_body()).await;
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "shipyard-api");
}

#[tokio::test]
async fn test_create_project_success() {
    let app = create_app(AppState::in_memory());

    let payload = serde_json::json!({
        "name": "my-service",
        "repo_url": "https://github.com/example/my-service"
    });

    let req = Request::builder()
        .uri("/projects")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let json = response_json(res.into_body()).await;
    assert_eq!(json["name"], "my-service");
    assert_eq!(json["repo_url"], "https://github.com/example/my-service");
    assert_eq!(json["default_branch"], "main");
    assert!(json["id"].is_string());
    assert!(json["created_at"].is_string());
}

#[tokio::test]
async fn test_create_project_custom_branch() {
    let app = create_app(AppState::in_memory());

    let payload = serde_json::json!({
        "name": "backend",
        "repo_url": "https://github.com/example/backend",
        "default_branch": "develop"
    });

    let req = Request::builder()
        .uri("/projects")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let json = response_json(res.into_body()).await;
    assert_eq!(json["default_branch"], "develop");
}

#[tokio::test]
async fn test_create_project_validation_failure() {
    let app = create_app(AppState::in_memory());

    // Empty name
    let payload = serde_json::json!({
        "name": "   ",
        "repo_url": "https://github.com/example/backend"
    });

    let req = Request::builder()
        .uri("/projects")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let json = response_json(res.into_body()).await;
    assert_eq!(json["error"], "Project name cannot be empty");
}

#[tokio::test]
async fn test_list_and_get_project() {
    let app = create_app(AppState::in_memory());

    // 1. Create a project
    let payload = serde_json::json!({
        "name": "api-gateway",
        "repo_url": "https://github.com/example/gateway"
    });

    let create_req = Request::builder()
        .uri("/projects")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let create_res = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(create_res.status(), StatusCode::CREATED);

    let created_json = response_json(create_res.into_body()).await;
    let project_id = created_json["id"].as_str().unwrap();

    // 2. List projects
    let list_req = Request::builder()
        .uri("/projects")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let list_res = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);

    let list_json = response_json(list_res.into_body()).await;
    let projects = list_json.as_array().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["id"], project_id);

    // 3. Get single project by ID
    let get_req = Request::builder()
        .uri(format!("/projects/{project_id}"))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let get_res = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_res.status(), StatusCode::OK);

    let get_json = response_json(get_res.into_body()).await;
    assert_eq!(get_json["name"], "api-gateway");

    // 4. Get non-existent project
    let non_existent_id = Uuid::new_v4();
    let not_found_req = Request::builder()
        .uri(format!("/projects/{non_existent_id}"))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let not_found_res = app.oneshot(not_found_req).await.unwrap();
    assert_eq!(not_found_res.status(), StatusCode::NOT_FOUND);

    let not_found_json = response_json(not_found_res.into_body()).await;
    assert!(
        not_found_json["error"]
            .as_str()
            .unwrap()
            .contains("not found")
    );
}
