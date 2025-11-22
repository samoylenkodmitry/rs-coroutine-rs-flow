use std::future::Future;
use std::pin::Pin;

/// FlowCollector receives emitted values from a Flow
pub struct FlowCollector<T> {
    emit_fn: Box<dyn FnMut(T) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>,
}

impl<T> FlowCollector<T> {
    pub fn new<F, Fut>(mut f: F) -> Self
    where
        F: FnMut(T) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self {
            emit_fn: Box::new(move |value| Box::pin(f(value))),
        }
    }

    pub async fn emit(&mut self, value: T) {
        (self.emit_fn)(value).await
    }
}
