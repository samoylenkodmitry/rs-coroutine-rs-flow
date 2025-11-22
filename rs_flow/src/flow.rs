use std::future::Future;
use std::sync::Arc;
use futures::future::BoxFuture;
use crate::FlowCollector;

/// Flow represents a cold asynchronous stream
pub struct Flow<T> {
    collect_fn: Arc<dyn Fn(FlowCollector<T>) -> BoxFuture<'static, ()> + Send>,
}

impl<T: Send + 'static> Flow<T> {
    /// Create a flow from a collect function
    pub fn from_fn<F>(f: F) -> Self
    where
        F: Fn(FlowCollector<T>) -> BoxFuture<'static, ()> + Send + 'static,
    {
        Self {
            collect_fn: Arc::new(f),
        }
    }

    /// Collect values from this flow
    pub async fn collect<F, Fut>(&self, mut on_value: F)
    where
        F: FnMut(T) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let collector = FlowCollector::new(on_value);
        (self.collect_fn)(collector).await
    }

    /// Create a flow that emits a single value
    pub fn just(value: T) -> Self
    where
        T: Clone,
    {
        Self::from_fn(move |mut collector| {
            let value = value.clone();
            Box::pin(async move {
                collector.emit(value).await;
            })
        })
    }

    /// Create an empty flow
    pub fn empty() -> Self {
        Self::from_fn(|_collector| Box::pin(async {}))
    }
}

impl<T: Send + 'static> Clone for Flow<T> {
    fn clone(&self) -> Self {
        Self {
            collect_fn: self.collect_fn.clone(),
        }
    }
}
