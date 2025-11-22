use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A suspending block that can be called multiple times
pub struct Suspending<T> {
    invoke: Arc<dyn Fn() -> Pin<Box<dyn Future<Output = T> + Send>> + Send + Sync>,
}

impl<T> Clone for Suspending<T> {
    fn clone(&self) -> Self {
        Self {
            invoke: Arc::clone(&self.invoke),
        }
    }
}

impl<T> Suspending<T> {
    /// Create a new Suspending from a function
    pub fn new<F>(f: F) -> Self
    where
        F: Fn() -> Pin<Box<dyn Future<Output = T> + Send>> + Send + Sync + 'static,
    {
        Self {
            invoke: Arc::new(f),
        }
    }

    /// Call the suspending block
    pub async fn call(&self) -> T {
        (self.invoke)().await
    }

    /// Invoke the suspending block (alias for call)
    pub async fn invoke(&self) -> T {
        self.call().await
    }
}

/// Macro to create a suspending block
#[macro_export]
macro_rules! suspend_block {
    ($($body:tt)*) => {{
        $crate::suspending::Suspending::new(|| {
            Box::pin(async move {
                $($body)*
            })
        })
    }};
}
