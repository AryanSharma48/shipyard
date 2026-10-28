use shipyard_api::{AppState, create_app};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let state = AppState::in_memory();
    let app = create_app(state);

    let listener = TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("Failed to bind TCP listener");

    println!("Shipyard API listening on http://127.0.0.1:3000");

    axum::serve(listener, app).await.expect("Server failed");
}

#[cfg(test)]
mod tests {
    use shipyard_api::routes::health::health;

    #[tokio::test]
    async fn health_returns_ok() {
        let response = health().await;
        let body = response.0;

        assert_eq!(body.status, "ok");
        assert_eq!(body.service, "shipyard-api");
    }
}
