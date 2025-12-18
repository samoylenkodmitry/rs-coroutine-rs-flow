/// Internal utilities for Flow implementation
///
/// This module contains helpers used by Flow operators to integrate with
/// structured concurrency properly.
use rs_coroutine_core::{CancelToken, JobHandle};
use std::future::Future;

/// Spawn a task in the current scope (REQUIRES CoroutineScope)
///
/// This helper integrates Flow operators with structured concurrency by:
/// 1. Checking if there's a current CoroutineScope via CURRENT_SCOPE
/// 2. Using scope.launch() to spawn in the scope tree
/// 3. **PANICS** if no scope is available
///
/// # Panics
///
/// Panics if called outside a CoroutineScope. This is intentional to enforce
/// structured concurrency and prevent accidental task leaks.
///
/// # Why No Fallback?
///
/// This library is explicitly for **Structured Concurrency**. The whole point
/// is that tasks cannot leak. A "lenient fallback" to detached tasks would:
/// - Silently degrade to unstructured concurrency
/// - Allow zombie tasks that can't be cancelled
/// - Defeat the core promise of the library
///
/// # Solution
///
/// Wrap your Flow operations in a CoroutineScope:
///
/// ```ignore
/// // ✅ CORRECT: Use a scope
/// let scope = CoroutineScope::new(Dispatchers::default());
/// scope.launch(async {
///     flow.buffer(10).for_each(|x| async move { ... }).await;
/// });
///
/// // ❌ WRONG: No scope - will panic!
/// flow.buffer(10).for_each(|x| async move { ... }).await;  // PANIC!
/// ```
///
/// # For Tests
///
/// Use `coroutine_scope!` or create a scope:
/// ```ignore
/// #[tokio::test]
/// async fn test_flow() {
///     let scope = CoroutineScope::new(Dispatchers::default());
///     scope.launch(async {
///         // Your flow code here
///     }).join().await;
/// }
/// ```
pub fn spawn_in_scope<F>(fut: F) -> ScopeAwareHandle
where
    F: Future<Output = ()> + Send + 'static,
{
    // Try to get current scope via CURRENT_SCOPE
    match rs_coroutine_core::CURRENT_SCOPE.try_with(|scope| scope.clone()) {
        Ok(scope) => {
            // We're in a scope - use structured spawning
            let cancel_token = scope.cancel_token.clone();
            let job = scope.launch(fut);
            ScopeAwareHandle {
                job,
                cancel_token,
            }
        }
        Err(_) => {
            // No scope available - PANIC to enforce structured concurrency
            panic!(
                "spawn_in_scope called outside CoroutineScope!\n\
                 \n\
                 This library requires structured concurrency. You must wrap Flow operations\n\
                 in a CoroutineScope to prevent task leaks.\n\
                 \n\
                 Solution:\n\
                 \n\
                 let scope = CoroutineScope::new(Dispatchers::default());\n\
                 scope.launch(async {{\n\
                     // Your flow code here\n\
                 }});\n\
                 \n\
                 Or use the coroutine_scope! macro for a scoped block.\n\
                 \n\
                 See documentation for CoroutineScope for more details."
            );
        }
    }
}

/// A handle to a task spawned in a CoroutineScope
///
/// This wraps both the JobHandle (for completion tracking) and the CancelToken
/// (for cancellation). This design makes cancellation explicit and avoids
/// the dual-token hierarchy confusion.
pub struct ScopeAwareHandle {
    job: JobHandle,
    cancel_token: CancelToken,
}

impl ScopeAwareHandle {
    /// Wait for the task to complete
    pub async fn join(self) {
        self.job.join().await;
    }

    /// Cancel the task immediately (cooperative cancellation)
    pub fn cancel(self) {
        self.cancel_token.cancel();
    }

    /// Cancel the task and wait for it to complete
    ///
    /// CRITICAL: Required for flat_map_latest to properly switch streams.
    /// This ensures the old stream is fully stopped before starting a new one,
    /// preventing concurrent stream execution which could cause data races.
    pub async fn cancel_and_join(self) {
        self.cancel_token.cancel();
        self.job.join().await;
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
