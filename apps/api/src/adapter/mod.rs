pub mod mock;

use crate::error::ApiError;
use crate::models::Deployment;
use crate::store::BoxFuture;

pub use mock::MockRuntimeAdapter;

pub trait RuntimeAdapter: Send + Sync {
    fn deploy(&self, deployment: &Deployment) -> BoxFuture<'_, Result<(), ApiError>>;
    fn check_health(&self, deployment: &Deployment) -> BoxFuture<'_, Result<bool, ApiError>>;
    fn rollback(
        &self,
        _failed_deployment: &Deployment,
        _previous_healthy: Option<&Deployment>,
    ) -> BoxFuture<'_, Result<(), ApiError>> {
        Box::pin(async { Ok(()) })
    }
}
