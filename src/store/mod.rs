use crate::error::ApiError;
use crate::models::Project;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ProjectStore: Send + Sync {
    fn create_project(&self, project: Project) -> BoxFuture<'_, Result<Project, ApiError>>;
    fn get_project(&self, id: Uuid) -> BoxFuture<'_, Result<Option<Project>, ApiError>>;
    fn list_projects(&self) -> BoxFuture<'_, Result<Vec<Project>, ApiError>>;
}

#[derive(Clone, Default)]
pub struct MemoryProjectStore {
    projects: Arc<RwLock<HashMap<Uuid, Project>>>,
}

impl MemoryProjectStore {
    pub fn new() -> Self {
        Self {
            projects: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl ProjectStore for MemoryProjectStore {
    fn create_project(&self, project: Project) -> BoxFuture<'_, Result<Project, ApiError>> {
        Box::pin(async move {
            let mut lock = self.projects.write().await;
            lock.insert(project.id, project.clone());
            Ok(project)
        })
    }

    fn get_project(&self, id: Uuid) -> BoxFuture<'_, Result<Option<Project>, ApiError>> {
        Box::pin(async move {
            let lock = self.projects.read().await;
            Ok(lock.get(&id).cloned())
        })
    }

    fn list_projects(&self) -> BoxFuture<'_, Result<Vec<Project>, ApiError>> {
        Box::pin(async move {
            let lock = self.projects.read().await;
            let mut list: Vec<Project> = lock.values().cloned().collect();
            list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            Ok(list)
        })
    }
}
