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

    /// Convert to a drop guard that does nothing on drop
    /// (tasks are already managed by their scope or tokio)
    pub fn into_keep_alive(self) -> KeepAlive {
        KeepAlive(Some(self))
    }
}

/// A wrapper that keeps a task alive until dropped
///
/// Unlike AbortOnDrop, this doesn't abort the task - it just ensures
/// the task handle isn't dropped prematurely. Tasks will complete normally
/// or be cancelled through scope cancellation (cooperative).
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
        // This is intentional - scoped tasks will be cleaned up by scope cancellation
        // Unscoped tasks will complete independently (fallback behavior)
    }
}
