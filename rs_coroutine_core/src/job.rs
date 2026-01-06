use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::error::TaskError;

/// A cancellation token for cooperative cancellation
///
/// This is a thin wrapper around tokio_util::sync::CancellationToken
/// which provides battle-tested, correct cancellation semantics.
///
/// Supports hierarchical cancellation: when a parent token is cancelled,
/// all child tokens are automatically considered cancelled as well.
/// Children can also be cancelled independently without affecting the parent.
#[derive(Clone)]
pub struct CancelToken {
    inner: CancellationToken,
}

impl CancelToken {
    /// Create a new root CancelToken
    pub fn new() -> Self {
        Self {
            inner: CancellationToken::new(),
        }
    }

    /// Cancel this token
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    /// Check if this token is cancelled
    /// This checks both this token and all parent tokens in the hierarchy
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// Wait for cancellation
    /// This will return when either this token or any parent token is cancelled
    ///
    /// This implementation is CORRECT and does not have missed wake-up races.
    pub async fn cancelled(&self) {
        self.inner.cancelled().await;
    }

    /// Create a child token that is linked to this parent
    /// The child will be automatically cancelled when the parent is cancelled,
    /// but can also be cancelled independently without affecting the parent.
    pub fn child(&self) -> Self {
        Self {
            inner: self.inner.child_token(),
        }
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

/// A handle to a job that tracks completion and outcome
///
/// CRITICAL CHANGE: JobHandle now OWNS its cancellation token.
/// This restores the hierarchical structure required for Structured Concurrency:
/// - Each job has its own child token
/// - Cancelling a job cancels only that job and its children
/// - Parent cancellation still propagates down via token hierarchy
///
/// ## Implementation Notes
///
/// Uses `AtomicBool` + `Notify` pattern to avoid missed wakeups:
/// - `is_completed` is checked BEFORE awaiting notification
/// - This prevents the race where `complete_with()` fires before `join()` starts waiting
///
/// Uses `std::sync::Mutex` (not `tokio::sync::Mutex`) for outcome:
/// - Blocking lock ensures outcome is NEVER lost (no `try_lock` failures)
/// - Safe because critical section is tiny (just a comparison + store)
#[derive(Clone)]
pub struct JobHandle {
    /// CRITICAL: Job owns its cancellation token
    /// This allows individual job cancellation and proper hierarchical structure
    cancel_token: CancelToken,
    completed: Arc<Notify>,
    outcome: Arc<StdMutex<Option<Result<(), TaskError>>>>,
    is_completed: Arc<AtomicBool>,
}

impl JobHandle {
    /// Create a new JobHandle with a cancellation token
    ///
    /// CRITICAL: Caller must provide a child token from the parent scope
    /// to maintain the cancellation hierarchy.
    pub fn new(cancel_token: CancelToken) -> Self {
        Self {
            cancel_token,
            completed: Arc::new(Notify::new()),
            outcome: Arc::new(StdMutex::new(None)),
            is_completed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Cancel this specific job
    ///
    /// This cancels only this job and its children (via token hierarchy).
    /// Parent jobs are NOT affected.
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Get the cancellation token for this job
    ///
    /// Useful for creating child tasks that should be cancelled when this job is cancelled.
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel_token
    }

    /// Wait for this job to complete (backwards compatible - discards outcome)
    ///
    /// ## Correctness
    ///
    /// Uses the correct Notify pattern to avoid missed wakeup race:
    /// 1. Create the notified() future FIRST (registers waiter)
    /// 2. Then check is_completed flag
    /// 3. If completed, return (dropping the future)
    /// 4. Otherwise await the future
    ///
    /// This ensures the waiter is registered BEFORE we check completion,
    /// preventing the race where notify_waiters() fires between check and await.
    pub async fn join(&self) {
        // CRITICAL: Create notified future BEFORE checking flag
        // This registers us as a waiter immediately
        let notified = self.completed.notified();

        // Now check if already completed
        if self.is_completed.load(Ordering::Acquire) {
            return; // Drop the notified future, we don't need it
        }

        // Wait for notification (we're already registered as a waiter)
        notified.await;
    }

    /// Wait for this job to complete and get the outcome
    pub async fn join_result(&self) -> Result<(), TaskError> {
        // CRITICAL: Same pattern as join() - create notified BEFORE checking flag
        let notified = self.completed.notified();

        if !self.is_completed.load(Ordering::Acquire) {
            notified.await;
        }

        // Get the outcome (using std::sync::Mutex, so this is a blocking lock)
        // Safe because: (1) critical section is tiny, (2) no await inside lock
        self.outcome
            .lock()
            .expect("JobHandle outcome mutex poisoned")
            .clone()
            .unwrap_or(Ok(()))
    }

    /// Mark this job as completed with success
    ///
    /// CRITICAL: Restricted to pub(crate) to prevent external code from spoofing completion.
    /// Only the library internals (observer tasks) should determine job outcome.
    #[allow(dead_code)]
    pub(crate) fn complete(&self) {
        self.complete_with(Ok(()));
    }

    /// Mark this job as completed with an outcome
    ///
    /// CRITICAL: Restricted to pub(crate) to prevent completion races.
    /// If this were `pub`, user code could call it and race with the observer's
    /// legitimate completion (e.g., user sets "Success" right before observer sets "Panic").
    ///
    /// ## Outcome Upgrade Policy
    ///
    /// Allows "worse" outcomes to overwrite "better" ones to ensure critical errors
    /// are never masked:
    ///
    /// - **Panicked** > Aborted > Cancelled > Ok (success)
    /// - If outcome is already `Panicked`, it cannot be downgraded
    /// - If outcome is `Ok` but new outcome is an error, upgrade to the error
    ///
    /// This prevents the race where:
    /// 1. Observer might try to store `Ok(())` for normal completion
    /// 2. Later, another thread detects panic and tries to store `Panicked`
    /// 3. Without upgrades, panic would be lost and job reports success
    ///
    /// ## Thread Safety
    ///
    /// Uses `std::sync::Mutex` (blocking) instead of `try_lock` to guarantee
    /// outcome is NEVER silently lost due to lock contention.
    pub(crate) fn complete_with(&self, result: Result<(), TaskError>) {
        // CRITICAL: Use blocking lock, not try_lock
        // If this blocks, it's only for microseconds (tiny critical section)
        let mut outcome = self
            .outcome
            .lock()
            .expect("JobHandle outcome mutex poisoned");

        // Determine if we should update the outcome (allow upgrades to "worse" outcomes)
        let should_update = match (&*outcome, &result) {
            // No outcome yet - always store
            (None, _) => true,

            // Current is Ok - any error is an upgrade
            (Some(Ok(())), Err(_)) => true,

            // Current is Cancelled - Panicked or Aborted is an upgrade
            (Some(Err(TaskError::Cancelled)), Err(TaskError::Panicked(_))) => true,
            (Some(Err(TaskError::Cancelled)), Err(TaskError::Aborted)) => true,

            // Current is Aborted - only Panicked can upgrade
            (Some(Err(TaskError::Aborted)), Err(TaskError::Panicked(_))) => true,

            // Current is Panicked - cannot downgrade (panic is worst outcome)
            (Some(Err(TaskError::Panicked(_))), _) => false,

            // All other cases - don't overwrite
            _ => false,
        };

        if should_update {
            *outcome = Some(result);
        }

        // Release lock before notifying
        drop(outcome);

        // Set completion flag BEFORE notifying to ensure join() sees it
        self.is_completed.store(true, Ordering::Release);

        // Wake all waiters
        self.completed.notify_waiters();
    }
}

impl Default for JobHandle {
    fn default() -> Self {
        Self::new(CancelToken::new())
    }
}

/// A guard that aborts a Tokio JoinHandle when dropped
///
/// This is useful for unstructured background tasks (tokio::spawn) that should
/// be aborted if the owner is dropped.
pub struct AbortOnDrop<T> {
    handle: Option<tokio::task::JoinHandle<T>>,
}

impl<T> AbortOnDrop<T> {
    /// Create a new AbortOnDrop guard for a JoinHandle
    pub fn new(handle: tokio::task::JoinHandle<T>) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    /// Get a reference to the handle
    pub fn handle(&self) -> &tokio::task::JoinHandle<T> {
        self.handle
            .as_ref()
            .expect("AbortOnDrop handle already taken")
    }

    /// Take the handle, disabling automatic abort
    pub fn into_inner(mut self) -> tokio::task::JoinHandle<T> {
        self.handle
            .take()
            .expect("AbortOnDrop handle already taken")
    }

    /// Abort the task
    pub fn abort(&self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

impl<T> std::ops::Deref for AbortOnDrop<T> {
    type Target = tokio::task::JoinHandle<T>;

    fn deref(&self) -> &Self::Target {
        self.handle()
    }
}
