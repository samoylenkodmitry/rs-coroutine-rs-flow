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

    /// Emit a value to the collector (with control flow)
    ///
    /// Returns `ControlFlow::Break` if the downstream consumer wants to stop
    /// receiving values (e.g., after `take(n)` reaches its limit).
    ///
    /// For simple emission without checking termination signals, use `emit_value()`.
    ///
    /// # Example
    /// ```ignore
    /// // Check for early termination
    /// for i in 1..=100 {
    ///     match collector.emit(i).await {
    ///         Continue(()) => {}, // Keep going
    ///         Break(()) => break, // Stop early
    ///     }
    /// }
    /// ```
    pub async fn emit(&self, value: T) -> ControlFlow<()> {
        (self.emit_fn)(value).await
    }

    /// Emit a value to the collector (ergonomic API)
    ///
    /// This is the ergonomic alternative to `emit()` that doesn't return
    /// `ControlFlow`. Use this when you don't need to check for early termination.
    ///
    /// # ⚠️ WARNING: Ignores Downstream Termination
    ///
    /// This method **silently ignores** early termination signals from downstream
    /// operators like `take()`, `first()`, etc. This means your producer will
    /// continue emitting ALL values even if the consumer stopped listening.
    ///
    /// **When to use `emit_value()`:**
    /// - Small, finite data sources (e.g., emitting 5-10 values)
    /// - When you know downstream will consume everything
    /// - Simple test cases
    ///
    /// **When NOT to use `emit_value()` (use `emit()` instead):**
    /// - Large or infinite data sources (e.g., emitting 1000+ values)
    /// - With downstream operators like `.take(5)` or `.first()`
    /// - When performance matters (avoiding wasted work)
    ///
    /// # Example - The Problem
    /// ```ignore
    /// // ❌ BAD: Emits all 1000 values even though downstream only wants 5!
    /// let flow = flow_fn(|collector| async move {
    ///     for i in 1..=1000 {
    ///         collector.emit_value(i).await;  // Ignores termination!
    ///     }
    /// });
    /// flow.take(5).for_each(|x| async move { println!("{}", x) }).await;
    ///
    /// // ✅ GOOD: Stops emitting after 5 values
    /// let flow = flow(|collector| async move {
    ///     for i in 1..=1000 {
    ///         match collector.emit(i).await {
    ///             Continue(()) => {},
    ///             Break(()) => break,  // Downstream signaled termination!
    ///         }
    ///     }
    ///     Continue(())
    /// });
    /// flow.take(5).for_each(|x| async move { println!("{}", x) }).await;
    /// ```
    pub async fn emit_value(&self, value: T) {
        let _ = self.emit(value).await;
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

    /// Collect values from this flow with control flow support
    ///
    /// This is the low-level API that allows operators to signal early termination
    /// via `ControlFlow::Break`. Most users should use `for_each()` instead.
    ///
    /// Use this when you need to:
    /// - Stop upstream emission early (e.g., implementing `take`)
    /// - Propagate termination signals between operators
    ///
    /// For simple collection without early termination, use `for_each()`.
    pub async fn collect<F, Fut>(&self, on_value: F) -> ControlFlow<()>
    where
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ControlFlow<()>> + Send + 'static,
    {
        let collector = FlowCollector::new(on_value);
        (self.collect_fn)(collector).await
    }

    /// Perform an action for each value in the flow (ergonomic API)
    ///
    /// This is the ergonomic alternative to `collect()` for users who don't need
    /// early termination control. The closure returns `()` instead of `ControlFlow<()>`.
    ///
    /// # Example
    /// ```ignore
    /// flow.for_each(|value| async move {
    ///     println!("Got: {}", value);
    /// }).await;
    /// ```
    ///
    /// If you need to stop collection early based on values, use `take()`, `take_while()`,
    /// or other operators before calling `for_each()`.
    pub async fn for_each<F, Fut>(&self, f: F)
    where
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let f = Arc::new(f);
        let _ = self
            .collect(move |value| {
                let f = Arc::clone(&f);
                async move {
                    f(value).await;
                    ControlFlow::Continue(())
                }
            })
            .await;
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
    task: FlowStreamGuard,
}

/// Guard for FlowStream background task
///
/// # Drop Behavior & Limitations
///
/// **CRITICAL**: This cancels the background collection task when the stream is dropped.
/// Essential for flat_map_latest which switches flows - without cancel, old flows keep running.
///
/// **IMPORTANT LIMITATION**: Like CancelOnDrop, this has async drop limitations:
/// - Drop calls `cancel()` to signal task to stop
/// - Drop returns **before task actually stops**
/// - Task may still be running after drop
///
/// This is a fundamental Rust limitation (Drop cannot be async).
///
/// ## For Deterministic Cleanup
///
/// If you need to ensure the background task is fully stopped before continuing,
/// use `FlowStream::cancel_and_join()`:
///
/// ```ignore
/// // ❌ Drop doesn't wait - old and new tasks may overlap!
/// let mut stream = flow.to_stream(10);
/// drop(stream);
/// let new_stream = flow.to_stream(10);  // May run concurrently with old!
///
/// // ✅ Explicit cleanup - guaranteed no overlap
/// let stream = flow.to_stream(10);
/// stream.cancel_and_join().await;  // Wait for full stop
/// let new_stream = flow.to_stream(10);  // Safe, old is stopped
/// ```
///
/// **Note**: flat_map_latest uses cancel_and_join() internally to prevent
/// concurrent stream execution.
struct FlowStreamGuard(Option<crate::internal_utils::ScopeAwareHandle>);

impl FlowStreamGuard {
    /// Cancel and wait for the background task to complete
    ///
    /// CRITICAL: This is required for flat_map_latest to avoid concurrent stream execution.
    /// Without await, dropping old stream and starting new stream creates a race.
    async fn cancel_and_join(mut self) {
        if let Some(handle) = self.0.take() {
            handle.cancel_and_join().await;
        }
    }
}

impl Drop for FlowStreamGuard {
    fn drop(&mut self) {
        // CRITICAL: Must cancel the collection task when stream is dropped
        // Without this, flat_map_latest leaks 999 tasks for 1000 items
        //
        // NOTE: We can only signal cancellation here, not await completion (async drop doesn't exist).
        // For proper awaited cancellation, use FlowStream::cancel_and_join() explicitly.
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
            let _ = flow
                .collect(move |value| {
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
            task: FlowStreamGuard(Some(task)),
        }
    }

    /// Cancel the background collection task and wait for it to complete
    ///
    /// CRITICAL: Required for flat_map_latest to properly switch streams without race conditions.
    /// Regular drop only signals cancellation but doesn't wait - this ensures complete cleanup.
    ///
    /// After calling this, the stream is effectively dead (task is gone).
    pub async fn cancel_and_join(mut self) {
        // Take ownership of the guard and await its completion
        let guard = std::mem::replace(&mut self.task, FlowStreamGuard(None));
        guard.cancel_and_join().await;
        // self (including rx) drops here
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

/// Builder function for creating flows (with control flow support)
///
/// This is the low-level builder that requires returning `ControlFlow<()>`.
/// For simpler flow creation, use `flow_fn()` which doesn't require ControlFlow.
///
/// Use this when you need to handle early termination signals from downstream.
pub fn flow<T, F, Fut>(builder: F) -> Flow<T>
where
    T: Send + 'static,
    F: Fn(FlowCollector<T>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ControlFlow<()>> + Send + 'static,
{
    Flow::new(builder)
}

/// Builder function for creating flows (ergonomic API)
///
/// This is the ergonomic alternative to `flow()` that doesn't require
/// returning `ControlFlow<()>`. Perfect for simple flow creation.
///
/// # Example
/// ```ignore
/// let numbers = flow_fn(|collector| async move {
///     for i in 1..=5 {
///         collector.emit_value(i).await;
///     }
/// });
/// ```
///
/// If you need to handle early termination (e.g., responding to downstream
/// `take()`), use `flow()` instead and check the ControlFlow result from `emit()`.
pub fn flow_fn<T, F, Fut>(builder: F) -> Flow<T>
where
    T: Send + 'static,
    F: Fn(FlowCollector<T>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let builder = Arc::new(builder);
    Flow::new(move |collector| {
        let builder = Arc::clone(&builder);
        async move {
            builder(collector).await;
            ControlFlow::Continue(())
        }
    })
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
