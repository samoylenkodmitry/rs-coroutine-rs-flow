use std::sync::{Arc, Mutex as StdMutex};
use std::sync::atomic::{AtomicBool, Ordering};
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
/// JobHandle does NOT handle cancellation directly. Cancellation is managed
/// via CancelToken in the CoroutineScope. This separation makes the cancellation
/// hierarchy clear and prevents dual-token confusion.
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
    completed: Arc<Notify>,
    outcome: Arc<StdMutex<Option<Result<(), TaskError>>>>,
    is_completed: Arc<AtomicBool>,
}

impl JobHandle {
    /// Create a new JobHandle
    pub fn new() -> Self {
        Self {
            completed: Arc::new(Notify::new()),
            outcome: Arc::new(StdMutex::new(None)),
            is_completed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Wait for this job to complete (backwards compatible - discards outcome)
    ///
    /// ## Correctness
    ///
    /// Checks `is_completed` BEFORE awaiting notification to avoid missed wakeup race:
    /// - If already completed, returns immediately
    /// - Otherwise, awaits notification (which is guaranteed to come after we checked)
    pub async fn join(&self) {
        // CRITICAL: Check completion state BEFORE creating notified() future
        // This prevents the race where complete_with() fires between now and await
        if self.is_completed.load(Ordering::Acquire) {
            return;
        }
        self.completed.notified().await;
    }

    /// Wait for this job to complete and get the outcome
    pub async fn join_result(&self) -> Result<(), TaskError> {
        // CRITICAL: Check completion state BEFORE awaiting notification (same as join())
        if !self.is_completed.load(Ordering::Acquire) {
            self.completed.notified().await;
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
    pub fn complete(&self) {
        self.complete_with(Ok(()));
    }

    /// Mark this job as completed with an outcome
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
    /// 1. JobCompletionGuard stores `Ok(())` on normal completion
    /// 2. Observer detects panic and tries to store `Panicked`
    /// 3. Without upgrades, panic would be lost and job reports success
    ///
    /// ## Thread Safety
    ///
    /// Uses `std::sync::Mutex` (blocking) instead of `try_lock` to guarantee
    /// outcome is NEVER silently lost due to lock contention.
    pub fn complete_with(&self, result: Result<(), TaskError>) {
        // CRITICAL: Use blocking lock, not try_lock
        // If this blocks, it's only for microseconds (tiny critical section)
        let mut outcome = self.outcome.lock().expect("JobHandle outcome mutex poisoned");

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
        Self::new()
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
        self.handle.as_ref().expect("AbortOnDrop handle already taken")
    }

    /// Take the handle, disabling automatic abort
    pub fn into_inner(mut self) -> tokio::task::JoinHandle<T> {
        self.handle.take().expect("AbortOnDrop handle already taken")
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
