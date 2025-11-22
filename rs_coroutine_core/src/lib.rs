use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub mod executor;
pub mod dispatcher;
pub mod scope;
pub mod job;

pub use executor::Executor;
pub use dispatcher::{Dispatcher, Dispatchers};
pub use scope::CoroutineScope;
pub use job::JobHandle;

tokio::task_local! {
    pub static CURRENT_SCOPE: Arc<CoroutineScope>;
}

/// Helper to execute a closure with the current scope
pub async fn with_current_scope<F, Fut, T>(f: F) -> T
where
    F: FnOnce(&CoroutineScope) -> Fut,
    Fut: Future<Output = T>,
{
    CURRENT_SCOPE.with(|scope| f(scope)).await
}

/// Deferred represents a value that will be available in the future
pub struct Deferred<T> {
    rx: oneshot::Receiver<T>,
    job: JobHandle,
}

impl<T> Deferred<T> {
    pub fn new(rx: oneshot::Receiver<T>, job: JobHandle) -> Self {
        Self { rx, job }
    }

    pub async fn await_value(self) -> T {
        self.rx.await.expect("task dropped")
    }

    pub fn job(&self) -> &JobHandle {
        &self.job
    }
}
