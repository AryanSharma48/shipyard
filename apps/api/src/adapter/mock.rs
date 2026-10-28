use crate::adapter::RuntimeAdapter;
use crate::error::ApiError;
use crate::models::Deployment;
use crate::store::BoxFuture;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct MockRuntimeAdapter {
    pub should_fail: AtomicBool,
}

impl Default for MockRuntimeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl MockRuntimeAdapter {
    pub fn new() -> Self {
        Self {
            should_fail: AtomicBool::new(false),
        }
    }

    pub fn with_failure() -> Self {
        Self {
            should_fail: AtomicBool::new(true),
        }
    }

    pub fn set_should_fail(&self, fail: bool) {
        self.should_fail.store(fail, Ordering::SeqCst);
    }
}

impl RuntimeAdapter for MockRuntimeAdapter {
    fn deploy(&self, deployment: &Deployment) -> BoxFuture<'_, Result<(), ApiError>> {
        let fails = self.should_fail.load(Ordering::SeqCst);
        let dep_id = deployment.id;

        Box::pin(async move {
            if fails {
                Err(ApiError::Internal(format!(
                    "Mock adapter failed to deploy deployment '{dep_id}'"
                )))
            } else {
                Ok(())
            }
        })
    }

    fn check_health(&self, _deployment: &Deployment) -> BoxFuture<'_, Result<bool, ApiError>> {
        let fails = self.should_fail.load(Ordering::SeqCst);
        Box::pin(async move { Ok(!fails) })
    }
}
