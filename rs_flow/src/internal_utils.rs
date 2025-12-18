/// Internal utilities for Flow implementation
///
/// This module contains helpers used by Flow operators to integrate with
/// structured concurrency properly.

use rs_coroutine_core::{CancelToken, JobHandle, TaskError};
use std::future::Future;

/// Spawn a task in the current scope (with lenient fallback)
///
/// This helper integrates Flow operators with structured concurrency by:
/// 1. Checking if there's a current CoroutineScope via CURRENT_SCOPE
/// 2. Using scope.launch() to spawn in the scope tree if available
/// 3. **Falling back to detached spawn** if no scope is available
///
/// # Lenient Fallback Behavior
///
/// When called outside a CoroutineScope, this creates a "detached" task:
/// - Spawns directly with `tokio::spawn` (no parent scope)
/// - Creates an independent `CancelToken` (can only be cancelled via the returned handle)
/// - Task continues running until completion or explicit cancellation
///
/// This allows Flows to work in simpler contexts (like unit tests) without
/// requiring full CoroutineScope setup, similar to Kotlin's Flow behavior.
///
/// # Best Practices
///
/// - **Production code**: Always use within a CoroutineScope for proper cancellation hierarchy
/// - **Tests/Prototypes**: Lenient fallback allows quick experimentation
/// - **Concurrent operators** (buffer, merge, etc.): Work in both modes but cancellation
///   is more limited in detached mode
///
/// # Example
/// ```ignore
/// // In production - structured concurrency
/// scope.launch(async {
///     flow.buffer(10).collect(|x| { ... }).await;
/// });
///
/// // In tests - lenient fallback (detached)
/// flow.buffer(10).collect(|x| { ... }).await; // Still works!
/// ```
pub fn spawn_in_scope<F>(fut: F) -> ScopeAwareHandle
where
    F: Future<Output = ()> + Send + 'static,
{
    // Try to get current scope via CURRENT_SCOPE
    match rs_coroutine_core::CURRENT_SCOPE.try_with(|scope| scope.clone()) {
        Ok(scope) => {
            // We're in a scope - use structured spawning (preferred path)
            let cancel_token = scope.cancel_token.clone();
            let job = scope.launch(fut);
            ScopeAwareHandle {
                job,
                cancel_token,
                is_detached: false,
            }
        }
        Err(_) => {
            // No scope available - create detached task (lenient fallback)
            // This allows Flows to work in simpler contexts without requiring
            // full CoroutineScope setup, similar to Kotlin's Flow

            // WARN in debug builds - helps catch unintended detached spawns
            #[cfg(debug_assertions)]
            {
                eprintln!(
                    "⚠️  spawn_in_scope: No CoroutineScope found, using detached task fallback.\n\
                     → This task will NOT be part of structured concurrency hierarchy.\n\
                     → For production code, wrap Flow operations in `scope.launch()` or `coroutine_scope!`.\n\
                     → This warning only appears in debug builds."
                );
            }

            let cancel_token = CancelToken::new();
            let cancel_clone = cancel_token.clone();
            let job = JobHandle::new();
            let job_clone = job.clone();

            // Spawn detached task with cancellation support
            let handle = tokio::spawn(async move {
                // Wrap future to respect cancellation token
                tokio::select! {
                    biased;
                    _ = cancel_clone.cancelled() => {
                        // Task was cancelled via handle
                        job_clone.complete_with(Err(TaskError::Cancelled));
                    }
                    _ = fut => {
                        // Task completed normally
                        job_clone.complete();
                    }
                }
            });

            // Spawn observer to detect panics (same pattern as scope.launch)
            let job_observer = job.clone();
            tokio::spawn(async move {
                match handle.await {
                    Ok(()) => {
                        // Task completed (job.complete already called)
                    }
                    Err(join_err) if join_err.is_panic() => {
                        let panic_msg = rs_coroutine_core::error::extract_panic_message(join_err);
                        job_observer.complete_with(Err(TaskError::Panicked(panic_msg)));
                    }
                    Err(_) => {
                        job_observer.complete_with(Err(TaskError::Aborted));
                    }
                }
            });

            ScopeAwareHandle {
                job,
                cancel_token,
                is_detached: true,
            }
        }
    }
}

/// A handle to a task spawned in a CoroutineScope (or detached)
///
/// This wraps both the JobHandle (for completion tracking) and the CancelToken
/// (for cancellation). This design makes cancellation explicit and avoids
/// the dual-token hierarchy confusion.
///
/// The `is_detached` flag tracks whether this task was spawned within a scope
/// (structured concurrency) or as a standalone task (lenient fallback).
pub struct ScopeAwareHandle {
    job: JobHandle,
    cancel_token: CancelToken,
    #[allow(dead_code)]
    is_detached: bool,
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
