use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub struct Suspending<T> {
    action: Arc<dyn Fn() -> Pin<Box<dyn Future<Output = T> + Send>> + Send + Sync>,
}

impl<T> Suspending<T> {
    pub fn from_async_block<F, Fut>(block: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        Self {
            action: Arc::new(move || Box::pin(block())),
        }
    }

    pub async fn call(&self) -> T {
        (self.action)().await
    }
}

impl<T> Clone for Suspending<T> {
    fn clone(&self) -> Self {
        Self {
            action: self.action.clone(),
        }
    }
}
