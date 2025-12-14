use rs_coroutine_core::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_hierarchical_cancellation_child_tokens() {
    let parent = CancelToken::new();
    let child = parent.child();
    let grandchild = child.child();

    // Initially nothing is cancelled
    assert!(!parent.is_cancelled());
    assert!(!child.is_cancelled());
    assert!(!grandchild.is_cancelled());

    // Cancel parent
    parent.cancel();

    // All descendants should be cancelled
    assert!(parent.is_cancelled());
    assert!(child.is_cancelled());
    assert!(grandchild.is_cancelled());
}

#[tokio::test]
async fn test_child_cancellation_doesnt_affect_parent() {
    let parent = CancelToken::new();
    let child = parent.child();

    // Cancel child
    child.cancel();

    // Child is cancelled but parent is not
    assert!(child.is_cancelled());
    assert!(!parent.is_cancelled());
}

#[tokio::test]
async fn test_scope_cancellation_propagates_to_child_scopes() {
    let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let child_cancelled = Arc::new(AtomicBool::new(false));

    // Launch a task that uses with_dispatcher (creates child scope)
    let child_cancelled_clone = Arc::clone(&child_cancelled);
    let scope_clone = Arc::clone(&scope);
    let job = scope.launch(async move {
        let flag = Arc::clone(&child_cancelled_clone);
        let result = scope_clone
            .with_dispatcher(Dispatchers::io(), async move {
                // Simulate some work that checks for cancellation
                for _ in 0..10 {
                    sleep(Duration::from_millis(10)).await;
                    // Check if child scope can see parent cancellation
                    if let Ok(current) = CURRENT_SCOPE.try_with(|s| s.is_cancelled()) {
                        if current {
                            flag.store(true, Ordering::SeqCst);
                            return;
                        }
                    }
                }
            })
            .await;

        // If we got Err(CancellationError), that also counts as seeing cancellation
        if result.is_err() {
            child_cancelled_clone.store(true, Ordering::SeqCst);
        }
    });

    // Let it start
    sleep(Duration::from_millis(10)).await;

    // Cancel the scope
    scope.cancel();

    // Wait for job to complete - no hacky sleep needed!
    job.join().await;

    // The child scope should have seen the parent cancellation
    assert!(child_cancelled.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_cancel_token_cancelled_await() {
    let token = CancelToken::new();
    let token_clone = token.clone();

    // Spawn a task that waits for cancellation
    let handle = tokio::spawn(async move {
        token_clone.cancelled().await;
        "cancelled"
    });

    // Give it a moment
    sleep(Duration::from_millis(10)).await;

    // Cancel the token
    token.cancel();

    // Should complete quickly
    let result = tokio::time::timeout(Duration::from_millis(100), handle)
        .await
        .expect("Should complete within timeout")
        .expect("Task should succeed");

    assert_eq!(result, "cancelled");
}

#[tokio::test]
async fn test_hierarchical_cancel_token_await() {
    let parent = CancelToken::new();
    let child = parent.child();
    let child_clone = child.clone();

    // Wait on child
    let handle = tokio::spawn(async move {
        child_clone.cancelled().await;
        "done"
    });

    sleep(Duration::from_millis(10)).await;

    // Cancel parent
    parent.cancel();

    // Child should be notified
    let result = tokio::time::timeout(Duration::from_millis(100), handle)
        .await
        .expect("Should complete within timeout")
        .expect("Task should succeed");

    assert_eq!(result, "done");
}

#[tokio::test]
async fn test_job_handle_child_cancellation() {
    let job = JobHandle::new();
    let child_job = job.new_child();

    // Initially not cancelled
    assert!(!job.is_cancelled());
    assert!(!child_job.is_cancelled());

    // Cancel parent job
    job.cancel();

    // Both should be cancelled due to hierarchical token
    assert!(job.is_cancelled());
    assert!(child_job.is_cancelled());
}

#[tokio::test]
async fn test_async_task_respects_scope_cancellation() {
    let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let started = Arc::new(AtomicBool::new(false));

    let started_clone = Arc::clone(&started);

    let deferred = scope.async_task(Dispatchers::io(), async move {
        started_clone.store(true, Ordering::SeqCst);
        // Simulate a long-running task that would normally complete
        sleep(Duration::from_millis(100)).await;
        42
    });

    // Let it start
    sleep(Duration::from_millis(20)).await;

    // Cancel scope
    scope.cancel();

    // Await the deferred - should return Err(CancellationError) immediately
    let result = deferred.await_result().await;

    // Task should have been cancelled
    assert!(result.is_err());
    assert!(scope.is_cancelled());
}

#[tokio::test]
async fn test_multiple_nested_scopes() {
    let root = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let deepest_saw_cancellation = Arc::new(AtomicBool::new(false));

    let flag_clone = Arc::clone(&deepest_saw_cancellation);
    let root_clone = Arc::clone(&root);

    let job = root.launch(async move {
        let root_clone2 = root_clone.clone();
        let flag_inner = Arc::clone(&flag_clone);
        let flag_mid = Arc::clone(&flag_clone);
        let result = root_clone
            .with_dispatcher(Dispatchers::io(), async move {
                let flag_innermost = Arc::clone(&flag_inner);
                let result2 = root_clone2
                    .with_dispatcher(Dispatchers::io(), async move {
                        // Simulate work with cooperative cancellation
                        for _ in 0..10 {
                            sleep(Duration::from_millis(10)).await;
                            if let Ok(current) = CURRENT_SCOPE.try_with(|s| s.is_cancelled()) {
                                if current {
                                    flag_innermost.store(true, Ordering::SeqCst);
                                    return;
                                }
                            }
                        }
                    })
                    .await;

                // If inner was cancelled, that counts too
                if result2.is_err() {
                    flag_inner.store(true, Ordering::SeqCst);
                }
            })
            .await;

        // If outer was cancelled, that counts too
        if result.is_err() {
            flag_mid.store(true, Ordering::SeqCst);
        }
    });

    // Let it start
    sleep(Duration::from_millis(10)).await;

    // Cancel root
    root.cancel();

    // Wait for job to complete - no hacky sleep needed!
    job.join().await;

    // The deepest nested scope should have seen the cancellation
    assert!(deepest_saw_cancellation.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_check_cancellation_function() {
    let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let result = Arc::new(AtomicBool::new(false));

    let result_clone = Arc::clone(&result);
    let job = scope.launch(async move {
        CURRENT_SCOPE
            .with(|_| async {
                for _ in 0..10 {
                    // Use the check_cancellation function
                    if check_cancellation().is_err() {
                        return;
                    }
                    yield_now().await;
                }
                result_clone.store(true, Ordering::SeqCst);
            })
            .await;
    });

    // Cancel immediately
    scope.cancel();

    // Wait
    job.join().await;

    // Should not have completed
    assert!(!result.load(Ordering::SeqCst));
}
