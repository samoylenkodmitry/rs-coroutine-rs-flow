use crate::flow::Flow;
use rs_coroutine_core::Suspending;

/// Extension trait for Suspending to convert to Flow
pub trait SuspendingExt<T> {
    /// Convert a Suspending into a Flow
    fn as_flow(self) -> Flow<T>;
}

impl<T> SuspendingExt<T> for Suspending<T>
where
    T: Send + 'static,
{
    fn as_flow(self) -> Flow<T> {
        Flow::from_fn(move |collector| {
            let this = self.clone();
            async move {
                let value = this.call().await;
                collector.emit(value).await;
            }
        })
    }
}
