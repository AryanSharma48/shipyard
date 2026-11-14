use chrono::Utc;
use shipyard_api::adapter::MockRuntimeAdapter;
use shipyard_api::error::ApiError;
use shipyard_api::models::{Build, BuildStatus, Deployment, DeploymentStatus, Project, TargetType};
use shipyard_api::orchestrator::execute_deployment;
use shipyard_api::store::{PgProjectStore, ProjectStore};
use std::sync::Arc;
use uuid::Uuid;

fn load_database_url() -> Option<String> {
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_filename("../../.env");

    std::env::var("DATABASE_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn get_pg_store() -> Option<PgProjectStore> {
    let db_url = match load_database_url() {
        Some(url) => url,
        None => {
            println!("Skipping PostgreSQL integration test: DATABASE_URL is not configured");
            return None;
        }
    };

    let store = PgProjectStore::connect(&db_url)
        .await
        .expect("Failed to connect to PostgreSQL database");
    store
        .run_migrations()
        .await
        .expect("Failed to run SQLx database migrations");
    Some(store)
}

async fn cleanup_project(store: &PgProjectStore, project_id: Uuid) {
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(store.pool())
        .await
        .expect("Failed to clean up test project");
}

async fn cleanup_delivery(store: &PgProjectStore, delivery_id: &str) {
    sqlx::query("DELETE FROM webhook_deliveries WHERE delivery_id = $1")
        .bind(delivery_id)
        .execute(store.pool())
        .await
        .expect("Failed to clean up test webhook delivery");
}

#[tokio::test]
async fn test_pg_migrations_and_project_crud_and_repo_lookup() {
    let Some(store) = get_pg_store().await else {
        return;
    };

    let project_id = Uuid::new_v4();
    let repo_slug = format!("shipyard-test/repo-{}", project_id.simple());
    let now = Utc::now();
    let project = Project {
        id: project_id,
        name: format!("pg-test-{}", project_id.simple()),
        repo_url: format!("https://github.com/{repo_slug}.git"),
        default_branch: "main".to_string(),
        created_at: now,
        updated_at: now,
    };

    let created = store.create_project(project.clone()).await.unwrap();
    assert_eq!(created.id, project_id);

    let fetched = store.get_project(project_id).await.unwrap().unwrap();
    assert_eq!(fetched.name, project.name);
    assert_eq!(fetched.repo_url, project.repo_url);

    let listed = store.list_projects().await.unwrap();
    assert!(listed.iter().any(|p| p.id == project_id));

    let found_by_full_name = store.find_project_by_repo(&repo_slug).await.unwrap();
    assert!(found_by_full_name.is_some());
    assert_eq!(found_by_full_name.unwrap().id, project_id);

    let found_by_url = store
        .find_project_by_repo(&format!("https://github.com/{repo_slug}/"))
        .await
        .unwrap();
    assert!(found_by_url.is_some());
    assert_eq!(found_by_url.unwrap().id, project_id);

    cleanup_project(&store, project_id).await;
}

#[tokio::test]
async fn test_pg_build_and_deployment_lifecycle_and_superseding() {
    let Some(store) = get_pg_store().await else {
        return;
    };

    let project_id = Uuid::new_v4();
    let now = Utc::now();
    let project = Project {
        id: project_id,
        name: format!("pg-lifecycle-{}", project_id.simple()),
        repo_url: format!(
            "https://github.com/shipyard-test/lifecycle-{}",
            project_id.simple()
        ),
        default_branch: "main".to_string(),
        created_at: now,
        updated_at: now,
    };
    store.create_project(project).await.unwrap();

    // 1. Build lifecycle: Queued -> Building -> Succeeded
    let build_id = Uuid::new_v4();
    let build = Build {
        id: build_id,
        project_id,
        commit_sha: "abc123def456".to_string(),
        branch: "main".to_string(),
        status: BuildStatus::Queued,
        image_digest: None,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        failure_reason: None,
    };
    store.create_build(build).await.unwrap();

    let building = store
        .update_build_status(build_id, BuildStatus::Building, None, None)
        .await
        .unwrap();
    assert_eq!(building.status, BuildStatus::Building);
    assert!(building.started_at.is_some());

    let succeeded = store
        .update_build_status(
            build_id,
            BuildStatus::Succeeded,
            Some("sha256:deadbeefcafebabe".to_string()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(succeeded.status, BuildStatus::Succeeded);
    assert_eq!(
        succeeded.image_digest.as_deref(),
        Some("sha256:deadbeefcafebabe")
    );
    assert!(succeeded.finished_at.is_some());

    let project_builds = store.list_builds_by_project(project_id).await.unwrap();
    assert_eq!(project_builds.len(), 1);

    // 2. Run two deployments via shared orchestrator and verify Healthy -> Superseded
    let store_arc: Arc<dyn ProjectStore> = Arc::new(store.clone());
    let adapter: Arc<dyn shipyard_api::adapter::RuntimeAdapter> =
        Arc::new(MockRuntimeAdapter::new());

    let first_dep = execute_deployment(&store_arc, &adapter, project_id, "sha-v1", None)
        .await
        .unwrap();
    assert_eq!(first_dep.status, DeploymentStatus::Healthy);

    let second_dep = execute_deployment(&store_arc, &adapter, project_id, "sha-v2", None)
        .await
        .unwrap();
    assert_eq!(second_dep.status, DeploymentStatus::Healthy);

    let refreshed_first = store.get_deployment(first_dep.id).await.unwrap().unwrap();
    assert_eq!(refreshed_first.status, DeploymentStatus::Superseded);

    // 3. Overlapping active deployment guard returns ApiError::Conflict
    let active_dep = Deployment {
        id: Uuid::new_v4(),
        project_id,
        commit_sha: "sha-active".to_string(),
        target_type: TargetType::HostedEcs,
        status: DeploymentStatus::Queued,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        failure_reason: None,
    };
    store.create_deployment(active_dep).await.unwrap();

    let overlapping = Deployment {
        id: Uuid::new_v4(),
        project_id,
        commit_sha: "sha-overlap".to_string(),
        target_type: TargetType::HostedEcs,
        status: DeploymentStatus::Queued,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        failure_reason: None,
    };
    let conflict_err = store.create_deployment(overlapping).await.unwrap_err();
    assert!(matches!(conflict_err, ApiError::Conflict(_)));

    cleanup_project(&store, project_id).await;
}

#[tokio::test]
async fn test_pg_control_plane_restart_durability() {
    let Some(db_url) = load_database_url() else {
        println!("Skipping PostgreSQL durability test: DATABASE_URL is not configured");
        return;
    };

    let project_id = Uuid::new_v4();
    let build_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();
    let delivery_id = format!("gh-delivery-{}", Uuid::new_v4().simple());

    // Control plane instance 1 creates durable records and is dropped (simulating server restart)
    {
        let store_instance_1 = PgProjectStore::connect(&db_url).await.unwrap();
        store_instance_1.run_migrations().await.unwrap();

        let now = Utc::now();
        store_instance_1
            .create_project(Project {
                id: project_id,
                name: format!("durability-app-{}", project_id.simple()),
                repo_url: format!(
                    "https://github.com/shipyard-test/durability-{}",
                    project_id.simple()
                ),
                default_branch: "main".to_string(),
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();

        store_instance_1
            .record_delivery(delivery_id.clone())
            .await
            .unwrap();

        store_instance_1
            .create_build(Build {
                id: build_id,
                project_id,
                commit_sha: "durability-sha-001".to_string(),
                branch: "main".to_string(),
                status: BuildStatus::Queued,
                image_digest: None,
                created_at: now,
                started_at: None,
                finished_at: None,
                failure_reason: None,
            })
            .await
            .unwrap();

        store_instance_1
            .create_deployment(Deployment {
                id: deployment_id,
                project_id,
                commit_sha: "durability-sha-001".to_string(),
                target_type: TargetType::HostedEcs,
                status: DeploymentStatus::Queued,
                created_at: now,
                started_at: None,
                finished_at: None,
                failure_reason: None,
            })
            .await
            .unwrap();
    }

    // Control plane instance 2 boots with a fresh connection pool
    let store_instance_2 = PgProjectStore::connect(&db_url).await.unwrap();

    let recovered_project = store_instance_2.get_project(project_id).await.unwrap();
    assert!(recovered_project.is_some());
    assert_eq!(recovered_project.unwrap().id, project_id);

    let recovered_build = store_instance_2.get_build(build_id).await.unwrap();
    assert!(recovered_build.is_some());
    assert_eq!(recovered_build.unwrap().status, BuildStatus::Queued);

    let recovered_deployment = store_instance_2
        .get_deployment(deployment_id)
        .await
        .unwrap();
    assert!(recovered_deployment.is_some());
    assert_eq!(
        recovered_deployment.unwrap().status,
        DeploymentStatus::Queued
    );

    let delivery_still_recorded = store_instance_2
        .is_delivery_processed(&delivery_id)
        .await
        .unwrap();
    assert!(delivery_still_recorded);

    cleanup_project(&store_instance_2, project_id).await;
    cleanup_delivery(&store_instance_2, &delivery_id).await;
}
