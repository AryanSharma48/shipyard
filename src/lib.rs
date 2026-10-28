pub mod error;
pub mod models;
pub mod routes;
pub mod store;

use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;
use store::ProjectStore;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn ProjectStore>,
}

impl AppState {
    pub fn new(store: Arc<dyn ProjectStore>) -> Self {
        Self { store }
    }

    pub fn in_memory() -> Self {
        Self {
            store: Arc::new(store::MemoryProjectStore::new()),
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
        .with_state(state)
}
