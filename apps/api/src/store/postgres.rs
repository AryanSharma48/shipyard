use crate::error::ApiError;
use crate::models::{Build, BuildStatus, Deployment, DeploymentStatus, Project, TargetType};
use crate::store::{BoxFuture, MemoryProjectStore, ProjectStore};
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{FromRow, PgPool};
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

/// PostgreSQL-backed implementation of [`ProjectStore`] using an async `sqlx::PgPool`.
#[derive(Clone)]
pub struct PgProjectStore {
    pool: PgPool,
}

#[derive(FromRow)]
struct ProjectRow {
    id: Uuid,
    name: String,
    repo_url: String,
    default_branch: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ProjectRow> for Project {
    fn from(row: ProjectRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            repo_url: row.repo_url,
            default_branch: row.default_branch,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(FromRow)]
struct DeploymentRow {
    id: Uuid,
    project_id: Uuid,
    commit_sha: String,
    target_type: String,
    status: String,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    failure_reason: Option<String>,
}

impl DeploymentRow {
    fn into_domain(self) -> Result<Deployment, ApiError> {
        Ok(Deployment {
            id: self.id,
            project_id: self.project_id,
            commit_sha: self.commit_sha,
            target_type: TargetType::parse(&self.target_type)?,
            status: DeploymentStatus::parse(&self.status)?,
            created_at: self.created_at,
            started_at: self.started_at,
            finished_at: self.finished_at,
            failure_reason: self.failure_reason,
        })
    }
}

#[derive(FromRow)]
struct BuildRow {
    id: Uuid,
    project_id: Uuid,
    commit_sha: String,
    branch: String,
    status: String,
    image_digest: Option<String>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    failure_reason: Option<String>,
}

impl BuildRow {
    fn into_domain(self) -> Result<Build, ApiError> {
        Ok(Build {
            id: self.id,
            project_id: self.project_id,
            commit_sha: self.commit_sha,
            branch: self.branch,
            status: BuildStatus::parse(&self.status)?,
            image_digest: self.image_digest,
            created_at: self.created_at,
            started_at: self.started_at,
            finished_at: self.finished_at,
            failure_reason: self.failure_reason,
        })
    }
}

impl PgProjectStore {
    /// Creates a store wrapping an existing PostgreSQL connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Connects to PostgreSQL with connection pooling configured for long-running control plane servers.
    pub async fn connect(database_url: &str) -> Result<Self, ApiError> {
        let sanitized = sanitize_database_url(database_url);
        let pool_options = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(15));

        match PgConnectOptions::from_str(&sanitized) {
            Ok(connect_opts) => match pool_options.clone().connect_with(connect_opts).await {
                Ok(pool) => Ok(Self { pool }),
                Err(err) => {
                    if let Some(stripped) = strip_channel_binding(&sanitized) {
                        let fallback_opts = PgConnectOptions::from_str(&stripped).map_err(|e| {
                            ApiError::Internal(format!("Invalid PostgreSQL connection URL: {e}"))
                        })?;
                        let pool = pool_options
                            .connect_with(fallback_opts)
                            .await
                            .map_err(map_sqlx_error)?;
                        Ok(Self { pool })
                    } else {
                        Err(map_sqlx_error(err))
                    }
                }
            },
            Err(err) => {
                if let Some(stripped) = strip_channel_binding(&sanitized) {
                    let fallback_opts = PgConnectOptions::from_str(&stripped).map_err(|e| {
                        ApiError::Internal(format!("Invalid PostgreSQL connection URL: {e}"))
                    })?;
                    let pool = pool_options
                        .connect_with(fallback_opts)
                        .await
                        .map_err(map_sqlx_error)?;
                    Ok(Self { pool })
                } else {
                    Err(ApiError::Internal(format!(
                        "Invalid PostgreSQL connection URL: {err}"
                    )))
                }
            }
        }
    }

    /// Returns a reference to the underlying connection pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Applies embedded SQL migrations from `apps/api/migrations/`.
    pub async fn run_migrations(&self) -> Result<(), ApiError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|err| ApiError::Internal(format!("Database migration failed: {err}")))
    }
}

fn sanitize_database_url(raw: &str) -> String {
    raw.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn strip_channel_binding(url: &str) -> Option<String> {
    let (base, query) = url.split_once('?')?;
    let mut had_channel_binding = false;
    let remaining: Vec<&str> = query
        .split('&')
        .filter(|param| {
            let is_cb = param.starts_with("channel_binding=");
            if is_cb {
                had_channel_binding = true;
            }
            !is_cb && !param.is_empty()
        })
        .collect();

    if !had_channel_binding {
        return None;
    }

    if remaining.is_empty() {
        Some(base.to_string())
    } else {
        Some(format!("{base}?{}", remaining.join("&")))
    }
}

fn map_sqlx_error(err: sqlx::Error) -> ApiError {
    match &err {
        sqlx::Error::RowNotFound => {
            ApiError::NotFound("Requested record was not found".to_string())
        }
        sqlx::Error::Database(db_err) => match db_err.code().as_deref() {
            Some("23505") => ApiError::Conflict(format!("Database conflict: {}", db_err.message())),
            Some("23503") => {
                ApiError::NotFound(format!("Referenced record not found: {}", db_err.message()))
            }
            _ => ApiError::Internal(format!("Database error: {err}")),
        },
        _ => ApiError::Internal(format!("Database error: {err}")),
    }
}

impl ProjectStore for PgProjectStore {
    fn create_project(&self, project: Project) -> BoxFuture<'_, Result<Project, ApiError>> {
        Box::pin(async move {
            let row = sqlx::query_as::<_, ProjectRow>(
                r#"
                INSERT INTO projects (id, name, repo_url, default_branch, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6)
                RETURNING id, name, repo_url, default_branch, created_at, updated_at
                "#,
            )
            .bind(project.id)
            .bind(&project.name)
            .bind(&project.repo_url)
            .bind(&project.default_branch)
            .bind(project.created_at)
            .bind(project.updated_at)
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

            Ok(row.into())
        })
    }

    fn get_project(&self, id: Uuid) -> BoxFuture<'_, Result<Option<Project>, ApiError>> {
        Box::pin(async move {
            let row = sqlx::query_as::<_, ProjectRow>(
                r#"
                SELECT id, name, repo_url, default_branch, created_at, updated_at
                FROM projects
                WHERE id = $1
                "#,
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

            Ok(row.map(Into::into))
        })
    }

    fn list_projects(&self) -> BoxFuture<'_, Result<Vec<Project>, ApiError>> {
        Box::pin(async move {
            let rows = sqlx::query_as::<_, ProjectRow>(
                r#"
                SELECT id, name, repo_url, default_branch, created_at, updated_at
                FROM projects
                ORDER BY created_at DESC
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

            Ok(rows.into_iter().map(Into::into).collect())
        })
    }

    fn find_project_by_repo(
        &self,
        repo_identifier: &str,
    ) -> BoxFuture<'_, Result<Option<Project>, ApiError>> {
        let target = MemoryProjectStore::normalize_repo_url(repo_identifier);
        Box::pin(async move {
            let rows = sqlx::query_as::<_, ProjectRow>(
                r#"
                SELECT id, name, repo_url, default_branch, created_at, updated_at
                FROM projects
                ORDER BY created_at DESC
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

            for row in rows {
                let norm = MemoryProjectStore::normalize_repo_url(&row.repo_url);
                if norm == target || norm.ends_with(&format!("/{target}")) {
                    return Ok(Some(row.into()));
                }
            }

            Ok(None)
        })
    }

    fn create_deployment(
        &self,
        deployment: Deployment,
    ) -> BoxFuture<'_, Result<Deployment, ApiError>> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

            let active_id: Option<Uuid> = sqlx::query_scalar(
                r#"
                SELECT id
                FROM deployments
                WHERE project_id = $1
                  AND status IN ('queued', 'deploying', 'verifying')
                LIMIT 1
                FOR UPDATE
                "#,
            )
            .bind(deployment.project_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

            if active_id.is_some() {
                return Err(ApiError::Conflict(format!(
                    "Project '{}' already has an active deployment in progress",
                    deployment.project_id
                )));
            }

            let row = sqlx::query_as::<_, DeploymentRow>(
                r#"
                INSERT INTO deployments (
                    id, project_id, commit_sha, target_type, status,
                    failure_reason, created_at, started_at, finished_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                RETURNING id, project_id, commit_sha, target_type, status,
                          created_at, started_at, finished_at, failure_reason
                "#,
            )
            .bind(deployment.id)
            .bind(deployment.project_id)
            .bind(&deployment.commit_sha)
            .bind(deployment.target_type.as_str())
            .bind(deployment.status.as_str())
            .bind(&deployment.failure_reason)
            .bind(deployment.created_at)
            .bind(deployment.started_at)
            .bind(deployment.finished_at)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

            tx.commit().await.map_err(map_sqlx_error)?;

            row.into_domain()
        })
    }

    fn get_deployment(&self, id: Uuid) -> BoxFuture<'_, Result<Option<Deployment>, ApiError>> {
        Box::pin(async move {
            let row = sqlx::query_as::<_, DeploymentRow>(
                r#"
                SELECT id, project_id, commit_sha, target_type, status,
                       created_at, started_at, finished_at, failure_reason
                FROM deployments
                WHERE id = $1
                "#,
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

            row.map(DeploymentRow::into_domain).transpose()
        })
    }

    fn list_deployments_by_project(
        &self,
        project_id: Uuid,
    ) -> BoxFuture<'_, Result<Vec<Deployment>, ApiError>> {
        Box::pin(async move {
            let rows = sqlx::query_as::<_, DeploymentRow>(
                r#"
                SELECT id, project_id, commit_sha, target_type, status,
                       created_at, started_at, finished_at, failure_reason
                FROM deployments
                WHERE project_id = $1
                ORDER BY created_at DESC
                "#,
            )
            .bind(project_id)
            .fetch_all(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

            rows.into_iter().map(DeploymentRow::into_domain).collect()
        })
    }

    fn update_deployment_status(
        &self,
        id: Uuid,
        new_status: DeploymentStatus,
        failure_reason: Option<String>,
    ) -> BoxFuture<'_, Result<Deployment, ApiError>> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

            let current_row = sqlx::query_as::<_, DeploymentRow>(
                r#"
                SELECT id, project_id, commit_sha, target_type, status,
                       created_at, started_at, finished_at, failure_reason
                FROM deployments
                WHERE id = $1
                FOR UPDATE
                "#,
            )
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_error)?
            .ok_or_else(|| ApiError::NotFound(format!("Deployment with ID '{id}' not found")))?;

            let current = current_row.into_domain()?;
            if !current.status.can_transition_to(&new_status) {
                return Err(ApiError::Conflict(format!(
                    "Cannot transition deployment '{id}' from '{:?}' to '{:?}'",
                    current.status, new_status
                )));
            }

            let now = Utc::now();
            let started_at =
                if new_status == DeploymentStatus::Deploying && current.started_at.is_none() {
                    Some(now)
                } else {
                    current.started_at
                };

            let finished_at = if matches!(
                new_status,
                DeploymentStatus::Healthy | DeploymentStatus::Failed | DeploymentStatus::RolledBack
            ) {
                Some(now)
            } else {
                current.finished_at
            };

            let next_failure_reason = if failure_reason.is_some() {
                failure_reason
            } else {
                current.failure_reason
            };

            if new_status == DeploymentStatus::Healthy {
                sqlx::query(
                    r#"
                    UPDATE deployments
                    SET status = 'superseded'
                    WHERE project_id = $1
                      AND id <> $2
                      AND status = 'healthy'
                    "#,
                )
                .bind(current.project_id)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
            }

            let updated_row = sqlx::query_as::<_, DeploymentRow>(
                r#"
                UPDATE deployments
                SET status = $2,
                    started_at = $3,
                    finished_at = $4,
                    failure_reason = $5
                WHERE id = $1
                RETURNING id, project_id, commit_sha, target_type, status,
                          created_at, started_at, finished_at, failure_reason
                "#,
            )
            .bind(id)
            .bind(new_status.as_str())
            .bind(started_at)
            .bind(finished_at)
            .bind(next_failure_reason)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

            tx.commit().await.map_err(map_sqlx_error)?;

            updated_row.into_domain()
        })
    }

    fn create_build(&self, build: Build) -> BoxFuture<'_, Result<Build, ApiError>> {
        Box::pin(async move {
            let row = sqlx::query_as::<_, BuildRow>(
                r#"
                INSERT INTO builds (
                    id, project_id, commit_sha, branch, status, image_digest,
                    created_at, started_at, finished_at, failure_reason
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                RETURNING id, project_id, commit_sha, branch, status, image_digest,
                          created_at, started_at, finished_at, failure_reason
                "#,
            )
            .bind(build.id)
            .bind(build.project_id)
            .bind(&build.commit_sha)
            .bind(&build.branch)
            .bind(build.status.as_str())
            .bind(&build.image_digest)
            .bind(build.created_at)
            .bind(build.started_at)
            .bind(build.finished_at)
            .bind(&build.failure_reason)
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

            row.into_domain()
        })
    }

    fn get_build(&self, id: Uuid) -> BoxFuture<'_, Result<Option<Build>, ApiError>> {
        Box::pin(async move {
            let row = sqlx::query_as::<_, BuildRow>(
                r#"
                SELECT id, project_id, commit_sha, branch, status, image_digest,
                       created_at, started_at, finished_at, failure_reason
                FROM builds
                WHERE id = $1
                "#,
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

            row.map(BuildRow::into_domain).transpose()
        })
    }

    fn list_builds_by_project(
        &self,
        project_id: Uuid,
    ) -> BoxFuture<'_, Result<Vec<Build>, ApiError>> {
        Box::pin(async move {
            let rows = sqlx::query_as::<_, BuildRow>(
                r#"
                SELECT id, project_id, commit_sha, branch, status, image_digest,
                       created_at, started_at, finished_at, failure_reason
                FROM builds
                WHERE project_id = $1
                ORDER BY created_at DESC
                "#,
            )
            .bind(project_id)
            .fetch_all(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

            rows.into_iter().map(BuildRow::into_domain).collect()
        })
    }

    fn update_build_status(
        &self,
        id: Uuid,
        new_status: BuildStatus,
        image_digest: Option<String>,
        failure_reason: Option<String>,
    ) -> BoxFuture<'_, Result<Build, ApiError>> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

            let current_row = sqlx::query_as::<_, BuildRow>(
                r#"
                SELECT id, project_id, commit_sha, branch, status, image_digest,
                       created_at, started_at, finished_at, failure_reason
                FROM builds
                WHERE id = $1
                FOR UPDATE
                "#,
            )
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_error)?
            .ok_or_else(|| ApiError::NotFound(format!("Build with ID '{id}' not found")))?;

            let current = current_row.into_domain()?;
            if !current.status.can_transition_to(&new_status) {
                return Err(ApiError::Conflict(format!(
                    "Cannot transition build '{id}' from '{:?}' to '{:?}'",
                    current.status, new_status
                )));
            }

            let now = Utc::now();
            let started_at = if new_status == BuildStatus::Building && current.started_at.is_none()
            {
                Some(now)
            } else {
                current.started_at
            };

            let finished_at = if matches!(
                new_status,
                BuildStatus::Succeeded | BuildStatus::Failed | BuildStatus::Cancelled
            ) {
                Some(now)
            } else {
                current.finished_at
            };

            let next_image_digest = if image_digest.is_some() {
                image_digest
            } else {
                current.image_digest
            };

            let next_failure_reason = if failure_reason.is_some() {
                failure_reason
            } else {
                current.failure_reason
            };

            let updated_row = sqlx::query_as::<_, BuildRow>(
                r#"
                UPDATE builds
                SET status = $2,
                    started_at = $3,
                    finished_at = $4,
                    image_digest = $5,
                    failure_reason = $6
                WHERE id = $1
                RETURNING id, project_id, commit_sha, branch, status, image_digest,
                          created_at, started_at, finished_at, failure_reason
                "#,
            )
            .bind(id)
            .bind(new_status.as_str())
            .bind(started_at)
            .bind(finished_at)
            .bind(next_image_digest)
            .bind(next_failure_reason)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

            tx.commit().await.map_err(map_sqlx_error)?;

            updated_row.into_domain()
        })
    }

    fn is_delivery_processed(&self, delivery_id: &str) -> BoxFuture<'_, Result<bool, ApiError>> {
        let key = delivery_id.to_string();
        Box::pin(async move {
            let exists: bool = sqlx::query_scalar(
                r#"
                SELECT EXISTS(
                    SELECT 1 FROM webhook_deliveries WHERE delivery_id = $1
                )
                "#,
            )
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

            Ok(exists)
        })
    }

    fn record_delivery(&self, delivery_id: String) -> BoxFuture<'_, Result<(), ApiError>> {
        Box::pin(async move {
            sqlx::query(
                r#"
                INSERT INTO webhook_deliveries (delivery_id, processed_at)
                VALUES ($1, NOW())
                ON CONFLICT (delivery_id) DO NOTHING
                "#,
            )
            .bind(delivery_id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

            Ok(())
        })
    }
}
