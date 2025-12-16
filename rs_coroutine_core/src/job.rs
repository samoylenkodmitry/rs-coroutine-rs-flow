use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
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
#[derive(Clone)]
pub struct JobHandle {
    completed: Arc<Notify>,
    outcome: Arc<Mutex<Option<Result<(), TaskError>>>>,
}

impl JobHandle {
    /// Create a new JobHandle
    pub fn new() -> Self {
        Self {
            completed: Arc::new(Notify::new()),
            outcome: Arc::new(Mutex::new(None)),
        }
    }

    /// Wait for this job to complete (backwards compatible - discards outcome)
    pub async fn join(&self) {
        self.completed.notified().await;
    }

    /// Wait for this job to complete and get the outcome
    pub async fn join_result(&self) -> Result<(), TaskError> {
        self.completed.notified().await;
        // Get the outcome, defaulting to success if none was set
        self.outcome
            .lock()
            .await
            .clone()
            .unwrap_or(Ok(()))
    }

    /// Mark this job as completed with success
    pub fn complete(&self) {
        self.complete_with(Ok(()));
    }

    /// Mark this job as completed with an outcome
    pub fn complete_with(&self, result: Result<(), TaskError>) {
        // Store the outcome (don't overwrite if already set)
        if let Ok(mut outcome) = self.outcome.try_lock() {
            if outcome.is_none() {
                *outcome = Some(result);
            }
        }
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
