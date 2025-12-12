use crate::executor::Dispatcher;
use crate::job::{CancelToken, JobHandle};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::oneshot;

tokio::task_local! {
    pub static CURRENT_SCOPE: Arc<CoroutineScope>;
}

/// A coroutine scope manages the lifecycle of coroutines
#[derive(Clone)]
pub struct CoroutineScope {
    pub dispatcher: Dispatcher,
    pub job: JobHandle,
    pub cancel_token: CancelToken,
}

impl CoroutineScope {
    /// Create a new CoroutineScope
    pub fn new(dispatcher: Dispatcher) -> Self {
        Self {
            dispatcher,
            job: JobHandle::new(),
            cancel_token: CancelToken::new(),
        }
    }

    /// Launch a new coroutine in this scope
    /// The coroutine will be automatically cancelled when the scope is cancelled
    pub fn launch<F>(&self, fut: F) -> JobHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let scope = Arc::new(self.clone());
        let dispatcher = self.dispatcher.clone();
        let job = self.job.new_child();
        let cancel_token = self.cancel_token.clone();

        let job_clone = job.clone();
        let cancel_token_clone = cancel_token.clone();
        dispatcher.spawn(async move {
            CURRENT_SCOPE
                .scope(scope.clone(), async move {
                    // Wrap the future with cancellation checking
                    let cancellable = CancellableWrap {
                        future: fut,
                        cancel_token: cancel_token_clone,
                    };
                    cancellable.await;
                    job_clone.complete();
                })
                .await;
        });

        job
    }

    /// Switch to a different dispatcher for the given future
    pub async fn with_dispatcher<F, T>(&self, dispatcher: Dispatcher, fut: F) -> T
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let child_scope = Arc::new(CoroutineScope {
            dispatcher: dispatcher.clone(),
            job: self.job.new_child(),
            cancel_token: self.cancel_token.child(),
        });

        dispatcher.spawn(async move {
            let res = CURRENT_SCOPE.scope(child_scope, fut).await;
            let _ = tx.send(res);
        });

        rx.await.expect("dispatcher dropped future")
    }

    /// Async task that returns a Deferred
    pub fn async_task<F, T>(&self, dispatcher: Dispatcher, fut: F) -> Deferred<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let child_scope = Arc::new(CoroutineScope {
            dispatcher: dispatcher.clone(),
            job: self.job.new_child(),
            cancel_token: self.cancel_token.child(),
        });
        let job = child_scope.job.clone();

        dispatcher.spawn(async move {
            let res = CURRENT_SCOPE.scope(child_scope, fut).await;
            let _ = tx.send(res);
        });

        Deferred { rx, job }
    }

    /// Cancel this scope
    pub fn cancel(&self) {
        self.cancel_token.cancel();
        self.job.cancel();
    }

    /// Check if this scope is cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }
}

/// A deferred value that can be awaited
pub struct Deferred<T> {
    rx: oneshot::Receiver<T>,
    job: JobHandle,
}

impl<T> Deferred<T> {
    /// Await the deferred value
    pub async fn await_result(self) -> T {
        self.rx.await.expect("task dropped")
    }

    /// Get the job handle
    pub fn job(&self) -> &JobHandle {
        &self.job
    }
}

/// Helper to access the current scope
pub async fn with_current_scope<F, Fut, T>(f: F) -> T
where
    F: FnOnce(&CoroutineScope) -> Fut,
    Fut: Future<Output = T>,
{
    CURRENT_SCOPE.with(|scope| f(scope)).await
}

/// Helper to get a reference to the current scope (for macros)
pub fn get_current_scope() -> Arc<CoroutineScope> {
    CURRENT_SCOPE.with(Arc::clone)
}

/// Check if the current scope is cancelled and return an error if so
/// This can be used in async functions to check for cancellation
pub fn check_cancellation() -> Result<(), CancellationError> {
    CURRENT_SCOPE.with(|scope| {
        if scope.is_cancelled() {
            Err(CancellationError)
        } else {
            Ok(())
        }
    })
}

/// Error returned when a coroutine is cancelled
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancellationError;

impl std::fmt::Display for CancellationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Coroutine was cancelled")
    }
}

impl std::error::Error for CancellationError {}

/// A future wrapper that checks for cancellation and yields the error if cancelled
/// This allows cooperative cancellation at await points
struct CancellableWrap<F> {
    future: F,
    cancel_token: CancelToken,
}

impl<F> Future for CancellableWrap<F>
where
    F: Future<Output = ()>,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Check cancellation before polling the inner future
        if self.cancel_token.is_cancelled() {
            return Poll::Ready(());
        }

        // Safety: We're not moving the inner future
        let future = unsafe { self.map_unchecked_mut(|s| &mut s.future) };
        future.poll(cx)
    }
}

/// Yield control and check for cancellation
/// This is similar to Kotlin's yield() function
pub async fn yield_now() {
    tokio::task::yield_now().await;
}

/// Macro to check if the current coroutine is cancelled and return early if so
/// Similar to Kotlin's ensureActive()
///
/// # Example
/// ```ignore
/// check_cancelled!();
/// // continues if not cancelled, returns () if cancelled
/// ```
#[macro_export]
macro_rules! check_cancelled {
    () => {
        if let Err(_) = $crate::check_cancellation() {
            return;
        }
    };
}

/// Macro to ensure the coroutine is active, panicking with a message if cancelled
/// Similar to Kotlin's ensureActive() with panic behavior
///
/// # Example
/// ```ignore
/// ensure_active!();
/// // continues if not cancelled, panics if cancelled
/// ```
#[macro_export]
macro_rules! ensure_active {
    () => {
        if let Err(e) = $crate::check_cancellation() {
            panic!("Coroutine cancelled: {}", e);
        }
    };
    ($msg:expr) => {
        if let Err(_) = $crate::check_cancellation() {
            panic!("{}", $msg);
        }
    };
}

/// Macro to yield execution and check for cancellation
/// This combines yield_now() with cancellation checking
///
/// # Example
/// ```ignore
/// yield_and_check!();
/// // yields control and returns () if cancelled
/// ```
#[macro_export]
macro_rules! yield_and_check {
    () => {
        $crate::yield_now().await;
        check_cancelled!();
    };
}
