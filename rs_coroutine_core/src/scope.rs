use crate::error::{CancellationError, TaskError};
use crate::executor::{BoxedJoinHandle, Dispatcher};
use crate::job::{CancelToken, JobHandle};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::oneshot;

tokio::task_local! {
    pub static CURRENT_SCOPE: Arc<CoroutineScope>;
}

/// Future returned by with_dispatcher that cancels the job on drop
///
/// CRITICAL: This prevents task detachment when the future is dropped
/// (e.g., in a timeout or select!). The cancel token is cancelled immediately,
/// signaling the spawned task to stop.
///
/// CRITICAL: This Future is also responsible for completing the job when the
/// task finishes. It polls the join_handle directly and reports the outcome
/// to the JobHandle.
struct WithDispatcherFuture<T> {
    join_handle: Option<BoxedJoinHandle>,
    rx: oneshot::Receiver<Result<T, TaskError>>,
    child_cancel_token: CancelToken,
    job: JobHandle,
    join_completed: bool,
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
        // CRITICAL: Do NOT check parent_token here!
        // Parent cancellation already propagates via child token hierarchy.
        // The spawned task will send Err(Cancelled) when cancellation wins.
        // Checking parent here creates "await timing changes outcome" behavior.

        // Poll join handle if not yet completed
        if !self.join_completed {
            if let Some(mut handle) = self.join_handle.take() {
                let handle_pin = std::pin::Pin::new(&mut handle);
                match handle_pin.poll(cx) {
                    std::task::Poll::Ready(Ok(())) => {
                        // Task completed normally (no panic/abort)
                        self.join_completed = true;
                        // Don't return yet - need to get result from oneshot
                        // Fall through to poll rx below
                    }
                    std::task::Poll::Ready(Err(join_err)) if join_err.is_panic() => {
                        // Task panicked - highest priority, return immediately
                        let panic_msg = crate::error::extract_panic_message(join_err);
                        self.job.complete_with(Err(TaskError::Panicked(panic_msg.clone())));
                        self.join_completed = true;
                        return std::task::Poll::Ready(Err(TaskError::Panicked(panic_msg)));
                    }
                    std::task::Poll::Ready(Err(_)) => {
                        // Task aborted (runtime shutdown)
                        self.job.complete_with(Err(TaskError::Aborted));
                        self.join_completed = true;
                        return std::task::Poll::Ready(Err(TaskError::Aborted));
                    }
                    std::task::Poll::Pending => {
                        // Put handle back and return Pending
                        self.join_handle = Some(handle);
                        return std::task::Poll::Pending;
                    }
                }
            }
        }

        // Join handle completed (or already was completed) - poll oneshot for result
        let rx_pin = std::pin::Pin::new(&mut self.rx);
        match rx_pin.poll(cx) {
            std::task::Poll::Ready(Ok(result)) => {
                // Got result - complete the job based on task outcome
                match &result {
                    Ok(_) => self.job.complete_with(Ok(())),
                    Err(e) => self.job.complete_with(Err(e.clone())),
                }
                return std::task::Poll::Ready(result);
            }
            std::task::Poll::Ready(Err(_)) => {
                // Sender dropped without sending (shouldn't happen but handle gracefully)
                self.job.complete_with(Err(TaskError::Aborted));
                return std::task::Poll::Ready(Err(TaskError::Aborted));
            }
            std::task::Poll::Pending => {
                // Result not ready yet - keep polling
                // This is OK even if join handle completed - the send might be in flight
                return std::task::Poll::Pending;
            }
        }
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
        // CRITICAL FIX: Create token first, then pass to JobHandle
        let cancel_token = CancelToken::new();
        Self {
            dispatcher,
            job: JobHandle::new(cancel_token.clone()),
            cancel_token,
        }
    }

    /// Launch a new coroutine in this scope
    /// The coroutine will be automatically cancelled when the scope is cancelled
    pub fn launch<F>(&self, fut: F) -> JobHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        // CRITICAL FIX: Create a CHILD token for this specific job
        // This restores hierarchical cancellation - each job can be cancelled independently
        let child_token = self.cancel_token.child();

        // JobHandle owns this specific child token
        let job = JobHandle::new(child_token.clone());

        // Create a new child scope for the task to run in
        // This ensures children-of-children attach to THIS job, not the root
        let child_scope = Arc::new(CoroutineScope {
            dispatcher: self.dispatcher.clone(),
            job: job.clone(),
            cancel_token: child_token.clone(),
        });

        let job_for_observer = job.clone();
        let cancel_token_for_task = child_token.clone(); // Use child token!

        // Channel to communicate which select branch won
        let (outcome_tx, outcome_rx) = oneshot::channel();

        let join_handle = child_scope.dispatcher.clone().spawn(async move {
            // NO GUARD - Observer is single source of truth for outcome
            // This prevents the race where guard stores Ok() before observer detects panic

            let result = CURRENT_SCOPE
                .scope(child_scope.clone(), async move {
                    // CRITICAL: Check cancellation BEFORE starting work
                    // This prevents "already cancelled but ran anyway" races
                    if cancel_token_for_task.is_cancelled() {
                        return Err(TaskError::Cancelled);
                    }

                    // CRITICAL: Race the future against cancellation
                    // biased + cancellation-first = deterministic cancellation semantics
                    // This ensures scope.cancel() actually stops the future
                    tokio::select! {
                        biased;
                        _ = cancel_token_for_task.cancelled() => {
                            // Cancellation branch won
                            Err(TaskError::Cancelled)
                        }
                        _ = fut => {
                            // Future completed normally
                            Ok(())
                        }
                    }
                })
                .await;

            // Send the actual outcome (which select branch won) to observer
            let _ = outcome_tx.send(result);
        });

        // Spawn observer task - SINGLE SOURCE OF TRUTH for job outcome
        // CRITICAL: This is the ONLY place that completes the job
        // Prevents panic masking race where guard completes as Ok before observer detects panic
        tokio::spawn(async move {
            match join_handle.await {
                Ok(()) => {
                    // Task completed - get the actual outcome from the oneshot
                    match outcome_rx.await {
                        Ok(result) => {
                            // Use the result from the select site (source of truth)
                            job_for_observer.complete_with(result);
                        }
                        Err(_) => {
                            // Sender dropped without sending - shouldn't happen but treat as aborted
                            job_for_observer.complete_with(Err(TaskError::Aborted));
                        }
                    }
                }
                Err(join_err) if join_err.is_panic() => {
                    // Task panicked - highest priority outcome (overrides select result)
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

        // CRITICAL FIX: Create child token for this job
        let child_token = self.cancel_token.child();
        let job = JobHandle::new(child_token.clone());

        let child_scope = Arc::new(CoroutineScope {
            dispatcher: dispatcher.clone(),
            job: job.clone(),
            cancel_token: child_token.clone(),
        });
        let cancel_token = child_scope.cancel_token.clone();
        let job_for_future = job.clone();
        let child_cancel_token_for_guard = child_scope.cancel_token.clone();

        let join_handle = dispatcher.spawn(async move {
            // NO GUARD - WithDispatcherFuture will be single source of truth for job outcome

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
        });

        // CRITICAL: Return a future that cancels the token on drop
        // This prevents detachment when the outer future is dropped (e.g., in timeout)
        // The future is also responsible for completing the job when it polls the join_handle
        WithDispatcherFuture {
            join_handle: Some(join_handle),
            rx,
            child_cancel_token: child_cancel_token_for_guard,
            job: job_for_future,
            join_completed: false,
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

        // CRITICAL FIX: Create child token for this job
        let child_token = self.cancel_token.child();
        let job = JobHandle::new(child_token.clone());

        let child_scope = Arc::new(CoroutineScope {
            dispatcher: dispatcher.clone(),
            job: job.clone(),
            cancel_token: child_token.clone(),
        });
        let cancel_token = child_scope.cancel_token.clone();
        let job_for_observer = job.clone();

        // Separate channel to communicate outcome to observer
        let (outcome_tx, outcome_rx) = oneshot::channel();

        let join_handle = dispatcher.spawn(async move {
            // NO GUARD - Observer will be single source of truth for job outcome

            // CRITICAL: Check cancellation BEFORE starting work
            if cancel_token.is_cancelled() {
                let _ = tx.send(Err(TaskError::Cancelled));
                let _ = outcome_tx.send(Err(TaskError::Cancelled));
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

            // Send result to both Deferred and observer
            let job_outcome = result.as_ref().map(|_| ()).map_err(|e| e.clone());
            let _ = tx.send(result);
            let _ = outcome_tx.send(job_outcome);
        });

        // Spawn observer to complete the job and detect panics
        tokio::spawn(async move {
            match join_handle.await {
                Ok(()) => {
                    // Task completed - get actual outcome from select site
                    match outcome_rx.await {
                        Ok(result) => {
                            // Use the result from the select site (source of truth)
                            job_for_observer.complete_with(result);
                        }
                        Err(_) => {
                            // Sender dropped without sending - shouldn't happen
                            job_for_observer.complete_with(Err(TaskError::Aborted));
                        }
                    }
                }
                Err(join_err) if join_err.is_panic() => {
                    let panic_msg = crate::error::extract_panic_message(join_err);
                    job_for_observer.complete_with(Err(TaskError::Panicked(panic_msg)));
                }
                Err(_) => {
                    job_for_observer.complete_with(Err(TaskError::Aborted));
                }
            }
        });

        Deferred {
            rx,
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
    job: JobHandle,
    parent_cancel_token: CancelToken,
}

impl<T> Deferred<T> {
    /// Await the deferred value
    ///
    /// This races the child task against parent cancellation. If the result is already
    /// computed when you call this method, it will be returned even if the parent was
    /// cancelled. Only if we need to *wait* for the result do we check parent cancellation.
    ///
    /// # Errors
    ///
    /// - `TaskError::Cancelled` if parent scope is cancelled *while waiting*
    /// - `TaskError::Panicked` if the task panics
    /// - `TaskError::Aborted` if the task is dropped before completion
    ///
    /// Note: If the child task itself was cancelled (not the parent), this returns
    /// `Err(TaskError::Cancelled)` from the child.
    pub async fn await_result(mut self) -> Result<T, TaskError> {
        // CRITICAL: Check if result is already available BEFORE racing with parent cancellation
        // This prevents "await timing changes outcome" behavior where a completed result
        // gets thrown away just because parent was cancelled between completion and await.
        match self.rx.try_recv() {
            Ok(result) => {
                // Result is already available - return it immediately
                // Parent cancellation doesn't matter; the work is already done
                return result;
            }
            Err(_) => {
                // Result not ready yet - fall through to race against parent cancellation
            }
        }

        // Result not available yet - race between parent cancellation and task completion
        tokio::select! {
            biased;
            _ = self.parent_cancel_token.cancelled() => {
                // Parent cancelled while waiting - return Cancelled
                // Note: This doesn't corrupt the job outcome; the job was completed by the observer
                Err(TaskError::Cancelled)
            }
            result = self.rx => {
                // Result arrived - return it
                result.unwrap_or(Err(TaskError::Aborted))
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
        // The observer handles the join_handle and completes the job
        self.job.join().await;

        // Job completed - get result from oneshot
        // RecvError only occurs if sender dropped without sending (runtime abort/shutdown)
        self.rx.await.unwrap_or(Err(TaskError::Aborted))
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
