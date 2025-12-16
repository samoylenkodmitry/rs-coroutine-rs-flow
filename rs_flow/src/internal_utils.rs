/// Internal utilities for Flow implementation
///
/// This module contains helpers used by Flow operators to integrate with
/// structured concurrency properly.

use rs_coroutine_core::JobHandle;
use std::future::Future;

/// Spawn a task in the current scope if available, otherwise use tokio::spawn
///
/// This helper integrates Flow operators with structured concurrency by:
/// 1. Checking if there's a current CoroutineScope via CURRENT_SCOPE
/// 2. If yes, using scope.launch() to spawn in the scope tree
/// 3. If no, falling back to unstructured tokio::spawn
///
/// Returns a handle that can be used to join the task or detect panics.
///
/// # Example
/// ```ignore
/// let handle = spawn_in_scope(async {
///     // Task work here
/// });
/// handle.join().await;
/// ```
pub fn spawn_in_scope<F>(fut: F) -> ScopeAwareHandle
where
    F: Future<Output = ()> + Send + 'static,
{
    // Try to get current scope via CURRENT_SCOPE
    match rs_coroutine_core::CURRENT_SCOPE.try_with(|scope| scope.clone()) {
        Ok(scope) => {
            // We're in a scope - use structured spawning
            let job = scope.launch(fut);
            ScopeAwareHandle::Scoped(job)
        }
        Err(_) => {
            // No current scope - fall back to unstructured spawn
            let handle = tokio::spawn(fut);
            ScopeAwareHandle::Unscoped(handle)
        }
    }
}

/// A handle to a task that might be scoped or unscoped
pub enum ScopeAwareHandle {
    /// Task spawned in a CoroutineScope
    Scoped(JobHandle),
    /// Task spawned with tokio::spawn
    Unscoped(tokio::task::JoinHandle<()>),
}

impl ScopeAwareHandle {
    /// Wait for the task to complete
    pub async fn join(self) {
        match self {
            ScopeAwareHandle::Scoped(job) => {
                job.join().await;
            }
            ScopeAwareHandle::Unscoped(handle) => {
                let _ = handle.await;
            }
        }
    }

    /// Cancel/abort the task immediately
    ///
    /// For scoped tasks: cancels the job (cooperative cancellation)
    /// For unscoped tasks: aborts the task (forced cancellation)
    pub fn cancel(self) {
        match self {
            ScopeAwareHandle::Scoped(job) => {
                job.cancel();
            }
            ScopeAwareHandle::Unscoped(handle) => {
                handle.abort();
            }
        }
    }

    /// Convert to a cancel-on-drop guard (default, safe behavior)
    pub fn into_cancel_on_drop(self) -> CancelOnDrop {
        CancelOnDrop(Some(self))
    }

    /// UNSAFE: Convert to keep-alive guard (task keeps running on drop)
    ///
    /// WARNING: For unscoped tasks, dropping the guard DETACHES the task.
    /// Only use this if you're absolutely sure the task will be cleaned up
    /// via other means (e.g., parent scope cancellation).
    #[allow(dead_code)]
    pub fn into_keep_alive(self) -> KeepAlive {
        KeepAlive(Some(self))
    }
}

/// A cancel-on-drop guard (DEFAULT, SAFE)
///
/// When dropped, this cancels the task immediately:
/// - Scoped tasks: calls job.cancel() (cooperative cancellation)
/// - Unscoped tasks: calls handle.abort() (forced cancellation)
///
/// This is the safe default that prevents task leaks.
pub struct CancelOnDrop(Option<ScopeAwareHandle>);

impl CancelOnDrop {
    /// Wait for the task to complete, consuming the guard
    #[allow(dead_code)]
    pub async fn join(mut self) {
        if let Some(handle) = self.0.take() {
            handle.join().await;
        }
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        // CRITICAL: Cancel the task on drop to prevent leaks
        if let Some(handle) = self.0.take() {
            handle.cancel();
        }
    }
}

/// A keep-alive guard (UNSAFE, USE WITH CAUTION)
///
/// WARNING: This does NOT cancel the task on drop!
/// - Scoped tasks: rely on scope cancellation (usually safe)
/// - Unscoped tasks: DETACH when guard is dropped (LEAK!)
///
/// Only use this if you're certain the task will be cleaned up via
/// other means (e.g., parent scope cancellation).
#[allow(dead_code)]
pub struct KeepAlive(Option<ScopeAwareHandle>);

#[allow(dead_code)]
impl KeepAlive {
    /// Wait for the task to complete, consuming the guard
    pub async fn join(mut self) {
        if let Some(handle) = self.0.take() {
            handle.join().await;
        }
    }
}

impl Drop for KeepAlive {
    fn drop(&mut self) {
        // Task handle is dropped but task keeps running
        // WARNING: For unscoped tasks, this DETACHES the task!
        // This is intentional but dangerous - use with caution
    }
}
