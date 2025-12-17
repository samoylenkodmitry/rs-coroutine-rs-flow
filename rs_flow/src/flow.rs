use futures::stream::Stream;
use std::future::Future;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

type FlowFuture = Pin<Box<dyn Future<Output = ControlFlow<()>> + Send>>;

/// A collector that receives emitted values
pub struct FlowCollector<T> {
    emit_fn: Arc<dyn Fn(T) -> FlowFuture + Send + Sync>,
}

impl<T> FlowCollector<T> {
    /// Create a new FlowCollector
    pub fn new<F, Fut>(emit_fn: F) -> Self
    where
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ControlFlow<()>> + Send + 'static,
    {
        Self {
            emit_fn: Arc::new(move |value| Box::pin(emit_fn(value))),
        }
    }

    /// Emit a value to the collector
    pub async fn emit(&self, value: T) -> ControlFlow<()> {
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
    collect_fn: Arc<dyn Fn(FlowCollector<T>) -> FlowFuture + Send + Sync>,
}

impl<T> Flow<T>
where
    T: Send + 'static,
{
    /// Create a new Flow from a collect function
    pub fn new<F, Fut>(collect_fn: F) -> Self
    where
        F: Fn(FlowCollector<T>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ControlFlow<()>> + Send + 'static,
    {
        Self {
            collect_fn: Arc::new(move |collector| Box::pin(collect_fn(collector))),
        }
    }

    /// Create a flow from a function (alias for new)
    pub fn from_fn<F, Fut>(collect_fn: F) -> Self
    where
        F: Fn(FlowCollector<T>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ControlFlow<()>> + Send + 'static,
    {
        Self::new(collect_fn)
    }

    /// Collect values from this flow
    pub async fn collect<F, Fut>(&self, on_value: F) -> ControlFlow<()>
    where
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ControlFlow<()>> + Send + 'static,
    {
        let collector = FlowCollector::new(on_value);
        (self.collect_fn)(collector).await
    }

    /// Convert this Flow to a Stream
    ///
    /// This allows using Flow with Stream combinators and select! macros.
    /// The conversion spawns a background task to collect from the flow,
    /// using a bounded channel (buffer_size) to manage backpressure.
    ///
    /// The background task is automatically aborted when the returned Stream is dropped.
    ///
    /// # Example
    /// ```ignore
    /// use futures::StreamExt;
    ///
    /// let flow = flow_of!(1, 2, 3);
    /// let mut stream = flow.to_stream(10);
    ///
    /// while let Some(value) = stream.next().await {
    ///     println!("{}", value);
    /// }
    /// ```
    pub fn to_stream(self, buffer_size: usize) -> FlowStream<T> {
        FlowStream::new(self, buffer_size)
    }
}

/// A Stream adapter for Flow
///
/// This converts a callback-based Flow<T> into a poll-based Stream<Item = T>
/// by using a channel internally. The flow collection happens in a background task.
///
/// When the stream is dropped:
/// - If in a CoroutineScope: task will be cancelled cooperatively via scope cancellation
/// - If not in a scope: task will be aborted (fallback for backward compatibility)
pub struct FlowStream<T> {
    rx: mpsc::Receiver<T>,
    _task: FlowStreamGuard,
}

/// Guard for FlowStream background task
///
/// CRITICAL: This MUST cancel the collection task when stream is dropped.
/// Essential for flat_map_latest which switches flows - without cancel, old flows keep running.
struct FlowStreamGuard(Option<crate::internal_utils::ScopeAwareHandle>);

impl Drop for FlowStreamGuard {
    fn drop(&mut self) {
        // CRITICAL: Must cancel the collection task when stream is dropped
        // Without this, flat_map_latest leaks 999 tasks for 1000 items
        if let Some(handle) = self.0.take() {
            handle.cancel();
        }
    }
}

impl<T> FlowStream<T>
where
    T: Send + 'static,
{
    fn new(flow: Flow<T>, buffer_size: usize) -> Self {
        use crate::internal_utils::spawn_in_scope;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let (tx, rx) = mpsc::channel(buffer_size);
        let stopped = Arc::new(AtomicBool::new(false));

        // Spawn collection task in current scope if available
        let stopped_clone = Arc::clone(&stopped);
        let task = spawn_in_scope(async move {
            let _ = flow.collect(move |value| {
                let tx = tx.clone();
                let stopped = Arc::clone(&stopped_clone);
                async move {
                    use std::ops::ControlFlow::{Break, Continue};

                    // CRITICAL: Stop immediately if receiver dropped
                    // Without this check, we busy-loop burning CPU after stream is dropped
                    if stopped.load(Ordering::Relaxed) {
                        return Break(());
                    }

                    // Try to send - if it fails, receiver is dropped, stop collecting
                    if tx.send(value).await.is_err() {
                        stopped.store(true, Ordering::Relaxed);
                        return Break(());
                    }

                    Continue(())
                }
            })
            .await;
        });

        Self {
            rx,
            _task: FlowStreamGuard(Some(task)),
        }
    }
}

impl<T> Stream for FlowStream<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
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
    Fut: Future<Output = ControlFlow<()>> + Send + 'static,
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
