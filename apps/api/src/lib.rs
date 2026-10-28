pub mod adapter;
pub mod builder;
pub mod error;
pub mod models;
pub mod orchestrator;
pub mod queue;
pub mod routes;
pub mod store;

use adapter::RuntimeAdapter;
use axum::{
    Router,
    routing::{get, post},
};
use queue::JobQueue;
use std::sync::Arc;
use store::ProjectStore;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn ProjectStore>,
    pub adapter: Arc<dyn RuntimeAdapter>,
    pub queue: Arc<dyn JobQueue>,
    pub webhook_secret: String,
}

impl AppState {
    pub fn new(
        store: Arc<dyn ProjectStore>,
        adapter: Arc<dyn RuntimeAdapter>,
        queue: Arc<dyn JobQueue>,
        webhook_secret: String,
    ) -> Self {
        Self {
            store,
            adapter,
            queue,
            webhook_secret,
        }
    }

    pub fn in_memory() -> Self {
        let secret = std::env::var("GITHUB_WEBHOOK_SECRET")
            .unwrap_or_else(|_| "shipyard-test-secret".to_string());
        Self {
            store: Arc::new(store::MemoryProjectStore::new()),
            adapter: Arc::new(adapter::MockRuntimeAdapter::new()),
            queue: Arc::new(queue::MemoryJobQueue::new()),
            webhook_secret: secret,
        }
    }

    pub fn in_memory_with_secret(webhook_secret: String) -> Self {
        Self {
            store: Arc::new(store::MemoryProjectStore::new()),
            adapter: Arc::new(adapter::MockRuntimeAdapter::new()),
            queue: Arc::new(queue::MemoryJobQueue::new()),
            webhook_secret,
        }
    }

    pub fn in_memory_with_adapter(adapter: Arc<dyn RuntimeAdapter>) -> Self {
        let secret = std::env::var("GITHUB_WEBHOOK_SECRET")
            .unwrap_or_else(|_| "shipyard-test-secret".to_string());
        Self {
            store: Arc::new(store::MemoryProjectStore::new()),
            adapter,
            queue: Arc::new(queue::MemoryJobQueue::new()),
            webhook_secret: secret,
        }
    }

    pub fn in_memory_with_queue(queue: Arc<dyn JobQueue>) -> Self {
        let secret = std::env::var("GITHUB_WEBHOOK_SECRET")
            .unwrap_or_else(|_| "shipyard-test-secret".to_string());
        Self {
            store: Arc::new(store::MemoryProjectStore::new()),
            adapter: Arc::new(adapter::MockRuntimeAdapter::new()),
            queue,
            webhook_secret: secret,
        }
    }
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health::health))
        .route(
            "/projects",
            post(routes::projects::create_project).get(routes::projects::list_projects),
        )
        .route("/projects/{id}", get(routes::projects::get_project))
        .route(
            "/projects/{id}/deployments",
            get(routes::deployments::list_project_deployments),
        )
        .route(
            "/projects/{id}/builds",
            get(routes::builds::list_project_builds),
        )
        .route("/deployments", post(routes::deployments::create_deployment))
        .route(
            "/deployments/{id}",
            get(routes::deployments::get_deployment),
        )
        .route("/builds/{id}", get(routes::builds::get_build))
        .route(
            "/webhooks/github",
            post(routes::webhooks::handle_github_webhook),
        )
        .with_state(state)
}
