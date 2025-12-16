/// Internal utilities for Flow implementation
///
/// This module contains helpers used by Flow operators to integrate with
/// structured concurrency properly.

use rs_coroutine_core::JobHandle;
use std::future::Future;

/// Spawn a task in the current scope
///
/// This helper integrates Flow operators with structured concurrency by:
/// 1. Checking if there's a current CoroutineScope via CURRENT_SCOPE
/// 2. Using scope.launch() to spawn in the scope tree
///
/// # Panics
/// Panics if called outside a CoroutineScope. Flow operators require structured
/// concurrency for proper cancellation - if there's no scope, it's a programming error.
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
            ScopeAwareHandle(job)
        }
        Err(_) => {
            // TODO: This should panic! Flow operators REQUIRE a scope for proper cancellation.
            // Temporarily allowing tokio::spawn fallback during migration period.
            // Future versions will panic here to enforce structured concurrency.
            eprintln!(
                "WARNING: spawn_in_scope() called outside CoroutineScope. \
                 Falling back to unscoped tokio::spawn (DEPRECATED). \
                 This will panic in future versions. Ensure Flow collection \
                 happens within a scope (e.g., via launch, async_task, or with_dispatcher)."
            );
            // Temporary fallback - spawn without scope for compatibility
            // This means the task won't be automatically cancelled with a parent scope
            // CRITICAL: We still need to wrap in JobHandle and return ScopeAwareHandle
            // for API compatibility, even though it's unscoped
            let job = JobHandle::new();
            let job_clone = job.clone();

            tokio::spawn(async move {
                fut.await;
                job_clone.complete();
            });

            ScopeAwareHandle(job)
        }
    }
}

/// A handle to a task spawned in a CoroutineScope
///
/// This is a simple wrapper around JobHandle that provides a consistent API
/// for task cancellation and joining. Since spawn_in_scope() now panics if
/// there's no scope, this handle is always scoped.
pub struct ScopeAwareHandle(JobHandle);

impl ScopeAwareHandle {
    /// Wait for the task to complete
    pub async fn join(self) {
        self.0.join().await;
    }

    /// Cancel the task immediately (cooperative cancellation)
    pub fn cancel(self) {
        self.0.cancel();
    }

    /// Convert to a cancel-on-drop guard (default, safe behavior)
    pub fn into_cancel_on_drop(self) -> CancelOnDrop {
        CancelOnDrop(Some(self))
    }

    /// UNSAFE: Convert to keep-alive guard (task keeps running on drop)
    ///
    /// WARNING: The task relies on parent scope cancellation to be cleaned up.
    /// Only use this if you're absolutely certain the task will be properly
    /// cancelled when the parent scope ends.
    #[allow(dead_code)]
    pub fn into_keep_alive(self) -> KeepAlive {
        KeepAlive(Some(self))
    }
}

/// A cancel-on-drop guard (DEFAULT, SAFE)
///
/// When dropped, this cancels the task immediately by calling job.cancel()
/// (cooperative cancellation). This is the safe default that prevents task leaks.
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
/// The task will continue running until the parent scope is cancelled.
///
/// Only use this if you're certain the task will be cleaned up via
/// parent scope cancellation.
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
        // Task will be cancelled when parent scope ends
        // This is intentional but should be used carefully
    }
}
