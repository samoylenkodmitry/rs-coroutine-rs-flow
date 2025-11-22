use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use futures::future::BoxFuture;

pub mod flow;
pub mod collector;

pub use flow::Flow;
pub use collector::FlowCollector;

/// Suspending represents a cold suspendable computation
pub struct Suspending<T> {
    invoke: Arc<dyn Fn() -> BoxFuture<'static, T> + Send + Sync>,
}

impl<T> Clone for Suspending<T> {
    fn clone(&self) -> Self {
        Self {
            invoke: self.invoke.clone(),
        }
    }
}

impl<T: Send + 'static> Suspending<T> {
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        Self {
            invoke: Arc::new(move || Box::pin(f())),
        }
    }

    pub async fn call(&self) -> T {
        (self.invoke)().await
    }

    /// Convert this suspending computation into a Flow that emits one value
    pub fn as_flow(self) -> Flow<T> {
        Flow::from_fn(move |mut collector| {
            let this = self.clone();
            Box::pin(async move {
                let value = this.call().await;
                collector.emit(value).await;
            })
        })
    }
}

/// Macro to create a suspending block
#[macro_export]
macro_rules! suspend_block {
    ($($body:tt)*) => {
        $crate::Suspending::new(|| async move { $($body)* })
    };
}

/// Macro to create a flow
#[macro_export]
macro_rules! flow_block {
    (|mut $collector:ident| $($body:tt)*) => {
        $crate::Flow::from_fn(move |mut $collector| {
            Box::pin(async move { $($body)* })
        })
    };
    (|$collector:ident| $($body:tt)*) => {
        $crate::Flow::from_fn(move |$collector| {
            Box::pin(async move { $($body)* })
        })
    };
}
