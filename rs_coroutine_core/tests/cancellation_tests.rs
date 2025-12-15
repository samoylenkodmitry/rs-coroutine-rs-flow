use rs_coroutine_core::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
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
    let work_completed = Arc::new(AtomicBool::new(false));
    let started = Arc::new(Notify::new());

    // Create notified future BEFORE launching task (Notify requires this)
    let notified = started.notified();

    // Launch a task that uses with_dispatcher (creates child scope)
    let work_clone = Arc::clone(&work_completed);
    let scope_clone = Arc::clone(&scope);
    let started_clone = Arc::clone(&started);
    let job = scope.launch(async move {
        let flag = Arc::clone(&work_clone);
        scope_clone
            .with_dispatcher(Dispatchers::io(), async move {
                // Signal that we've started
                started_clone.notify_one();

                // Simulate long-running work that should be interrupted
                // With forced cancellation, this should never complete
                sleep(Duration::from_millis(200)).await;
                flag.store(true, Ordering::SeqCst);
            })
            .await
            .ok(); // Ignore cancellation error
    });

    // Wait for task to start (deterministic)
    notified.await;

    // Give it a tiny bit of time to ensure it's in the sleep
    sleep(Duration::from_millis(10)).await;

    // Cancel the scope
    scope.cancel();

    // Wait for job to complete - should complete quickly due to forced cancellation
    let join_result = tokio::time::timeout(Duration::from_millis(100), job.join()).await;

    // Job should complete within timeout (forced cancellation is immediate)
    assert!(
        join_result.is_ok(),
        "Job should complete quickly with forced cancellation"
    );

    // Work should NOT have completed (task was interrupted)
    assert!(
        !work_completed.load(Ordering::SeqCst),
        "Work should be interrupted by forced cancellation"
    );
}

#[tokio::test]
async fn test_cancel_token_cancelled_await() {
    let token = CancelToken::new();
    let token_clone = token.clone();
    let started = Arc::new(Notify::new());
    let started_clone = Arc::clone(&started);

    // Create notified future BEFORE spawning task (Notify requires this)
    let notified = started.notified();

    // Spawn a task that waits for cancellation
    let handle = tokio::spawn(async move {
        started_clone.notify_one();
        token_clone.cancelled().await;
        "cancelled"
    });

    // Wait for task to start (deterministic)
    notified.await;

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
    let started = Arc::new(Notify::new());
    let started_clone = Arc::clone(&started);

    // Create notified future BEFORE spawning task (Notify requires this)
    let notified = started.notified();

    // Wait on child
    let handle = tokio::spawn(async move {
        started_clone.notify_one();
        child_clone.cancelled().await;
        "done"
    });

    // Wait for task to start (deterministic)
    notified.await;

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
    let started = Arc::new(Notify::new());

    // Create notified future BEFORE launching task (Notify requires this)
    let notified = started.notified();

    let started_clone = Arc::clone(&started);

    let deferred = scope.async_task(Dispatchers::io(), async move {
        started_clone.notify_one();
        // Simulate a long-running task that would normally complete
        sleep(Duration::from_millis(100)).await;
        42
    });

    // Wait for task to start (deterministic)
    notified.await;

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
    let work_completed = Arc::new(AtomicBool::new(false));
    let started = Arc::new(Notify::new());

    // Create notified future BEFORE launching task (Notify requires this)
    let notified = started.notified();

    let work_clone = Arc::clone(&work_completed);
    let root_clone = Arc::clone(&root);
    let started_clone = Arc::clone(&started);

    let job = root.launch(async move {
        let root_clone2 = root_clone.clone();
        let flag = Arc::clone(&work_clone);
        root_clone
            .with_dispatcher(Dispatchers::io(), async move {
                root_clone2
                    .with_dispatcher(Dispatchers::io(), async move {
                        // Signal we've started
                        started_clone.notify_one();

                        // Simulate long-running nested work that should be interrupted
                        // With forced cancellation through the entire scope tree,
                        // this should never complete
                        sleep(Duration::from_millis(200)).await;
                        flag.store(true, Ordering::SeqCst);
                    })
                    .await
                    .ok(); // Ignore cancellation error
            })
            .await
            .ok(); // Ignore cancellation error
    });

    // Wait for task to start (deterministic)
    notified.await;

    // Give it a tiny bit of time to ensure it's in the sleep
    sleep(Duration::from_millis(10)).await;

    // Cancel root
    root.cancel();

    // Wait for job to complete - should complete quickly due to forced cancellation
    let join_result = tokio::time::timeout(Duration::from_millis(100), job.join()).await;

    // Job should complete within timeout (forced cancellation propagates through scope tree)
    assert!(
        join_result.is_ok(),
        "Nested job should complete quickly with forced cancellation"
    );

    // Work should NOT have completed (task was interrupted at any nested level)
    assert!(
        !work_completed.load(Ordering::SeqCst),
        "Nested work should be interrupted by forced cancellation"
    );
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
