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
        dispatcher.spawn(async move {
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
    pub async fn with_dispatcher<F, T>(
        &self,
        dispatcher: Dispatcher,
        fut: F,
    ) -> Result<T, TaskError>
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
        let cancel_token = child_scope.cancel_token.clone();
        let job = child_scope.job.clone();

        let mut join_handle = dispatcher.spawn(async move {
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

        // Race between parent cancellation and task completion (including panics)
        // biased + cancellation-first = deterministic: once parent cancelled, always return Cancelled
        let parent_cancelled = tokio::select! {
            biased;
            _ = self.cancel_token.cancelled() => true,
            _ = &mut join_handle => false,
        };

        // CRITICAL: Always await join_handle to avoid orphaned tasks
        // This ensures:
        // 1. Task completes (not orphaned in scope tree)
        // 2. Panics are detected (proper cleanup)
        // 3. No dangling JoinHandle references
        let join_result = join_handle.await;

        match join_result {
            Ok(()) => {
                if parent_cancelled {
                    // Parent cancelled, but task completed - return Cancelled
                    Err(TaskError::Cancelled)
                } else {
                    // Task completed normally, get result from oneshot
                    rx.await.unwrap_or(Err(TaskError::Aborted))
                }
            }
            Err(join_err) if join_err.is_panic() => {
                // Task panicked - propagate panic regardless of parent cancellation
                let panic_payload = join_err.into_panic();
                let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "panic with non-string payload".to_string()
                };
                Err(TaskError::Panicked(panic_msg))
            }
            Err(_) => {
                // Task cancelled by runtime shutdown
                Err(TaskError::Aborted)
            }
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
            job: self.job.new_child(),
            cancel_token: self.cancel_token.child(),
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
        // biased + cancellation-first = deterministic: once parent cancelled, always return Cancelled
        tokio::select! {
            biased;
            _ = self.parent_cancel_token.cancelled() => Err(TaskError::Cancelled),
            join_result = self.join_handle => {
                match join_result {
                    Ok(()) => {
                        // Task completed normally, get result from oneshot
                        // RecvError only occurs if sender dropped without sending (runtime abort/shutdown)
                        self.rx.await.unwrap_or(Err(TaskError::Aborted))
                    }
                    Err(join_err) if join_err.is_panic() => {
                        // Task panicked - use JoinError to get panic info (proper Tokio idiom)
                        let panic_payload = join_err.into_panic();
                        let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                            s.to_string()
                        } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "panic with non-string payload".to_string()
                        };
                        Err(TaskError::Panicked(panic_msg))
                    }
                    Err(_) => {
                        // Task cancelled by runtime shutdown
                        Err(TaskError::Aborted)
                    }
                }
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
                let panic_payload = join_err.into_panic();
                let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "panic with non-string payload".to_string()
                };
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
