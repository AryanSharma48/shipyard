use shipyard_api::builder::BuildWorker;
use shipyard_api::store::PgProjectStore;
use shipyard_api::{AppState, create_app};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    let state = match std::env::var("DATABASE_URL") {
        Ok(db_url) if !db_url.trim().is_empty() => {
            let pg_store = PgProjectStore::connect(&db_url)
                .await
                .expect("Failed to connect to PostgreSQL");
            pg_store
                .run_migrations()
                .await
                .expect("Failed to run PostgreSQL migrations");
            println!("Connected to PostgreSQL and verified schema migrations");
            AppState::with_store(Arc::new(pg_store))
        }
        _ => {
            println!("DATABASE_URL not set; starting with in-memory store");
            AppState::in_memory()
        }
    };

    let worker = BuildWorker::with_adapter(
        state.store.clone(),
        state.queue.clone(),
        state.adapter.clone(),
    );
    tokio::spawn(async move {
        loop {
            match worker.process_one_job_with_deployment().await {
                Ok(Some((build, _))) => {
                    println!("Processed build '{}' ({:?})", build.id, build.status);
                }
                Ok(None) => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Err(err) => {
                    eprintln!("Build worker error: {err}");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    });

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
