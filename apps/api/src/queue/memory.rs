use super::{BuildJob, JobQueue};
use crate::error::ApiError;
use crate::store::BoxFuture;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct MemoryJobQueue {
    queue: Arc<RwLock<VecDeque<BuildJob>>>,
}

impl MemoryJobQueue {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    pub async fn len(&self) -> usize {
        let lock = self.queue.read().await;
        lock.len()
    }

    pub async fn is_empty(&self) -> bool {
        let lock = self.queue.read().await;
        lock.is_empty()
    }
}

impl JobQueue for MemoryJobQueue {
    fn enqueue(&self, job: BuildJob) -> BoxFuture<'_, Result<(), ApiError>> {
        Box::pin(async move {
            let mut lock = self.queue.write().await;
            lock.push_back(job);
            Ok(())
        })
    }

    fn dequeue(&self) -> BoxFuture<'_, Result<Option<BuildJob>, ApiError>> {
        Box::pin(async move {
            let mut lock = self.queue.write().await;
            Ok(lock.pop_front())
        })
    }
}
