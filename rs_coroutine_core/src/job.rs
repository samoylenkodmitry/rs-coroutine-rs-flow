use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

/// Inner state of a CancelToken
struct CancelTokenInner {
    cancelled: AtomicBool,
    notify: Notify,
    parent: Option<Arc<CancelTokenInner>>,
}

/// A cancellation token for cooperative cancellation
///
/// Supports hierarchical cancellation: when a parent token is cancelled,
/// all child tokens are automatically considered cancelled as well.
/// Children can also be cancelled independently without affecting the parent.
///
/// Optimized to avoid recursive heap allocation by wrapping Arc<Inner>.
#[derive(Clone)]
pub struct CancelToken {
    inner: Arc<CancelTokenInner>,
}

impl CancelToken {
    /// Create a new root CancelToken
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CancelTokenInner {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
                parent: None,
            }),
        }
    }

    /// Cancel this token
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    /// Check if this token is cancelled
    /// This checks both this token and all parent tokens in the hierarchy
    /// Uses iterative approach to avoid stack overflow on deep nesting
    pub fn is_cancelled(&self) -> bool {
        let mut current: Option<&Arc<CancelTokenInner>> = Some(&self.inner);

        while let Some(inner) = current {
            if inner.cancelled.load(Ordering::SeqCst) {
                return true;
            }
            current = inner.parent.as_ref();
        }

        false
    }

    /// Wait for cancellation
    /// This will return when either this token or any parent token is cancelled
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        // We need to wait on both our own notify and parent's
        // Create a future that completes when either this token or parent is cancelled
        let own_notified = self.inner.notify.notified();

        if let Some(parent_inner) = &self.inner.parent {
            // Create parent token to await
            let parent = CancelToken {
                inner: Arc::clone(parent_inner),
            };
            // Box the recursive call to avoid infinite future size
            tokio::select! {
                _ = own_notified => {},
                _ = Box::pin(parent.cancelled()) => {},
            }
        } else {
            own_notified.await;
        }
    }

    /// Create a child token that is linked to this parent
    /// The child will be automatically cancelled when the parent is cancelled,
    /// but can also be cancelled independently without affecting the parent.
    pub fn child(&self) -> Self {
        Self {
            inner: Arc::new(CancelTokenInner {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
                parent: Some(Arc::clone(&self.inner)),
            }),
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
