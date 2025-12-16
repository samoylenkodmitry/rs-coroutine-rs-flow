use crate::error::{CancellationError, TaskError};
use crate::executor::{BoxedJoinHandle, Dispatcher};
use crate::job::{CancelToken, JobHandle};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::oneshot;

tokio::task_local! {
    pub static CURRENT_SCOPE: Arc<CoroutineScope>;
}

/// Guard that ensures job.complete() is called even on panic
/// This is critical for correctness - without it, panics in spawned tasks
/// would leave the job in an incomplete state forever.
struct JobCompletionGuard {
    job: JobHandle,
}

impl JobCompletionGuard {
    fn new(job: JobHandle) -> Self {
        Self { job }
    }
}

impl Drop for JobCompletionGuard {
    fn drop(&mut self) {
        // Always mark job as complete, even on panic unwind
        self.job.complete();
    }
}

/// Future returned by with_dispatcher that cancels the job on drop
///
/// CRITICAL: This prevents task detachment when the future is dropped
/// (e.g., in a timeout or select!). The cancel token is cancelled immediately,
/// signaling the spawned task to stop.
struct WithDispatcherFuture<T> {
    join_handle: Option<BoxedJoinHandle>,
    rx: oneshot::Receiver<Result<T, TaskError>>,
    parent_token: CancelToken,
    child_cancel_token: CancelToken,
}

impl<T> Drop for WithDispatcherFuture<T> {
    fn drop(&mut self) {
        // CRITICAL: Only cancel if the future hasn't completed yet.
        // After poll returns Ready, join_handle is None, so we don't spuriously
        // cancel a successfully completed job.
        if self.join_handle.is_some() {
            // Cancel the token that the task is actually waiting on
            self.child_cancel_token.cancel();
        }
        // JoinHandle drop is fine - the task will see cancellation via token
    }
}

impl<T> Future for WithDispatcherFuture<T>
where
    T: Send + 'static,
{
    type Output = Result<T, TaskError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // Check parent cancellation first
        if self.parent_token.is_cancelled() {
            // Parent cancelled - still need to await join_handle to observe panics
            // but we'll return Cancelled
            if let Some(mut handle) = self.join_handle.take() {
                // Poll the join handle to completion
                let handle_pin = std::pin::Pin::new(&mut handle);
                match handle_pin.poll(cx) {
                    std::task::Poll::Ready(Ok(())) => {
                        // Task completed - return Cancelled since parent was cancelled
                        return std::task::Poll::Ready(Err(TaskError::Cancelled));
                    }
                    std::task::Poll::Ready(Err(join_err)) if join_err.is_panic() => {
                        // Task panicked - propagate panic even though parent cancelled
                        let panic_msg = crate::error::extract_panic_message(join_err);
                        return std::task::Poll::Ready(Err(TaskError::Panicked(panic_msg)));
                    }
                    std::task::Poll::Ready(Err(_)) => {
                        // Task aborted
                        return std::task::Poll::Ready(Err(TaskError::Aborted));
                    }
                    std::task::Poll::Pending => {
                        // Put handle back and return Pending
                        self.join_handle = Some(handle);
                        return std::task::Poll::Pending;
                    }
                }
            }
            return std::task::Poll::Ready(Err(TaskError::Cancelled));
        }

        // Not cancelled yet - poll join handle
        if let Some(mut handle) = self.join_handle.take() {
            let handle_pin = std::pin::Pin::new(&mut handle);
            match handle_pin.poll(cx) {
                std::task::Poll::Ready(Ok(())) => {
                    // Task completed - get result from oneshot
                    let rx_pin = std::pin::Pin::new(&mut self.rx);
                    match rx_pin.poll(cx) {
                        std::task::Poll::Ready(Ok(result)) => {
                            return std::task::Poll::Ready(result);
                        }
                        std::task::Poll::Ready(Err(_)) => {
                            // Sender dropped without sending
                            return std::task::Poll::Ready(Err(TaskError::Aborted));
                        }
                        std::task::Poll::Pending => {
                            // This shouldn't happen - task completed but didn't send?
                            return std::task::Poll::Ready(Err(TaskError::Aborted));
                        }
                    }
                }
                std::task::Poll::Ready(Err(join_err)) if join_err.is_panic() => {
                    // Task panicked
                    let panic_msg = crate::error::extract_panic_message(join_err);
                    return std::task::Poll::Ready(Err(TaskError::Panicked(panic_msg)));
                }
                std::task::Poll::Ready(Err(_)) => {
                    // Task aborted
                    return std::task::Poll::Ready(Err(TaskError::Aborted));
                }
                std::task::Poll::Pending => {
                    // Put handle back and return Pending
                    self.join_handle = Some(handle);
                    return std::task::Poll::Pending;
                }
            }
        }

        // Handle already taken and completed
        std::task::Poll::Ready(Err(TaskError::Aborted))
    }
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
        let job = JobHandle::new();  // New job for launched coroutine
        let cancel_token = self.cancel_token.clone();

        let job_clone = job.clone();
        let job_for_observer = job.clone();

        let join_handle = dispatcher.spawn(async move {
            // Create guard FIRST - ensures job.complete() is called even on panic
            let _guard = JobCompletionGuard::new(job_clone);

            CURRENT_SCOPE
                .scope(scope.clone(), async move {
                    // CRITICAL: Check cancellation BEFORE starting work
                    // This prevents "already cancelled but ran anyway" races
                    if cancel_token.is_cancelled() {
                        return;
                    }

                    // CRITICAL: Race the future against cancellation
                    // biased + cancellation-first = deterministic cancellation semantics
                    // This ensures scope.cancel() actually stops the future
                    tokio::select! {
                        biased;
                        _ = cancel_token.cancelled() => {
                            // Scope was cancelled - stop execution immediately
                        }
                        _ = fut => {
                            // Future completed normally
                        }
                    }
                })
                .await;
            // Guard's Drop will call job.complete() here
        });

        // Spawn observer task to detect panics and store in job outcome
        // CRITICAL: This prevents panics from looking like success
        tokio::spawn(async move {
            match join_handle.await {
                Ok(()) => {
                    // Task completed normally (guard already called complete())
                }
                Err(join_err) if join_err.is_panic() => {
                    // Task panicked - store panic in job outcome
                    let panic_msg = crate::error::extract_panic_message(join_err);
                    job_for_observer.complete_with(Err(TaskError::Panicked(panic_msg)));
                }
                Err(_) => {
                    // Task aborted (e.g., runtime shutdown)
                    job_for_observer.complete_with(Err(TaskError::Aborted));
                }
            }
        });

        job
    }

    /// Switch to a different dispatcher for the given future
    ///
    /// This is equivalent to Kotlin's `withContext(dispatcher) { ... }`.
    ///
    /// # Errors
    ///
    /// Returns `TaskError::Cancelled` if the scope is cancelled.
    /// Returns `TaskError::Panicked` if the future panics.
    /// Returns `TaskError::Aborted` if the task is dropped before completion.
    /// Run a future on a different dispatcher, creating a child scope
    ///
    /// CRITICAL: This returns impl Future with a cancel-on-drop guard.
    /// If the returned future is dropped (e.g., in timeout/select!), the spawned
    /// task will be cancelled immediately to prevent detachment.
    pub fn with_dispatcher<F, T>(
        &self,
        dispatcher: Dispatcher,
        fut: F,
    ) -> impl Future<Output = Result<T, TaskError>> + Send + 'static
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let child_scope = Arc::new(CoroutineScope {
            dispatcher: dispatcher.clone(),
            job: JobHandle::new(),  // New job for child scope
            cancel_token: self.cancel_token.child(),  // Child of parent's cancellation token
        });
        let cancel_token = child_scope.cancel_token.clone();
        let job = child_scope.job.clone();
        let child_cancel_token_for_guard = child_scope.cancel_token.clone();
        let parent_token = self.cancel_token.clone();

        let join_handle = dispatcher.spawn(async move {
            // Create guard FIRST - ensures job.complete() is called even on panic
            let _guard = JobCompletionGuard::new(job);

            // CRITICAL: Check cancellation BEFORE starting work
            if cancel_token.is_cancelled() {
                let _ = tx.send(Err(TaskError::Cancelled));
                return;
            }

            // Run the future in scope, racing against cancellation
            let result = CURRENT_SCOPE.scope(child_scope, async move {
                // biased + cancellation-first = deterministic cancellation semantics
                tokio::select! {
                    biased;
                    _ = cancel_token.cancelled() => Err(TaskError::Cancelled),
                    res = fut => Ok(res),
                }
            }).await;

            let _ = tx.send(result);
            // Guard's Drop will call job.complete() here
        });

        // CRITICAL: Return a future that cancels the token on drop
        // This prevents detachment when the outer future is dropped (e.g., in timeout)
        WithDispatcherFuture {
            join_handle: Some(join_handle),
            rx,
            parent_token,
            child_cancel_token: child_cancel_token_for_guard,
        }
    }

    /// Async task that returns a Deferred
    ///
    /// This is equivalent to Kotlin's `async(dispatcher) { ... }`.
    /// The returned Deferred will complete with `Err(TaskError)` if cancelled or panicked.
    pub fn async_task<F, T>(&self, dispatcher: Dispatcher, fut: F) -> Deferred<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let child_scope = Arc::new(CoroutineScope {
            dispatcher: dispatcher.clone(),
            job: JobHandle::new(),  // New job for child scope
            cancel_token: self.cancel_token.child(),  // Child of parent's cancellation token
        });
        let cancel_token = child_scope.cancel_token.clone();
        let job = child_scope.job.clone();
        let job_for_spawn = job.clone();

        let join_handle = dispatcher.spawn(async move {
            // Create guard FIRST - ensures job.complete() is called even on panic
            let _guard = JobCompletionGuard::new(job_for_spawn);

            // CRITICAL: Check cancellation BEFORE starting work
            if cancel_token.is_cancelled() {
                let _ = tx.send(Err(TaskError::Cancelled));
                return;
            }

            // Run the future in scope, racing against cancellation
            let result = CURRENT_SCOPE.scope(child_scope, async move {
                // biased + cancellation-first = deterministic cancellation semantics
                tokio::select! {
                    biased;
                    _ = cancel_token.cancelled() => Err(TaskError::Cancelled),
                    res = fut => Ok(res),
                }
            }).await;

            let _ = tx.send(result);
            // Guard's Drop will call job.complete() here
        });

        Deferred {
            rx,
            join_handle,
            job,
            parent_cancel_token: self.cancel_token.clone(),
        }
    }

    /// Cancel this scope and all child scopes
    ///
    /// Cancellation is propagated via the CancelToken hierarchy.
    /// All tasks waiting on this scope's token (or child tokens) will observe cancellation.
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Check if this scope is cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }
}

/// A deferred value that can be awaited
pub struct Deferred<T> {
    rx: oneshot::Receiver<Result<T, TaskError>>,
    join_handle: BoxedJoinHandle,
    job: JobHandle,
    parent_cancel_token: CancelToken,
}

impl<T> Deferred<T> {
    /// Await the deferred value
    ///
    /// Returns `Err(TaskError)` if the task is cancelled, panics, or is aborted.
    ///
    /// # Errors
    ///
    /// - `TaskError::Cancelled` if the task or parent scope is cancelled
    /// - `TaskError::Panicked` if the task panics
    /// - `TaskError::Aborted` if the task is dropped before completion
    pub async fn await_result(self) -> Result<T, TaskError> {
        // Race between parent cancellation and task completion (including panics)
        // CRITICAL: Must await join_handle exactly ONCE (no double-polling, no detach)

        // Extract and pin the join_handle so we can poll it in select! without moving
        let join_handle = self.join_handle;
        tokio::pin!(join_handle);

        let join_result = tokio::select! {
            biased;
            _ = self.parent_cancel_token.cancelled() => {
                // Parent cancelled - still await to avoid orphaning/detaching task
                (&mut join_handle).await
            }
            res = &mut join_handle => res,
        };

        match join_result {
            Ok(()) => {
                // Task completed - check if it was due to cancellation
                if self.parent_cancel_token.is_cancelled() {
                    // Parent was cancelled at some point - return Cancelled
                    Err(TaskError::Cancelled)
                } else {
                    // Task completed normally, get result from oneshot
                    // RecvError only occurs if sender dropped without sending (runtime abort/shutdown)
                    self.rx.await.unwrap_or(Err(TaskError::Aborted))
                }
            }
            Err(join_err) if join_err.is_panic() => {
                // Task panicked - propagate panic regardless of parent cancellation
                let panic_msg = crate::error::extract_panic_message(join_err);
                Err(TaskError::Panicked(panic_msg))
            }
            Err(_) => {
                // Task cancelled by runtime shutdown
                Err(TaskError::Aborted)
            }
        }
    }

    /// Await the deferred value without being interrupted by parent cancellation
    ///
    /// This method does NOT race against parent cancellation. It will wait for the
    /// child task to complete (or be cancelled by its own scope) and return the result.
    ///
    /// Use this when you need to retrieve a result that may already be computed,
    /// even if the parent scope has been cancelled.
    ///
    /// # Errors
    ///
    /// - `TaskError::Cancelled` if the CHILD task itself was cancelled
    /// - `TaskError::Panicked` if the task panics
    /// - `TaskError::Aborted` if the task is dropped before completion
    ///
    /// Note: This will NOT return `TaskError::Cancelled` due to parent cancellation.
    pub async fn await_uninterruptible(self) -> Result<T, TaskError> {
        // Do NOT race against parent cancellation - just wait for task completion
        match self.join_handle.await {
            Ok(()) => {
                // Task completed normally, get result from oneshot
                // RecvError only occurs if sender dropped without sending (runtime abort/shutdown)
                self.rx.await.unwrap_or(Err(TaskError::Aborted))
            }
            Err(join_err) if join_err.is_panic() => {
                // Task panicked - use JoinError to get panic info (proper Tokio idiom)
                let panic_msg = crate::error::extract_panic_message(join_err);
                Err(TaskError::Panicked(panic_msg))
            }
            Err(_) => {
                // Task cancelled by runtime shutdown
                Err(TaskError::Aborted)
            }
        }
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

/// Check if the current scope is cancelled
///
/// # Errors
///
/// Returns `Err(CancellationError)` if the current scope is cancelled.
///
/// # Panics
///
/// Panics if called outside a CoroutineScope context.
/// Use `check_cancellation_lenient()` if you need to handle this case.
///
/// This ensures cancelled scopes always return early, preventing "zombie tasks".
pub fn check_cancellation() -> Result<(), CancellationError> {
    match CURRENT_SCOPE.try_with(|scope| scope.is_cancelled()) {
        Ok(true) => Err(CancellationError),
        Ok(false) => Ok(()),
        Err(_) => panic!("check_cancellation() called outside CoroutineScope - use check_cancellation_lenient() if intentional"),
    }
}

/// Lenient cancellation check - never errors when not in scope
///
/// Returns `Ok(())` if not in a scope context (treats as not cancelled).
///
/// Use this when cancellation checking is optional or when code
/// might legitimately run outside a scope.
pub fn check_cancellation_lenient() -> Result<(), CancellationError> {
    match CURRENT_SCOPE.try_with(|scope| scope.is_cancelled()) {
        Ok(true) => Err(CancellationError),
        Ok(false) | Err(_) => Ok(()),
    }
}

/// Check if currently executing within a CoroutineScope
///
/// # Errors
///
/// Returns `Err(NotInScopeError)` if called outside a CoroutineScope context.
///
/// Use this to verify scope context before performing scope-dependent operations.
pub fn require_scope() -> Result<(), crate::error::NotInScopeError> {
    match CURRENT_SCOPE.try_with(|_| ()) {
        Ok(()) => Ok(()),
        Err(_) => Err(crate::error::NotInScopeError),
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
/// # Usage
///
/// For functions returning `()`:
/// ```ignore
/// check_cancelled!();
/// // continues if not cancelled, returns () if cancelled
/// ```
///
/// For functions returning `Result<T, E>` where `E: From<CancellationError>`:
/// ```ignore
/// check_cancelled!(Result);
/// // continues if not cancelled, returns Err(CancellationError.into()) if cancelled
/// ```
#[macro_export]
macro_rules! check_cancelled {
    // For -> () functions
    () => {
        if let Err(_) = $crate::check_cancellation() {
            return;
        }
    };
    // For -> Result<T, E> functions where E: From<CancellationError>
    (Result) => {
        if let Err(e) = $crate::check_cancellation() {
            return Err(e.into());
        }
    };
}

/// Macro to yield execution and check for cancellation
/// This combines yield_now() with cancellation checking
///
/// # Usage
///
/// For functions returning `()`:
/// ```ignore
/// yield_and_check!();
/// // yields control and returns () if cancelled
/// ```
///
/// For functions returning `Result<T, E>`:
/// ```ignore
/// yield_and_check!(Result);
/// // yields control and returns Err(CancellationError.into()) if cancelled
/// ```
#[macro_export]
macro_rules! yield_and_check {
    () => {
        $crate::yield_now().await;
        $crate::check_cancelled!();
    };
    (Result) => {
        $crate::yield_now().await;
        $crate::check_cancelled!(Result);
    };
}
