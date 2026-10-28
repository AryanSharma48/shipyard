use crate::AppState;
use crate::error::ApiError;
use crate::models::{Build, BuildStatus, GitHubPushEvent};
use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

pub fn verify_signature(
    secret: &[u8],
    body: &[u8],
    signature_header: Option<&str>,
) -> Result<(), ApiError> {
    let header_val = signature_header
        .ok_or_else(|| ApiError::Unauthorized("Missing X-Hub-Signature-256 header".to_string()))?;

    let hex_sig = header_val
        .strip_prefix("sha256=")
        .ok_or_else(|| ApiError::Unauthorized("Invalid signature format".to_string()))?;

    let decoded_sig = hex::decode(hex_sig)
        .map_err(|_| ApiError::Unauthorized("Invalid hex signature".to_string()))?;

    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|_| ApiError::Internal("HMAC initialization failed".to_string()))?;
    mac.update(body);

    mac.verify_slice(&decoded_sig)
        .map_err(|_| ApiError::Unauthorized("Invalid webhook signature".to_string()))
}

pub async fn handle_github_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    // 1. Verify cryptographic signature
    let sig_header = headers
        .get("x-hub-signature-256")
        .and_then(|h| h.to_str().ok());
    verify_signature(state.webhook_secret.as_bytes(), &body, sig_header)?;

    // 2. Check for duplicate delivery (idempotency)
    if let Some(delivery_id) = headers
        .get("x-github-delivery")
        .and_then(|h| h.to_str().ok())
    {
        if state.store.is_delivery_processed(delivery_id).await? {
            return Ok((
                StatusCode::OK,
                Json(serde_json::json!({ "status": "already_processed" })),
            ));
        }
        state.store.record_delivery(delivery_id.to_string()).await?;
    }

    // 3. Inspect GitHub event type
    let event_type = headers
        .get("x-github-event")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    match event_type {
        "ping" => Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "status": "pong" })),
        )),
        "push" => {
            let push_event: GitHubPushEvent = serde_json::from_slice(&body).map_err(|err| {
                ApiError::BadRequest(format!("Failed to parse push event: {err}"))
            })?;

            if push_event.is_branch_deletion() {
                return Ok((
                    StatusCode::OK,
                    Json(serde_json::json!({ "status": "ignored_branch_deletion" })),
                ));
            }

            // Resolve matching project by repo full name or URL
            let project = match state
                .store
                .find_project_by_repo(&push_event.repository.full_name)
                .await?
            {
                Some(p) => p,
                None => {
                    return Ok((
                        StatusCode::OK,
                        Json(serde_json::json!({ "status": "ignored_untracked_repo" })),
                    ));
                }
            };

            // Filter for default or target branch
            if project.default_branch != push_event.branch_name() {
                return Ok((
                    StatusCode::OK,
                    Json(serde_json::json!({ "status": "ignored_non_default_branch" })),
                ));
            }

            let now = Utc::now();
            let build = Build {
                id: Uuid::new_v4(),
                project_id: project.id,
                commit_sha: push_event.after.clone(),
                branch: push_event.branch_name().to_string(),
                status: BuildStatus::Queued,
                image_digest: None,
                created_at: now,
                started_at: None,
                finished_at: None,
                failure_reason: None,
            };

            let created = state.store.create_build(build).await?;

            // Enqueue build job for background worker processing
            let job = crate::queue::BuildJob::new(
                created.id,
                project.id,
                created.commit_sha.clone(),
                created.branch.clone(),
                project.repo_url.clone(),
            );
            state.queue.enqueue(job).await?;

            Ok((
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "status": "queued",
                    "build_id": created.id,
                    "commit_sha": created.commit_sha
                })),
            ))
        }
        _ => Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "status": "ignored_unhandled_event" })),
        )),
    }
}
