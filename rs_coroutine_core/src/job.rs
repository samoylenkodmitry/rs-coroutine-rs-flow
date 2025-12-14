use std::sync::Arc;
use tokio::sync::Notify;
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

/// A guard that cancels a job when dropped
///
/// This is useful for background tasks that should be cancelled if the owner is dropped.
/// For example, Flow operators can use this to ensure producer tasks are cleaned up.
pub struct CancelOnDrop {
    job: Option<JobHandle>,
}

impl CancelOnDrop {
    /// Create a new CancelOnDrop guard for a job
    pub fn new(job: JobHandle) -> Self {
        Self { job: Some(job) }
    }

    /// Get a reference to the job handle
    pub fn job(&self) -> &JobHandle {
        self.job.as_ref().expect("CancelOnDrop job already taken")
    }

    /// Take the job handle, disabling automatic cancellation
    pub fn into_inner(mut self) -> JobHandle {
        self.job.take().expect("CancelOnDrop job already taken")
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancel();
            // Note: We cancel but don't join() - the task will stop cooperatively
            // Joining would block the Drop, which is not allowed in async contexts
        }
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
