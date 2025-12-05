use crate::flow::Flow;
use rs_coroutine_core::Suspending;

/// Extension trait for Suspending to convert to Flow
pub trait SuspendingExt<T> {
    /// Convert a Suspending into a Flow
    fn as_flow(&self) -> Flow<T>;
}

impl<T> SuspendingExt<T> for Suspending<T>
where
    T: Send + 'static,
{
    fn as_flow(&self) -> Flow<T> {
        let suspending = self.clone();
        Flow::from_fn(move |collector| {
            let suspending = suspending.clone();
            async move {
                let value = suspending.call().await;
                collector.emit(value).await;
            }
        })
    }
}
