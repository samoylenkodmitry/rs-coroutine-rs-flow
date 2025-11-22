use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

/// A cancellation token for cooperative cancellation
#[derive(Clone)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl CancelToken {
    /// Create a new CancelToken
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Cancel this token
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    /// Check if this token is cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Wait for cancellation
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }

    /// Create a child token that can be cancelled independently
    pub fn child(&self) -> Self {
        Self::new()
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
