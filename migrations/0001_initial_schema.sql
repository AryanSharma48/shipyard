-- Shipyard Cloud: Initial Control Plane Schema (Phase 2)
-- Compatible with PostgreSQL 15+ (Neon / RDS / Self-hosted)

CREATE TABLE IF NOT EXISTS projects (
    id UUID PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    repo_url TEXT NOT NULL,
    default_branch VARCHAR(255) NOT NULL DEFAULT 'main',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS environments (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT unique_project_env_name UNIQUE (project_id, name)
);

CREATE TABLE IF NOT EXISTS deployments (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    commit_sha VARCHAR(64) NOT NULL,
    target_type VARCHAR(64) NOT NULL DEFAULT 'hosted_ecs',
    status VARCHAR(32) NOT NULL DEFAULT 'queued',
    failure_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    CONSTRAINT check_deployment_target CHECK (
        target_type IN ('hosted_ecs', 'customer_aws_ecs')
    ),
    CONSTRAINT check_deployment_status CHECK (
        status IN ('queued', 'deploying', 'verifying', 'healthy', 'failed', 'rolling_back', 'rolled_back')
    )
);

CREATE INDEX IF NOT EXISTS idx_projects_created_at ON projects (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_environments_project_id ON environments (project_id);
CREATE INDEX IF NOT EXISTS idx_deployments_project_id ON deployments (project_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_deployments_status ON deployments (status);
