use crate::error::{CancellationError, TaskError};
use crate::executor::Dispatcher;
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
                    // Race the future against cancellation
                    // This properly wakes on cancel, unlike poll-based checking
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            // Cancelled before completion
                        }
                        _ = fut => {
                            // Completed normally
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

        dispatcher.spawn(async move {
            // Create guard FIRST - ensures job.complete() is called even on panic
            let _guard = JobCompletionGuard::new(job);

            // Spawn inner task with tokio to capture panics via JoinHandle
            let inner_handle = tokio::spawn(CURRENT_SCOPE.scope(child_scope, async move {
                // Race the future against cancellation (structured concurrency)
                tokio::select! {
                    res = fut => Ok(res),
                    _ = cancel_token.cancelled() => Err(TaskError::Cancelled),
                }
            }));

            // Await the inner task and check for panic
            let result = match inner_handle.await {
                Ok(task_result) => task_result,
                Err(join_err) if join_err.is_panic() => {
                    // Extract panic message
                    let panic_payload = join_err.into_panic();
                    let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "Unknown panic".to_string()
                    };
                    Err(TaskError::Panicked(panic_msg))
                }
                Err(join_err) if join_err.is_cancelled() => Err(TaskError::Cancelled),
                Err(_) => Err(TaskError::Aborted),
            };

            let _ = tx.send(result);
            // Guard's Drop will call job.complete() here
        });

        // Also race on the receiving side - if parent is cancelled, stop waiting
        tokio::select! {
            res = rx => res.unwrap_or(Err(TaskError::Aborted)),
            _ = self.cancel_token.cancelled() => Err(TaskError::Cancelled),
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

        dispatcher.spawn(async move {
            // Create guard FIRST - ensures job.complete() is called even on panic
            let _guard = JobCompletionGuard::new(job_for_spawn);

            // Spawn inner task with tokio to capture panics via JoinHandle
            let inner_handle = tokio::spawn(CURRENT_SCOPE.scope(child_scope, async move {
                // Race the future against cancellation (structured concurrency)
                tokio::select! {
                    res = fut => Ok(res),
                    _ = cancel_token.cancelled() => Err(TaskError::Cancelled),
                }
            }));

            // Await the inner task and check for panic
            let result = match inner_handle.await {
                Ok(task_result) => task_result,
                Err(join_err) if join_err.is_panic() => {
                    // Extract panic message
                    let panic_payload = join_err.into_panic();
                    let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "Unknown panic".to_string()
                    };
                    Err(TaskError::Panicked(panic_msg))
                }
                Err(join_err) if join_err.is_cancelled() => Err(TaskError::Cancelled),
                Err(_) => Err(TaskError::Aborted),
            };

            let _ = tx.send(result);
            // Guard's Drop will call job.complete() here
        });

        Deferred {
            rx,
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
        // Race receiving the result against parent cancellation
        tokio::select! {
            res = self.rx => res.unwrap_or(Err(TaskError::Aborted)),
            _ = self.parent_cancel_token.cancelled() => Err(TaskError::Cancelled),
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

/// Check if the current scope is cancelled and return an error if so
/// This can be used in async functions to check for cancellation
///
/// Returns Ok(()) if not in a scope context (instead of panicking)
pub fn check_cancellation() -> Result<(), CancellationError> {
    match CURRENT_SCOPE.try_with(|scope| scope.is_cancelled()) {
        Ok(true) => Err(CancellationError),
        Ok(false) => Ok(()),
        Err(_) => Ok(()), // Not in a scope context - treat as not cancelled
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
        $crate::check_cancelled!();
    };
}
