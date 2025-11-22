use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A collector that receives emitted values
pub struct FlowCollector<T> {
    emit_fn: Arc<dyn Fn(T) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>,
}

impl<T> FlowCollector<T> {
    /// Create a new FlowCollector
    pub fn new<F, Fut>(emit_fn: F) -> Self
    where
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self {
            emit_fn: Arc::new(move |value| Box::pin(emit_fn(value))),
        }
    }

    /// Emit a value to the collector
    pub async fn emit(&self, value: T) {
        (self.emit_fn)(value).await
    }
}

impl<T> Clone for FlowCollector<T> {
    fn clone(&self) -> Self {
        Self {
            emit_fn: Arc::clone(&self.emit_fn),
        }
    }
}

/// A cold flow that emits values when collected
pub struct Flow<T> {
    collect_fn: Arc<
        dyn Fn(FlowCollector<T>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
    >,
}

impl<T> Flow<T>
where
    T: Send + 'static,
{
    /// Create a new Flow from a collect function
    pub fn new<F, Fut>(collect_fn: F) -> Self
    where
        F: Fn(FlowCollector<T>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self {
            collect_fn: Arc::new(move |collector| Box::pin(collect_fn(collector))),
        }
    }

    /// Create a flow from a function (alias for new)
    pub fn from_fn<F, Fut>(collect_fn: F) -> Self
    where
        F: Fn(FlowCollector<T>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self::new(collect_fn)
    }

    /// Collect values from this flow
    pub async fn collect<F, Fut>(&self, on_value: F)
    where
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let collector = FlowCollector::new(on_value);
        (self.collect_fn)(collector).await
    }
}

impl<T> Clone for Flow<T> {
    fn clone(&self) -> Self {
        Self {
            collect_fn: Arc::clone(&self.collect_fn),
        }
    }
}

/// Builder function for creating flows
pub fn flow<T, F, Fut>(builder: F) -> Flow<T>
where
    T: Send + 'static,
    F: Fn(FlowCollector<T>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Flow::new(builder)
}

/// Macro to create a flow with a builder block
#[macro_export]
macro_rules! flow_block {
    (|$collector:ident| $($body:tt)*) => {{
        $crate::flow::flow(move |$collector| async move {
            $($body)*
        })
    }};
}
