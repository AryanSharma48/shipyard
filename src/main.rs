use axum::{Json, Router, routing::get};
use serde::Serialize;
use tokio::net::TcpListener;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "shipyard-api",
    })
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/health", get(health));

    let listener = TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("Failed to bind TCP listener");

    println!("Shipyard API listening on http://127.0.0.1:3000");

    axum::serve(listener, app).await.expect("Server failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_returns_ok() {
        let response = health().await;
        let body = response.0;

        assert_eq!(body.status, "ok");
        assert_eq!(body.service, "shipyard-api");
    }
}
