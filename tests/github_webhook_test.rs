use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use hmac::{Hmac, Mac};
use http_body_util::BodyExt;
use serde_json::Value;
use sha2::Sha256;
use shipyard_api::{AppState, create_app};
use tower::ServiceExt;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

const TEST_SECRET: &str = "super-secret-webhook-key";

fn compute_signature(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

async fn response_json(body: Body) -> Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn create_test_project(app: &axum::Router, repo_url: &str, branch: &str) -> String {
    let payload = serde_json::json!({
        "name": "octocat-repo",
        "repo_url": repo_url,
        "default_branch": branch
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
async fn test_webhook_ping_event() {
    let app = create_app(AppState::in_memory_with_secret(TEST_SECRET.to_string()));

    let body = serde_json::json!({ "zen": "Design for failure." }).to_string();
    let sig = compute_signature(TEST_SECRET, body.as_bytes());

    let req = Request::builder()
        .uri("/webhooks/github")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "ping")
        .header("x-hub-signature-256", sig)
        .body(Body::from(body))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let json = response_json(res.into_body()).await;
    assert_eq!(json["status"], "pong");
}

#[tokio::test]
async fn test_webhook_invalid_signature() {
    let app = create_app(AppState::in_memory_with_secret(TEST_SECRET.to_string()));

    let body = serde_json::json!({ "zen": "Design for failure." }).to_string();

    let req = Request::builder()
        .uri("/webhooks/github")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "ping")
        .header(
            "x-hub-signature-256",
            "sha256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .body(Body::from(body))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let json = response_json(res.into_body()).await;
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("Invalid webhook signature")
    );
}

#[tokio::test]
async fn test_webhook_missing_signature() {
    let app = create_app(AppState::in_memory_with_secret(TEST_SECRET.to_string()));

    let req = Request::builder()
        .uri("/webhooks/github")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "ping")
        .body(Body::from("{}"))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_webhook_push_event_creates_build() {
    let app = create_app(AppState::in_memory_with_secret(TEST_SECRET.to_string()));
    let project_id =
        create_test_project(&app, "https://github.com/octocat/hello-world", "main").await;

    let push_body = serde_json::json!({
        "ref": "refs/heads/main",
        "after": "c0ffee1234567890abcdef",
        "repository": {
            "full_name": "octocat/hello-world",
            "clone_url": "https://github.com/octocat/hello-world.git",
            "html_url": "https://github.com/octocat/hello-world"
        }
    })
    .to_string();

    let sig = compute_signature(TEST_SECRET, push_body.as_bytes());

    let req = Request::builder()
        .uri("/webhooks/github")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-github-delivery", "delivery-001")
        .header("x-hub-signature-256", sig)
        .body(Body::from(push_body))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);

    let json = response_json(res.into_body()).await;
    assert_eq!(json["status"], "queued");
    assert_eq!(json["commit_sha"], "c0ffee1234567890abcdef");
    let build_id = json["build_id"].as_str().unwrap();

    // 1. Fetch created build by ID
    let get_req = Request::builder()
        .uri(format!("/builds/{build_id}"))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let get_res = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_res.status(), StatusCode::OK);

    let build_json = response_json(get_res.into_body()).await;
    assert_eq!(build_json["id"], build_id);
    assert_eq!(build_json["project_id"], project_id);
    assert_eq!(build_json["commit_sha"], "c0ffee1234567890abcdef");
    assert_eq!(build_json["branch"], "main");
    assert_eq!(build_json["status"], "queued");

    // 2. List builds for project
    let list_req = Request::builder()
        .uri(format!("/projects/{project_id}/builds"))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let list_res = app.oneshot(list_req).await.unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);

    let list_json = response_json(list_res.into_body()).await;
    let list = list_json.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], build_id);
}

#[tokio::test]
async fn test_webhook_idempotent_duplicate_delivery() {
    let app = create_app(AppState::in_memory_with_secret(TEST_SECRET.to_string()));
    let _project_id =
        create_test_project(&app, "https://github.com/octocat/hello-world", "main").await;

    let push_body = serde_json::json!({
        "ref": "refs/heads/main",
        "after": "112233445566",
        "repository": {
            "full_name": "octocat/hello-world"
        }
    })
    .to_string();

    let sig = compute_signature(TEST_SECRET, push_body.as_bytes());

    // First delivery
    let req1 = Request::builder()
        .uri("/webhooks/github")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-github-delivery", "unique-delivery-id-999")
        .header("x-hub-signature-256", sig.clone())
        .body(Body::from(push_body.clone()))
        .unwrap();

    let res1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(res1.status(), StatusCode::ACCEPTED);

    // Second delivery with same delivery ID (duplicate retry)
    let req2 = Request::builder()
        .uri("/webhooks/github")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-github-delivery", "unique-delivery-id-999")
        .header("x-hub-signature-256", sig)
        .body(Body::from(push_body))
        .unwrap();

    let res2 = app.oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::OK);

    let json2 = response_json(res2.into_body()).await;
    assert_eq!(json2["status"], "already_processed");
}

#[tokio::test]
async fn test_webhook_push_ignored_conditions() {
    let app = create_app(AppState::in_memory_with_secret(TEST_SECRET.to_string()));
    let _project_id =
        create_test_project(&app, "https://github.com/octocat/tracked-repo", "main").await;

    // Condition A: Untracked repository
    let untracked_body = serde_json::json!({
        "ref": "refs/heads/main",
        "after": "aabbcc",
        "repository": { "full_name": "unknown/different-repo" }
    })
    .to_string();

    let sig_untracked = compute_signature(TEST_SECRET, untracked_body.as_bytes());
    let req_untracked = Request::builder()
        .uri("/webhooks/github")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-hub-signature-256", sig_untracked)
        .body(Body::from(untracked_body))
        .unwrap();

    let res_untracked = app.clone().oneshot(req_untracked).await.unwrap();
    assert_eq!(res_untracked.status(), StatusCode::OK);
    let json_untracked = response_json(res_untracked.into_body()).await;
    assert_eq!(json_untracked["status"], "ignored_untracked_repo");

    // Condition B: Non-default branch push
    let branch_body = serde_json::json!({
        "ref": "refs/heads/feature-branch",
        "after": "ddeeff",
        "repository": { "full_name": "octocat/tracked-repo" }
    })
    .to_string();

    let sig_branch = compute_signature(TEST_SECRET, branch_body.as_bytes());
    let req_branch = Request::builder()
        .uri("/webhooks/github")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-hub-signature-256", sig_branch)
        .body(Body::from(branch_body))
        .unwrap();

    let res_branch = app.clone().oneshot(req_branch).await.unwrap();
    assert_eq!(res_branch.status(), StatusCode::OK);
    let json_branch = response_json(res_branch.into_body()).await;
    assert_eq!(json_branch["status"], "ignored_non_default_branch");

    // Condition C: Branch deletion push (after is 40 zeros)
    let del_body = serde_json::json!({
        "ref": "refs/heads/main",
        "after": "0000000000000000000000000000000000000000",
        "repository": { "full_name": "octocat/tracked-repo" }
    })
    .to_string();

    let sig_del = compute_signature(TEST_SECRET, del_body.as_bytes());
    let req_del = Request::builder()
        .uri("/webhooks/github")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-hub-signature-256", sig_del)
        .body(Body::from(del_body))
        .unwrap();

    let res_del = app.oneshot(req_del).await.unwrap();
    assert_eq!(res_del.status(), StatusCode::OK);
    let json_del = response_json(res_del.into_body()).await;
    assert_eq!(json_del["status"], "ignored_branch_deletion");
}

#[tokio::test]
async fn test_get_nonexistent_build_returns_404() {
    let app = create_app(AppState::in_memory());

    let fake_id = Uuid::new_v4();
    let req = Request::builder()
        .uri(format!("/builds/{fake_id}"))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
