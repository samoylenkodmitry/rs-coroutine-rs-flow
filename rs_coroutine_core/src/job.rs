use std::sync::Arc;
use tokio_util::sync::CancellationToken;

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

/// A handle to a job that can be cancelled and awaited
#[derive(Clone)]
pub struct JobHandle {
    cancel_token: CancelToken,
    completed: Arc<Notify>,
}

impl JobHandle {
    /// Create a new JobHandle
    pub fn new() -> Self {
        Self {
            cancel_token: CancelToken::new(),
            completed: Arc::new(Notify::new()),
        }
    }

    /// Create a child job
    pub fn new_child(&self) -> Self {
        Self {
            cancel_token: self.cancel_token.child(),
            completed: Arc::new(Notify::new()),
        }
    }

    /// Cancel this job
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Check if this job is cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Wait for this job to complete
    pub async fn join(&self) {
        self.completed.notified().await;
    }

    /// Mark this job as completed
    pub fn complete(&self) {
        self.completed.notify_waiters();
    }

    /// Get the cancel token for this job
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel_token
    }
}

impl Default for JobHandle {
    fn default() -> Self {
        Self::new()
    }
}
