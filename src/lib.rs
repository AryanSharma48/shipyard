pub mod adapter;
pub mod error;
pub mod models;
pub mod routes;
pub mod store;

use adapter::RuntimeAdapter;
use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;
use store::ProjectStore;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn ProjectStore>,
    pub adapter: Arc<dyn RuntimeAdapter>,
}

impl AppState {
    pub fn new(store: Arc<dyn ProjectStore>, adapter: Arc<dyn RuntimeAdapter>) -> Self {
        Self { store, adapter }
    }

    pub fn in_memory() -> Self {
        Self {
            store: Arc::new(store::MemoryProjectStore::new()),
            adapter: Arc::new(adapter::MockRuntimeAdapter::new()),
        }
    }

    pub fn in_memory_with_adapter(adapter: Arc<dyn RuntimeAdapter>) -> Self {
        Self {
            store: Arc::new(store::MemoryProjectStore::new()),
            adapter,
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
        .route("/deployments", post(routes::deployments::create_deployment))
        .route(
            "/deployments/{id}",
            get(routes::deployments::get_deployment),
        )
        .with_state(state)
}
